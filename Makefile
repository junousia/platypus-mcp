SHELL := /bin/sh

BAZEL ?= bazel
CARGO ?= cargo
ROOT ?= $(CURDIR)
MAX_TASKS ?= 1
NPM_CACHE_DIR ?= $(CURDIR)/.platy/npm-cache
NPM_REQUIRED_PLATFORMS ?=
TEST ?= //...
ARGS ?=

.DEFAULT_GOAL := help

.PHONY: \
	help \
	check ci lint fmt-check fmt format test test-one build release-build package release-check clean \
	run \
	feedback opencode-feedback claude-feedback codex-feedback \
	smoke smoke-queue smoke-storage \
	metadata version

help: ## Show categorized developer commands.
	@printf '\033[1mPlatypus MCP developer commands\033[0m\n'
	@printf 'Bazel-first Rust MCP server for deterministic project orchestration.\n\n'
	@printf '\033[1mUsage\033[0m\n'
	@printf '  make \033[36m<target>\033[0m [VARIABLE=value]\n\n'
	@printf '\033[1mPrimary Bazel Workflow\033[0m\n'
	@printf '  \033[36mcheck\033[0m         Run Bazel test and release artifact validation\n'
	@printf '  \033[36mci\033[0m            Alias for check\n'
	@printf '  \033[36mlint\033[0m          Run Bazel rustfmt test\n'
	@printf '  \033[36mfmt-check\033[0m     Run Bazel rustfmt test\n'
	@printf '  \033[36mfmt\033[0m           Run rules_rust rustfmt\n'
	@printf '  \033[36mformat\033[0m        Alias for fmt\n'
	@printf '  \033[36mtest\033[0m          Run Bazel tests; override TEST=//target\n'
	@printf '  \033[36mtest-one\033[0m      Run one Bazel test target; set TEST=//target\n'
	@printf '  \033[36mbuild\033[0m         Build all Bazel targets\n'
	@printf '  \033[36mrelease-build\033[0m Build stamped release artifacts\n'
	@printf '  \033[36mpackage\033[0m       Alias for release-build\n'
	@printf '  \033[36mrelease-check\033[0m Alias for check\n'
	@printf '  \033[36mclean\033[0m         Clean Bazel output\n\n'
	@printf '\033[1mRun\033[0m\n'
	@printf '  \033[36mrun\033[0m           Run stdio MCP server through Bazel\n\n'
	@printf '\033[1mHost Exercises\033[0m\n'
	@printf '  \033[36mfeedback\033[0m         Alias for opencode-feedback\n'
	@printf '  \033[36mopencode-feedback\033[0m Create a temp OpenCode project and ask for workflow feedback\n'
	@printf '  \033[36mclaude-feedback\033[0m   Create a temp Claude Code project and ask for workflow feedback\n'
	@printf '  \033[36mcodex-feedback\033[0m    Create a temp Codex project and ask for workflow feedback\n\n'
	@printf '\033[1mInspect\033[0m\n'
	@printf '  \033[36msmoke\033[0m       Invoke inspect_status through the stdio tool helper\n'
	@printf '  \033[36msmoke-queue\033[0m Invoke inspect_work_queue through the stdio tool helper\n'
	@printf '  \033[36msmoke-storage\033[0m Probe storage backend capabilities through MCP\n'
	@printf '  \033[36mmetadata\033[0m    Print Bazel module/dependency metadata\n'
	@printf '  \033[36mversion\033[0m     Print Bazel version\n\n'
	@printf '\033[1mVariables\033[0m\n'
	@printf '  BAZEL=%s\n' '$(BAZEL)'
	@printf '  TEST=%s\n' '$(TEST)'
	@printf '  ROOT=%s\n' '$(ROOT)'
	@printf '  MAX_TASKS=%s\n' '$(MAX_TASKS)'
	@printf '  NPM_REQUIRED_PLATFORMS=linux-x64,linux-arm64,darwin-x64,darwin-arm64 for strict npm binary validation\n'
	@printf '  PROJECT_ROOT=/path/to/new/temp/project for *-feedback\n'
	@printf '  FEEDBACK_DRY_RUN=1 to bootstrap and smoke-check without invoking the host\n'
	@printf '  FEEDBACK_TIMEOUT_SECONDS=600 to control host run timeout budget\n'
	@printf '  FEEDBACK_DANGEROUS=1 to enable each host dangerous auto-approval mode\n'
	@printf '  FEEDBACK_MODEL=<model> for all feedback targets\n'
	@printf '  OPENCODE_MODEL=<model> OPENCODE_AGENT=<agent> for opencode-feedback\n'
	@printf '  OPENCODE_DANGEROUS=1 to auto-approve OpenCode permissions in the temp project\n'
	@printf '  OPENCODE_TIMEOUT_SECONDS=600 to control OpenCode run timeout budget\n'
	@printf '  OPENCODE_ISOLATE_DATA=1 to run OpenCode with an isolated copied profile (default 0)\n'
	@printf '  OPENCODE_DRY_RUN=1 to bootstrap and smoke-check without invoking OpenCode\n'
	@printf '  OPENCODE_PRINT_LOGS=1 to include OpenCode diagnostic logs in the exercise\n'
	@printf '  CLAUDE_MODEL=<model> CLAUDE_AGENT=<agent> for claude-feedback\n'
	@printf '  CLAUDE_DANGEROUS=1 to auto-approve Claude permissions in the temp project\n'
	@printf '  CLAUDE_DRY_RUN=1 to bootstrap and smoke-check without invoking Claude\n'
	@printf '  CODEX_MODEL=<model> CODEX_PROFILE=<profile> for codex-feedback\n'
	@printf '  CODEX_DANGEROUS=1 to bypass Codex approvals and sandboxing in the temp project\n'
	@printf '  CODEX_DRY_RUN=1 to bootstrap and smoke-check without invoking Codex\n'

check: test release-build ## Run Bazel test and release artifact validation.

ci: check ## Alias for check.

lint: fmt-check ## Run Bazel rustfmt test.

fmt-check: ## Run Bazel rustfmt test.
	$(BAZEL) test //:rustfmt_test

fmt: ## Run rules_rust rustfmt.
	$(BAZEL) run @rules_rust//:rustfmt

format: fmt ## Alias for fmt.

test: ## Run Bazel tests; override TEST=//target.
	$(BAZEL) test $(TEST) $(ARGS)

test-one: ## Run one Bazel test target; set TEST=//target.
	@if [ -z "$(TEST)" ]; then \
		printf 'TEST is required. Example: make test-one TEST=//:stdio_protocol_test\n' >&2; \
		exit 2; \
	fi
	$(BAZEL) test $(TEST) $(ARGS)

build: ## Build all Bazel targets.
	$(BAZEL) build //... $(ARGS)

release-build: ## Build stamped release artifacts.
	$(BAZEL) build --config=release //:platypus_mcp_binary_tar //:release_metadata_tar $(ARGS)

package: release-build ## Alias for release-build.

release-check: check ## Alias for check.

clean: ## Clean Bazel output.
	$(BAZEL) clean

run: ## Run stdio MCP server through Bazel.
	$(BAZEL) run //:platypus-mcp -- $(ARGS)

feedback: opencode-feedback ## Alias for opencode-feedback.

opencode-feedback: ## Create a temp OpenCode project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/opencode-feedback.sh

claude-feedback: ## Create a temp Claude Code project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/claude-feedback.sh

codex-feedback: ## Create a temp Codex project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/codex-feedback.sh

smoke: ## Invoke inspect_status through the stdio tool helper.
	$(BAZEL) run //:platypus-mcp -- tool --root "$(ROOT)" inspect_status '{"limit":5}'

smoke-queue: ## Invoke inspect_work_queue through the stdio tool helper.
	$(BAZEL) run //:platypus-mcp -- tool --root "$(ROOT)" inspect_work_queue '{"limit":5}'

smoke-storage: ## Probe storage backend capabilities through MCP.
	$(BAZEL) run //:platypus-mcp -- tool --root "$(ROOT)" storage_capability_probe '{}'

metadata: ## Print Bazel module/dependency metadata.
	$(BAZEL) mod graph

version: ## Print Bazel version.
	$(BAZEL) version
