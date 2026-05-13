#!/usr/bin/env sh
set -eu

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <opencode|claude|codex>\n' "$0" >&2
  exit 2
fi

harness=$1
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cargo_bin=${CARGO:-cargo}
project_root=${PROJECT_ROOT:-}
keep_project=${KEEP_PROJECT:-1}
created_temp_project=0

case "$harness" in
  opencode)
    display_name=OpenCode
    executable=${OPENCODE_BIN:-opencode}
    model=${OPENCODE_MODEL:-${FEEDBACK_MODEL:-}}
    agent=${OPENCODE_AGENT:-}
    profile=${OPENCODE_PROFILE:-}
    dangerous=${OPENCODE_DANGEROUS:-${FEEDBACK_DANGEROUS:-0}}
    dry_run=${OPENCODE_DRY_RUN:-${FEEDBACK_DRY_RUN:-0}}
    print_logs=${OPENCODE_PRINT_LOGS:-${FEEDBACK_PRINT_LOGS:-0}}
    timeout_seconds=${OPENCODE_TIMEOUT_SECONDS:-${FEEDBACK_TIMEOUT_SECONDS:-600}}
    isolate_data=${OPENCODE_ISOLATE_DATA:-${FEEDBACK_ISOLATE_DATA:-0}}
    config_file_name=opencode.json
    output_file_name=opencode-output.txt
    temp_prefix=platypus-opencode-feedback
    ;;
  claude)
    display_name="Claude Code"
    executable=${CLAUDE_BIN:-claude}
    model=${CLAUDE_MODEL:-${FEEDBACK_MODEL:-}}
    agent=${CLAUDE_AGENT:-}
    profile=
    dangerous=${CLAUDE_DANGEROUS:-${FEEDBACK_DANGEROUS:-0}}
    dry_run=${CLAUDE_DRY_RUN:-${FEEDBACK_DRY_RUN:-0}}
    print_logs=${CLAUDE_PRINT_LOGS:-${FEEDBACK_PRINT_LOGS:-0}}
    timeout_seconds=${CLAUDE_TIMEOUT_SECONDS:-${FEEDBACK_TIMEOUT_SECONDS:-600}}
    isolate_data=${CLAUDE_ISOLATE_DATA:-${FEEDBACK_ISOLATE_DATA:-0}}
    config_file_name=.mcp.json
    output_file_name=claude-output.txt
    temp_prefix=platypus-claude-feedback
    ;;
  codex)
    display_name=Codex
    executable=${CODEX_BIN:-codex}
    model=${CODEX_MODEL:-${FEEDBACK_MODEL:-}}
    agent=
    profile=${CODEX_PROFILE:-}
    dangerous=${CODEX_DANGEROUS:-${FEEDBACK_DANGEROUS:-0}}
    dry_run=${CODEX_DRY_RUN:-${FEEDBACK_DRY_RUN:-0}}
    print_logs=${CODEX_PRINT_LOGS:-${FEEDBACK_PRINT_LOGS:-0}}
    timeout_seconds=${CODEX_TIMEOUT_SECONDS:-${FEEDBACK_TIMEOUT_SECONDS:-600}}
    isolate_data=${CODEX_ISOLATE_DATA:-${FEEDBACK_ISOLATE_DATA:-0}}
    config_file_name=.codex/config.toml
    output_file_name=codex-output.txt
    temp_prefix=platypus-codex-feedback
    ;;
  *)
    printf 'unsupported feedback harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac

if ! command -v "$executable" >/dev/null 2>&1; then
  printf '%s is required on PATH.\n' "$executable" >&2
  exit 127
fi
if ! command -v python3 >/dev/null 2>&1; then
  printf 'python3 is required to patch the generated host config.\n' >&2
  exit 127
fi

if [ -z "$project_root" ]; then
  project_root=$(mktemp -d "${TMPDIR:-/tmp}/${temp_prefix}-XXXXXX")
  created_temp_project=1
elif [ -e "$project_root" ]; then
  printf 'PROJECT_ROOT already exists: %s\n' "$project_root" >&2
  exit 2
else
  mkdir -p "$project_root"
fi

feedback_file="$project_root/FEEDBACK.md"
stdout_dir="$project_root/.platy/feedback"
stdout_file="$stdout_dir/$output_file_name"
config_file="$project_root/$config_file_name"
profile_root="$project_root/.platy/${harness}-profile"

cleanup() {
  if [ "$keep_project" = "0" ] && [ "$created_temp_project" = "1" ]; then
    rm -rf "$project_root"
  fi
}
trap cleanup EXIT

printf '==> Project: %s\n' "$project_root"
printf '==> Harness: %s\n' "$display_name"
printf '==> Building local platypus-mcp\n'
(cd "$repo_root" && "$cargo_bin" build --quiet)

printf '==> Bootstrapping %s project config and Platypus scaffold\n' "$display_name"
(cd "$repo_root" && "$cargo_bin" run --quiet -- bootstrap "$harness" \
  --root "$project_root" \
  --config "$config_file" \
  --init-project \
  --project-name "$display_name Feedback Exercise" \
  --force)

printf '==> Rebinding %s MCP config to this source checkout\n' "$display_name"
python3 - "$harness" "$config_file" "$repo_root" "$project_root" "$cargo_bin" <<'PY'
import json
import shlex
import sys
from pathlib import Path

host = sys.argv[1]
config = Path(sys.argv[2])
repo_root = sys.argv[3]
project_root = sys.argv[4]
cargo_bin = sys.argv[5]
server_command = f"cd {shlex.quote(repo_root)} && exec {shlex.quote(cargo_bin)} run --quiet --"

config.parent.mkdir(parents=True, exist_ok=True)
if host == "codex":
    config.write_text(
        "\n".join(
            [
                '[mcp_servers."platypus"]',
                'command = "sh"',
                f"args = [\"-lc\", {json.dumps(server_command)}]",
                f"env = {{ PLATYPUS_MCP_ROOT = {json.dumps(project_root)} }}",
                "",
            ]
        )
    )
    raise SystemExit(0)

if config.exists() and config.read_text().strip():
    data = json.loads(config.read_text())
else:
    data = {}

if host == "opencode":
    data.setdefault("$schema", "https://opencode.ai/config.json")
    entry = data.setdefault("mcp", {}).setdefault("platypus", {})
    entry.clear()
    entry.update(
        {
            "type": "local",
            "enabled": True,
            "command": ["sh", "-lc", server_command],
            "environment": {"PLATYPUS_MCP_ROOT": project_root},
        }
    )
else:
    entry = data.setdefault("mcpServers", {}).setdefault("platypus", {})
    entry.clear()
    entry.update(
        {
            "command": "sh",
            "args": ["-lc", server_command],
            "env": {"PLATYPUS_MCP_ROOT": project_root},
        }
    )

config.write_text(json.dumps(data, indent=2) + "\n")
PY

printf '==> Initializing Git baseline\n'
git -C "$project_root" init -q
git -C "$project_root" config user.name "Platypus $display_name Exercise"
git -C "$project_root" config user.email "platypus-${harness}@example.invalid"
git -C "$project_root" add --all
git -C "$project_root" commit -q -m "Initialize $display_name feedback project"

printf '==> Running Platypus smoke checks\n'
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_status '{"limit":5}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_workflow_config '{}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_work_queue '{"limit":5,"require_task_plan":true}')
mkdir -p "$stdout_dir"
if [ "$dry_run" = "1" ]; then
  printf '==> Dry run complete; %s was not invoked.\n' "$display_name"
  printf '==> Project kept: %s\n' "$project_root"
  printf '==> %s config: %s\n' "$display_name" "$config_file"
  exit 0
fi

prompt="You are testing Platypus MCP through $display_name in this fresh temporary repository.

Use the platypus MCP tools when possible. Do not edit files outside this project root.

Important framing:
- Platypus is optimized for long-term product development with traceability,
  repeatable planning, and durable task lifecycle state.
- Platypus is not optimized for one-shot throwaway coding tasks; if a path
  feels heavy for one-shot work, evaluate whether that is expected by design
  versus a real workflow defect.
- Distinguish product bugs from intentional structure needed for sustained,
  multi-step development.

Exercise goal:
1. Inspect the project state and available Platypus workflow.
2. Read the tool preload guidance if your harness exposes Platypus resources,
   prompts, or schema discovery. Use the named groups only when useful; if your
   harness cannot preload schemas, say so in the feedback.
3. Use the host model, user intent, or optional sampling to decide whether this goal should be direct work, backlog tracking, or planned work.
4. Create concrete backlog items with create_backlog_items if tracking is useful; validate them and inspect the queue.
5. Report whether the deterministic tool surface plus documentation made the workflow clearer or added unnecessary ceremony.
6. Keep this exercise lightweight: do NOT run heavy scaffolding commands (no npm install, no long generator flows). Create tiny placeholder files if needed.
7. Move at most one tracked item through queue/dispatch/lifecycle boundaries just enough to expose workflow friction; do not fully implement the app.
8. If a step fails, use the structured tool output to recover or explain the blocker.
9. End with a concise feedback report in Markdown.
10. Write that final feedback report to FEEDBACK.md in this project root.

The final feedback report must include:
- How the deterministic planning guidance behaved
- Whether the named schema preload groups reduced cold-start friction, were ignored by the harness, or were unavailable
- How backlog creation and queue inspection behaved
- What worked
- Pain points
- Missing or confusing tool behavior
- Whether the workflow felt too heavy, too loose, or about right
- Concrete changes you recommend for Platypus

Be direct and honest. The feedback is the main output of this exercise."

printf '==> Asking %s to exercise Platypus and write feedback\n' "$display_name"
printf '==> %s timeout budget: %ss (override with %s_TIMEOUT_SECONDS or FEEDBACK_TIMEOUT_SECONDS)\n' "$display_name" "$timeout_seconds" "$(printf '%s' "$harness" | tr '[:lower:]' '[:upper:]')"

if [ "$isolate_data" = "1" ]; then
  printf '==> Isolating %s profile for this run\n' "$display_name"
  mkdir -p "$profile_root/data" "$profile_root/config" "$profile_root/state" "$profile_root/cache" "$profile_root/tmp"
  if [ "$harness" = "opencode" ] && [ -d "$HOME/.config/opencode" ] && [ ! -f "$profile_root/config/.seeded" ]; then
    cp -a "$HOME/.config/opencode/." "$profile_root/config/"
    : >"$profile_root/config/.seeded"
  fi
  export XDG_DATA_HOME="$profile_root/data"
  export XDG_CONFIG_HOME="$profile_root/config"
  export XDG_STATE_HOME="$profile_root/state"
  export XDG_CACHE_HOME="$profile_root/cache"
  export TMPDIR="$profile_root/tmp"
  printf '==> %s profile root: %s\n' "$display_name" "$profile_root"
fi

case "$harness" in
  opencode)
    set -- "$executable" run --dir "$project_root" --format default
    if [ "$print_logs" = "1" ]; then
      set -- "$@" --print-logs --log-level INFO
    fi
    if [ -n "$model" ]; then
      set -- "$@" --model "$model"
    fi
    if [ -n "$agent" ]; then
      set -- "$@" --agent "$agent"
    fi
    if [ "$dangerous" = "1" ]; then
      set -- "$@" --dangerously-skip-permissions
    fi
    ;;
  claude)
    set -- "$executable" --print --output-format text --mcp-config "$config_file" --strict-mcp-config --add-dir "$project_root" --add-dir "$repo_root"
    if [ "$print_logs" = "1" ]; then
      set -- "$@" --debug
    fi
    if [ -n "$model" ]; then
      set -- "$@" --model "$model"
    fi
    if [ -n "$agent" ]; then
      set -- "$@" --agent "$agent"
    fi
    if [ "$dangerous" = "1" ]; then
      set -- "$@" --dangerously-skip-permissions
    fi
    ;;
  codex)
    last_message_file="$stdout_dir/codex-last-message.md"
    set -- "$executable" exec -C "$project_root" --add-dir "$repo_root" --sandbox workspace-write --output-last-message "$last_message_file"
    if [ "$print_logs" = "1" ]; then
      set -- "$@" --json
    fi
    if [ -n "$model" ]; then
      set -- "$@" --model "$model"
    fi
    if [ -n "$profile" ]; then
      set -- "$@" --profile "$profile"
    fi
    if [ "$dangerous" = "1" ]; then
      set -- "$@" --dangerously-bypass-approvals-and-sandbox
    fi
    ;;
esac

set +e
attempt=1
max_attempts=3
status=1
while [ "$attempt" -le "$max_attempts" ]; do
  (cd "$project_root" && timeout "$timeout_seconds" "$@" "$prompt" >"$stdout_file" 2>&1)
  status=$?
  if [ "$status" -eq 0 ]; then
    break
  fi
  if [ "$status" -eq 124 ]; then
    printf 'timeout: %s run exceeded %ss and was terminated.\n' "$display_name" "$timeout_seconds"
    break
  fi
  if ! grep -q "wal_checkpoint(PASSIVE)" "$stdout_file"; then
    break
  fi
  if [ "$attempt" -ge "$max_attempts" ]; then
    break
  fi
  printf 'retry: %s SQLite WAL checkpoint lock detected (attempt %s/%s); retrying...\n' "$display_name" "$attempt" "$max_attempts"
  sleep 2
  attempt=$((attempt + 1))
done
set -e
cat "$stdout_file"

if [ ! -s "$feedback_file" ]; then
  if [ "$harness" = "codex" ] && [ -s "$stdout_dir/codex-last-message.md" ]; then
    printf 'warning: Codex did not write FEEDBACK.md; preserving last message as fallback feedback.\n' >&2
    cp "$stdout_dir/codex-last-message.md" "$feedback_file"
  else
    printf 'warning: %s did not write FEEDBACK.md; preserving stdout as fallback feedback.\n' "$display_name" >&2
    cp "$stdout_file" "$feedback_file"
  fi
fi

printf '\n==> Feedback saved: %s\n' "$feedback_file"
printf '==> %s stdout: %s\n' "$display_name" "$stdout_file"
printf '==> Project kept: %s\n' "$project_root"
case "$harness" in
  opencode)
    if [ "$dangerous" != "1" ]; then
      printf 'hint: set OPENCODE_DANGEROUS=1 to auto-approve OpenCode permissions for this temp exercise.\n'
    fi
    ;;
  claude)
    if [ "$dangerous" != "1" ]; then
      printf 'hint: set CLAUDE_DANGEROUS=1 to auto-approve Claude permissions for this temp exercise.\n'
    fi
    ;;
  codex)
    if [ "$dangerous" != "1" ]; then
      printf 'hint: set CODEX_DANGEROUS=1 to bypass Codex approvals and sandboxing for this temp exercise.\n'
    fi
    ;;
esac
exit "$status"
