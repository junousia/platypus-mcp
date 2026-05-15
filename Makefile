SHELL := /bin/sh

CARGO ?= cargo
ROOT ?= $(CURDIR)
MAX_TASKS ?= 1
NPM_CACHE_DIR ?= $(CURDIR)/.platy/npm-cache
NPM_REQUIRED_PLATFORMS ?=
TEST ?=
ARGS ?=

.DEFAULT_GOAL := help

.PHONY: \
	help \
	check ci lint fmt-check fmt format test test-lib test-stdio test-one \
	build doc clean package publish-dry-run publish npm-package npm-package-dry-run npm-publish-dry-run npm-publish release-check \
	run run-root smoke smoke-queue smoke-storage \
	feedback opencode-feedback claude-feedback codex-feedback \
	metadata version

help: ## Show categorized developer commands.
	@printf '\033[1mPlatypus MCP developer commands\033[0m\n'
	@printf 'Local-first Rust MCP server for deterministic project orchestration.\n\n'
	@printf '\033[1mUsage\033[0m\n'
	@printf '  make \033[36m<target>\033[0m [VARIABLE=value]\n\n'
	@printf '\033[1mVerification\033[0m\n'
	@printf '  \033[36mcheck\033[0m       Format check, build-check, and run all tests\n'
	@printf '  \033[36mci\033[0m          Alias for check\n'
	@printf '  \033[36mlint\033[0m        cargo fmt --check + cargo check\n'
	@printf '  \033[36mfmt-check\033[0m   Check Rust formatting\n'
	@printf '  \033[36mfmt\033[0m         Apply Rust formatting\n'
	@printf '  \033[36mformat\033[0m      Alias for fmt\n\n'
	@printf '\033[1mTests\033[0m\n'
	@printf '  \033[36mtest\033[0m        Run all tests\n'
	@printf '  \033[36mtest-lib\033[0m    Run library/unit tests\n'
	@printf '  \033[36mtest-stdio\033[0m  Run stdio protocol integration tests\n'
	@printf '  \033[36mtest-one\033[0m    Run a filtered test: make test-one TEST=name\n\n'
	@printf '\033[1mBuild And Docs\033[0m\n'
	@printf '  \033[36mbuild\033[0m       Build debug binary\n'
	@printf '  \033[36mdoc\033[0m         Build Rust API docs without dependencies\n'
	@printf '  \033[36mpackage\033[0m     Verify crates.io package contents\n'
	@printf '  \033[36mpublish-dry-run\033[0m Validate crates.io publishing without uploading\n'
	@printf '  \033[36mpublish\033[0m     Publish the crate to crates.io\n'
	@printf '  \033[36mnpm-package\033[0m Validate npm package contents\n'
	@printf '  \033[36mnpm-package-dry-run\033[0m Show npm package dry-run output\n'
	@printf '  \033[36mnpm-publish-dry-run\033[0m Validate npm publish without uploading\n'
	@printf '  \033[36mnpm-publish\033[0m Publish the npm package\n'
	@printf '  \033[36mrelease-check\033[0m Validate Rust and npm release packaging\n'
	@printf '  \033[36mclean\033[0m       Remove Cargo build output\n\n'
	@printf '\033[1mRun\033[0m\n'
	@printf '  \033[36mrun\033[0m         Run stdio MCP server in the current directory\n'
	@printf '  \033[36mrun-root\033[0m    Run stdio MCP server with ROOT=/path/to/project\n\n'
	@printf '\033[1mHost Exercises\033[0m\n'
	@printf '  \033[36mfeedback\033[0m         Alias for opencode-feedback\n'
	@printf '  \033[36mopencode-feedback\033[0m Create a temp OpenCode project and ask for workflow feedback\n'
	@printf '  \033[36mclaude-feedback\033[0m   Create a temp Claude Code project and ask for workflow feedback\n'
	@printf '  \033[36mcodex-feedback\033[0m    Create a temp Codex project and ask for workflow feedback\n\n'
	@printf '\033[1mInspect\033[0m\n'
	@printf '  \033[36msmoke\033[0m       Invoke inspect_status through the stdio tool helper\n'
	@printf '  \033[36msmoke-queue\033[0m Invoke inspect_work_queue through the stdio tool helper\n'
	@printf '  \033[36msmoke-storage\033[0m Probe storage backend capabilities through MCP\n'
	@printf '  \033[36mmetadata\033[0m    Print Cargo metadata without dependencies\n'
	@printf '  \033[36mversion\033[0m     Print Cargo and rustc versions\n\n'
	@printf '\033[1mVariables\033[0m\n'
	@printf '  ROOT=%s\n' '$(ROOT)'
	@printf '  MAX_TASKS=%s\n' '$(MAX_TASKS)'
	@printf '  NPM_REQUIRED_PLATFORMS=linux-x64,linux-arm64,darwin-x64,darwin-arm64 for strict npm binary validation\n'
	@printf '  TEST=%s\n' '$(TEST)'
	@printf '  ARGS=%s\n' '$(ARGS)'
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

check: lint test ## Format check, build-check, and run all tests.

ci: check ## Alias for check.

lint: fmt-check ## Run formatting check and cargo check.
	$(CARGO) check

fmt-check: ## Check Rust formatting.
	$(CARGO) fmt --check

fmt: ## Apply Rust formatting.
	$(CARGO) fmt

format: fmt ## Alias for fmt.

test: ## Run all tests.
	$(CARGO) test $(ARGS)

test-lib: ## Run library/unit tests.
	$(CARGO) test --lib $(ARGS)

test-stdio: ## Run stdio protocol integration tests.
	$(CARGO) test --test stdio_protocol $(ARGS)

test-one: ## Run a filtered test: make test-one TEST=name.
	@if [ -z "$(TEST)" ]; then \
		printf 'TEST is required. Example: make test-one TEST=parses_platypus_trailers\n' >&2; \
		exit 2; \
	fi
	$(CARGO) test $(TEST) $(ARGS)

build: ## Build debug binary.
	$(CARGO) build

doc: ## Build Rust API docs without dependencies.
	$(CARGO) doc --no-deps

package: ## Verify crates.io package contents.
	$(CARGO) package --list

publish-dry-run: ## Validate crates.io publishing without uploading.
	$(CARGO) publish --dry-run

publish: ## Publish the crate to crates.io.
	$(CARGO) publish

npm-package: ## Validate npm package contents.
	PLATYPUS_NPM_REQUIRED_PLATFORMS="$${PLATYPUS_NPM_REQUIRED_PLATFORMS:-$(NPM_REQUIRED_PLATFORMS)}" npm_config_cache=$(NPM_CACHE_DIR) npm run package:validate

npm-package-dry-run: ## Show npm package dry-run output.
	npm_config_cache=$(NPM_CACHE_DIR) npm run package:dry-run

npm-publish-dry-run: npm-package ## Validate npm publish without uploading.
	npm_config_cache=$(NPM_CACHE_DIR) npm run publish:dry-run

npm-publish: npm-package ## Publish the npm package.
	npm_config_cache=$(NPM_CACHE_DIR) npm run publish:release

release-check: publish-dry-run npm-publish-dry-run ## Validate Rust and npm release packaging.

clean: ## Remove Cargo build output.
	$(CARGO) clean

run: ## Run stdio MCP server in the current directory.
	$(CARGO) run -- $(ARGS)

run-root: ## Run stdio MCP server with ROOT=/path/to/project.
	PLATYPUS_MCP_ROOT="$(ROOT)" $(CARGO) run -- $(ARGS)

feedback: opencode-feedback ## Alias for opencode-feedback.

opencode-feedback: ## Create a temp OpenCode project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/opencode-feedback.sh

claude-feedback: ## Create a temp Claude Code project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/claude-feedback.sh

codex-feedback: ## Create a temp Codex project and ask for workflow feedback.
	CARGO="$(CARGO)" scripts/dev/codex-feedback.sh

smoke: ## Invoke inspect_status through the stdio tool helper.
	$(CARGO) run -- tool --root "$(ROOT)" inspect_status '{"limit":5}'

smoke-queue: ## Invoke inspect_work_queue through the stdio tool helper.
	$(CARGO) run -- tool --root "$(ROOT)" inspect_work_queue '{"limit":5}'

smoke-storage: ## Probe storage backend capabilities through MCP.
	$(CARGO) run -- tool --root "$(ROOT)" storage_capability_probe '{}'

metadata: ## Print Cargo metadata without dependencies.
	$(CARGO) metadata --no-deps

version: ## Print Cargo and rustc versions.
	@$(CARGO) --version
	@rustc --version
