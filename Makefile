SHELL := /bin/sh

CARGO ?= cargo
ROOT ?= $(CURDIR)
MAX_TASKS ?= 1
TEST ?=
ARGS ?=

.DEFAULT_GOAL := help

.PHONY: \
	help \
	check ci lint fmt-check fmt format test test-lib test-stdio test-one \
	build doc clean \
	run run-root runner smoke smoke-queue smoke-storage \
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
	@printf '  \033[36mclean\033[0m       Remove Cargo build output\n\n'
	@printf '\033[1mRun\033[0m\n'
	@printf '  \033[36mrun\033[0m         Run stdio MCP server in the current directory\n'
	@printf '  \033[36mrun-root\033[0m    Run stdio MCP server with ROOT=/path/to/project\n'
	@printf '  \033[36mrunner\033[0m      Run local preparation runner with MAX_TASKS=1 by default\n\n'
	@printf '\033[1mInspect\033[0m\n'
	@printf '  \033[36msmoke\033[0m       Invoke inspect_status through the stdio tool helper\n'
	@printf '  \033[36msmoke-queue\033[0m Invoke inspect_work_queue through the stdio tool helper\n'
	@printf '  \033[36msmoke-storage\033[0m Probe storage backend capabilities through MCP\n'
	@printf '  \033[36mmetadata\033[0m    Print Cargo metadata without dependencies\n'
	@printf '  \033[36mversion\033[0m     Print Cargo and rustc versions\n\n'
	@printf '\033[1mVariables\033[0m\n'
	@printf '  ROOT=%s\n' '$(ROOT)'
	@printf '  MAX_TASKS=%s\n' '$(MAX_TASKS)'
	@printf '  TEST=%s\n' '$(TEST)'
	@printf '  ARGS=%s\n' '$(ARGS)'

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

clean: ## Remove Cargo build output.
	$(CARGO) clean

run: ## Run stdio MCP server in the current directory.
	$(CARGO) run -- $(ARGS)

run-root: ## Run stdio MCP server with ROOT=/path/to/project.
	PLATYPUS_MCP_ROOT="$(ROOT)" $(CARGO) run -- $(ARGS)

runner: ## Run local preparation runner with MAX_TASKS=1 by default.
	$(CARGO) run -- runner --max-tasks "$(MAX_TASKS)" $(ARGS)

smoke: ## Invoke inspect_status through the stdio tool helper.
	$(CARGO) run -- tool --root "$(ROOT)" inspect_status '{"limit":5}'

smoke-queue: ## Invoke inspect_work_queue through the stdio tool helper.
	$(CARGO) run -- tool --root "$(ROOT)" inspect_work_queue '{"limit":5,"require_task_plan":true}'

smoke-storage: ## Probe storage backend capabilities through MCP.
	$(CARGO) run -- tool --root "$(ROOT)" storage_capability_probe '{}'

metadata: ## Print Cargo metadata without dependencies.
	$(CARGO) metadata --no-deps

version: ## Print Cargo and rustc versions.
	@$(CARGO) --version
	@rustc --version
