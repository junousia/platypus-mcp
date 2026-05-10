#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cargo_bin=${CARGO:-cargo}
project_root=${PROJECT_ROOT:-}
keep_project=${KEEP_PROJECT:-1}
opencode_model=${OPENCODE_MODEL:-}
opencode_agent=${OPENCODE_AGENT:-}
dangerous=${OPENCODE_DANGEROUS:-0}
dry_run=${OPENCODE_DRY_RUN:-0}
print_logs=${OPENCODE_PRINT_LOGS:-0}
opencode_timeout_seconds=${OPENCODE_TIMEOUT_SECONDS:-600}
opencode_isolate_data=${OPENCODE_ISOLATE_DATA:-0}
created_temp_project=0

if ! command -v opencode >/dev/null 2>&1; then
  printf 'opencode is required on PATH.\n' >&2
  exit 127
fi
if ! command -v python3 >/dev/null 2>&1; then
  printf 'python3 is required to patch the generated OpenCode config.\n' >&2
  exit 127
fi

if [ -z "$project_root" ]; then
  project_root=$(mktemp -d "${TMPDIR:-/tmp}/platypus-opencode-feedback-XXXXXX")
  created_temp_project=1
elif [ -e "$project_root" ]; then
  printf 'PROJECT_ROOT already exists: %s\n' "$project_root" >&2
  exit 2
else
  mkdir -p "$project_root"
fi

feedback_file="$project_root/FEEDBACK.md"
stdout_dir="$project_root/.platy/feedback"
stdout_file="$stdout_dir/opencode-output.txt"
config_file="$project_root/opencode.json"
opencode_profile_root="$project_root/.platy/opencode-profile"

cleanup() {
  if [ "$keep_project" = "0" ] && [ "$created_temp_project" = "1" ]; then
    rm -rf "$project_root"
  fi
}
trap cleanup EXIT

printf '==> Project: %s\n' "$project_root"
printf '==> Building local platypus-mcp\n'
(cd "$repo_root" && "$cargo_bin" build --quiet)

printf '==> Bootstrapping OpenCode project config and Platypus scaffold\n'
(cd "$repo_root" && "$cargo_bin" run --quiet -- bootstrap opencode \
  --root "$project_root" \
  --config "$config_file" \
  --init-project \
  --project-name "OpenCode Feedback Exercise" \
  --force)

printf '==> Rebinding OpenCode MCP config to this source checkout\n'
python3 - "$config_file" "$repo_root" "$project_root" <<'PY'
import json
import sys
from pathlib import Path

config = Path(sys.argv[1])
repo_root = sys.argv[2]
project_root = sys.argv[3]
data = json.loads(config.read_text())
entry = data.setdefault("mcp", {}).setdefault("platypus", {})
entry["type"] = "local"
entry["enabled"] = True
entry["command"] = ["sh", "-lc", f"cd {json.dumps(repo_root)} && exec cargo run --quiet --"]
entry["environment"] = {"PLATYPUS_MCP_ROOT": project_root}
config.write_text(json.dumps(data, indent=2) + "\n")
PY

printf '==> Initializing Git baseline\n'
git -C "$project_root" init -q
git -C "$project_root" config user.name "Platypus OpenCode Exercise"
git -C "$project_root" config user.email "platypus-opencode@example.invalid"
git -C "$project_root" add --all
git -C "$project_root" commit -q -m "Initialize OpenCode feedback project"

printf '==> Running Platypus smoke checks\n'
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_status '{"limit":5}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_workflow_config '{}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_work_queue '{"limit":5,"require_task_plan":true}')
mkdir -p "$stdout_dir"
if [ "$dry_run" = "1" ]; then
  printf '==> Dry run complete; OpenCode was not invoked.\n'
  printf '==> Project kept: %s\n' "$project_root"
  printf '==> OpenCode config: %s\n' "$config_file"
  exit 0
fi

prompt='You are testing Platypus MCP through OpenCode in this fresh temporary repository.

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
2. First call `plan_goal_work` for this goal: "Build a simple FastAPI + React web app with authentication and a dashboard".
3. Use the concrete next tool guidance from `plan_goal_work`; normally this means calling `start_goal_work` with the recommended arguments.
4. Report whether the `plan_goal_work` -> `start_goal_work` split made the workflow clearer or added unnecessary ceremony.
5. Keep this exercise lightweight: do NOT run heavy scaffolding commands (no `npm install`, no long generator flows). Create tiny placeholder files if needed.
6. Move at most one tracked item through queue/dispatch/lifecycle boundaries just enough to expose workflow friction; do not fully implement the app.
7. If a step fails, use the structured tool output to recover or explain the blocker.
8. End with a concise feedback report in Markdown.
9. Write that final feedback report to FEEDBACK.md in this project root.

The final feedback report must include:
- How `plan_goal_work` behaved
- How `start_goal_work` behaved
- What worked
- Pain points
- Missing or confusing tool behavior
- Whether the workflow felt too heavy, too loose, or about right
- Concrete changes you recommend for Platypus

Be direct and honest. The feedback is the main output of this exercise.'

printf '==> Asking OpenCode to exercise Platypus and write feedback\n'
printf '==> OpenCode timeout budget: %ss (override with OPENCODE_TIMEOUT_SECONDS)\n' "$opencode_timeout_seconds"
if [ "$opencode_isolate_data" = "1" ]; then
  printf '==> Isolating OpenCode profile for this run to avoid shared WAL lock contention\n'
  mkdir -p "$opencode_profile_root/data" "$opencode_profile_root/config" "$opencode_profile_root/state" "$opencode_profile_root/cache" "$opencode_profile_root/tmp"
  if [ -d "$HOME/.config/opencode" ] && [ ! -f "$opencode_profile_root/config/.seeded" ]; then
    cp -a "$HOME/.config/opencode/." "$opencode_profile_root/config/"
    : >"$opencode_profile_root/config/.seeded"
  fi
  export XDG_DATA_HOME="$opencode_profile_root/data"
  export XDG_CONFIG_HOME="$opencode_profile_root/config"
  export XDG_STATE_HOME="$opencode_profile_root/state"
  export XDG_CACHE_HOME="$opencode_profile_root/cache"
  export TMPDIR="$opencode_profile_root/tmp"
  printf '==> OpenCode profile root: %s\n' "$opencode_profile_root"
fi
set -- opencode run --dir "$project_root" --format default
if [ "$print_logs" = "1" ]; then
  set -- "$@" --print-logs --log-level INFO
fi
if [ -n "$opencode_model" ]; then
  set -- "$@" --model "$opencode_model"
fi
if [ -n "$opencode_agent" ]; then
  set -- "$@" --agent "$opencode_agent"
fi
if [ "$dangerous" = "1" ]; then
  set -- "$@" --dangerously-skip-permissions
fi
set +e
attempt=1
max_attempts=3
status=1
while [ "$attempt" -le "$max_attempts" ]; do
  timeout "${opencode_timeout_seconds}" "$@" "$prompt" >"$stdout_file" 2>&1
  status=$?
  if [ "$status" -eq 0 ]; then
    break
  fi
  if [ "$status" -eq 124 ]; then
    printf 'timeout: OpenCode run exceeded %ss and was terminated.\n' "$opencode_timeout_seconds"
    break
  fi
  if ! grep -q "wal_checkpoint(PASSIVE)" "$stdout_file"; then
    break
  fi
  if [ "$attempt" -ge "$max_attempts" ]; then
    break
  fi
  printf 'retry: OpenCode SQLite WAL checkpoint lock detected (attempt %s/%s); retrying...\n' "$attempt" "$max_attempts"
  sleep 2
  attempt=$((attempt + 1))
done
set -e
cat "$stdout_file"

if [ ! -s "$feedback_file" ]; then
  printf 'warning: OpenCode did not write FEEDBACK.md; preserving stdout as fallback feedback.\n' >&2
  cp "$stdout_file" "$feedback_file"
fi

printf '\n==> Feedback saved: %s\n' "$feedback_file"
printf '==> OpenCode stdout: %s\n' "$stdout_file"
printf '==> Project kept: %s\n' "$project_root"
if [ "$dangerous" != "1" ]; then
  printf 'hint: set OPENCODE_DANGEROUS=1 to auto-approve OpenCode permissions for this temp exercise.\n'
fi
exit "$status"
