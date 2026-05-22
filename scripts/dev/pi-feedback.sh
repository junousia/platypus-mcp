#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cargo_bin=${CARGO:-cargo}
pi_bin=${PI_BIN:-pi}
project_root=${PROJECT_ROOT:-}
keep_project=${KEEP_PROJECT:-1}
dry_run=${PI_DRY_RUN:-${FEEDBACK_DRY_RUN:-1}}
timeout_seconds=${PI_TIMEOUT_SECONDS:-${FEEDBACK_TIMEOUT_SECONDS:-900}}
model=${PI_MODEL:-${FEEDBACK_MODEL:-gpt-5.5}}
provider=${PI_PROVIDER:-}
created_temp_project=0

if [ -z "$project_root" ]; then
  project_root=$(mktemp -d "${TMPDIR:-/tmp}/platypus-pi-feedback-XXXXXX")
  created_temp_project=1
elif [ -e "$project_root" ]; then
  printf 'PROJECT_ROOT already exists: %s\n' "$project_root" >&2
  exit 2
else
  mkdir -p "$project_root"
fi

feedback_file="$project_root/FEEDBACK.md"
stdout_dir="$project_root/.platy/feedback"
stdout_file="$stdout_dir/pi-output.txt"
prompt_file="$stdout_dir/pi-feedback-prompt.md"
settings_file="$project_root/.pi/settings.json"

cleanup() {
  if [ "$keep_project" = "0" ] && [ "$created_temp_project" = "1" ]; then
    rm -rf "$project_root"
  fi
}
trap cleanup EXIT

printf '==> Project: %s\n' "$project_root"
printf '==> Harness: Pi\n'
printf '==> Building local platypus-mcp\n'
(cd "$repo_root" && "$cargo_bin" build --quiet)

printf '==> Bootstrapping Pi package settings and Platypus scaffold\n'
(cd "$repo_root" && "$cargo_bin" run --quiet -- bootstrap pi \
  --root "$project_root" \
  --init-project \
  --project-name "Pi Feedback Exercise" \
  --force)

printf '==> Rebinding Pi package settings to this source checkout\n'
node - "$settings_file" "$repo_root" <<'NODE'
const fs = require("node:fs");
const [settingsFile, repoRoot] = process.argv.slice(2);
const settings = JSON.parse(fs.readFileSync(settingsFile, "utf8"));
const packages = Array.isArray(settings.packages) ? settings.packages : [];
const withoutPlatypus = packages.filter((entry) => {
  const source = typeof entry === "string" ? entry : entry?.source;
  return !(source === "platypus-pi" || source === "npm:platypus-pi" || String(source ?? "").startsWith("npm:platypus-pi@"));
});
withoutPlatypus.push(repoRoot);
settings.packages = withoutPlatypus;
fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2) + "\n");
NODE

printf '==> Initializing Git baseline\n'
git -C "$project_root" init -q
git -C "$project_root" config user.name "Platypus Pi Exercise"
git -C "$project_root" config user.email "platypus-pi@example.invalid"
git -C "$project_root" add --all
git -C "$project_root" commit -q -m "Initialize Pi feedback project"

printf '==> Validating package and core Platypus tool availability\n'
(cd "$repo_root" && npm_config_cache="$repo_root/.platy/npm-cache" npm run package:validate)
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_session '{"limit":5,"detail":"compact"}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_work_queue '{"limit":5}')
(cd "$repo_root" && "$cargo_bin" run --quiet -- tool --root "$project_root" inspect_toolsets '{"limit":10}')

mkdir -p "$stdout_dir"
cat >"$prompt_file" <<'PROMPT'
You are testing Platypus MCP through Pi in this fresh temporary repository.

Use the Platypus Pi extension tools when possible. Do not edit files outside this project root.

Important framing:
- Platypus is optimized for long-term product development with durable product direction, backlog traceability, implementation plans, findings, evidence, and closure.
- Platypus is not optimized for one-shot throwaway scaffolding. If a path feels heavy for one-shot work, explain whether that is expected by design or a real product defect.
- Judge whether the Pi experience makes sustainable product development feel guided rather than ceremonial.

Exercise goal:
1. Inspect the current Platypus session and queue.
2. Use /platy-direction, /platy-standards, /platy-story-review, /platy-plan-review, /platy-start, and /platy-review-result when they fit the flow.
3. Create or refine a small set of concrete backlog items for a long-lived toy product goal.
4. Move at most one direct item far enough to evaluate completion and closure behavior.
5. Keep implementation tiny: create placeholder files only when useful. Do not run heavy generators or package installs.
6. If a step fails, use the structured tool output and Pi command guidance to recover or explain the blocker.
7. End with a concise feedback report in Markdown.
8. Write that final feedback report to FEEDBACK.md in this project root.

The final feedback report must include:
- Setup and package bootstrap quality
- Pi command discoverability and UI usefulness
- Tool schema clarity
- Durable direction, story review, implementation-plan review, and result-review behavior
- Backlog creation and queue inspection behavior
- Completion, findings, evidence, and closure behavior
- What worked
- Pain points
- Missing or confusing behavior
- Whether the workflow felt too heavy, too loose, or about right for long-term product development
- Concrete changes you recommend for Platypus

Be direct and honest. The feedback is the main output of this exercise.
PROMPT

if [ "$dry_run" = "1" ]; then
  cat >"$feedback_file" <<EOF
# Pi Feedback Dry Run

Pi live execution was skipped because PI_DRY_RUN=1.

Validated:
- local platypus-mcp builds
- Pi package settings bootstrap with --init-project
- Pi settings were rebound to the source checkout
- package validation passed
- inspect_session, inspect_work_queue, and inspect_toolsets ran against the temporary project

Run live mode with:

\`\`\`bash
PI_DRY_RUN=0 make pi-feedback
\`\`\`

Temporary project:

\`\`\`
$project_root
\`\`\`
EOF
  printf '==> Dry run complete; Pi was not invoked.\n'
  printf '==> Feedback saved: %s\n' "$feedback_file"
  printf '==> Prompt saved: %s\n' "$prompt_file"
  printf '==> Project kept: %s\n' "$project_root"
  exit 0
fi

if ! command -v "$pi_bin" >/dev/null 2>&1; then
  printf 'Pi executable is required for live mode: %s\n' "$pi_bin" >&2
  printf 'hint: set PI_DRY_RUN=1 for setup-only validation.\n' >&2
  exit 127
fi

printf '==> Asking Pi to exercise Platypus and write feedback\n'
printf '==> Pi timeout budget: %ss (override with PI_TIMEOUT_SECONDS or FEEDBACK_TIMEOUT_SECONDS)\n' "$timeout_seconds"

set -- "$pi_bin" -p --no-session
if [ -n "$provider" ]; then
  set -- "$@" --provider "$provider"
fi
if [ -n "$model" ]; then
  set -- "$@" --model "$model"
fi

set +e
(cd "$project_root" && timeout "$timeout_seconds" "$@" "$(cat "$prompt_file")" >"$stdout_file" 2>&1)
status=$?
set -e
cat "$stdout_file"

if [ ! -s "$feedback_file" ]; then
  printf 'warning: Pi did not write FEEDBACK.md; preserving stdout as fallback feedback.\n' >&2
  cp "$stdout_file" "$feedback_file"
fi

printf '\n==> Feedback saved: %s\n' "$feedback_file"
printf '==> Pi stdout: %s\n' "$stdout_file"
printf '==> Project kept: %s\n' "$project_root"
exit "$status"
