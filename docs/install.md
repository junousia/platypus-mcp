# Install And Client Configuration

Platypus MCP is currently distributed as a Rust stdio MCP server. The stable
product boundary is the MCP tool contract, not a custom UI or daemon.

## Prerequisites

- Rust toolchain with `cargo`
- Git for worktree-based lifecycle tools
- An MCP host such as Codex, Claude, or opencode

Build and verify the server from the repository root:

```bash
make check
```

Install the released crate:

```bash
cargo install platypus-mcp
```

The installed binary is:

```bash
platypus-mcp
```

## Pi npm Package Binary Resolution

The Pi npm package is expected to make the `platypus-mcp` server available for
normal installed users without requiring Cargo during `npm install`. The Pi
extension resolves the binary in this order:

1. `PLATYPUS_MCP_BIN`, for local testing or custom installations.
2. A package-local prebuilt binary under `bin/platypus-mcp`,
   `bin/<platform>/platypus-mcp`, or `vendor/<platform>/platypus-mcp`, or a
   matching optional platform binary package. Current release platform keys are
   `linux-x64`, `linux-arm64`, `darwin-x64`, and `darwin-arm64`.
3. The repository `Cargo.toml` fallback when running from a development
   checkout.
4. `platypus-mcp` on `PATH`, such as a `cargo install platypus-mcp` install.

If none is available, Pi reports actionable guidance instead of silently
requiring Cargo. Release builds should add the relevant platform binary before
packing the npm artifact or publish matching platform-specific binary packages.

Validate npm package contents from the repository root:

```bash
make npm-package
make release-check
```

The npm validation uses `npm pack --dry-run` and checks that the package keeps
extension files and binary resolver metadata while excluding build caches,
local state, backlog files, and secrets.
Release validation can set `PLATYPUS_NPM_REQUIRE_BINARY=1` after staging a
platform binary under `bin/` or `vendor/`; the check then requires an
executable package-local binary candidate.
Set `NPM_REQUIRED_PLATFORMS=darwin-arm64` or
`PLATYPUS_NPM_REQUIRED_PLATFORMS=darwin-arm64` to require a specific packaged
platform binary. The publish workflow validates Linux x64, Linux arm64, macOS
Intel, and macOS arm64 runners by staging:

```text
vendor/linux-x64/platypus-mcp
vendor/linux-arm64/platypus-mcp
vendor/darwin-x64/platypus-mcp
vendor/darwin-arm64/platypus-mcp
```

The Apple silicon build uses Rust target `aarch64-apple-darwin`; the other
release binaries are built on native hosted runners. The macOS binaries are not
currently codesigned or notarized; add that as a separate release hardening step
if macOS distribution warnings become a blocker.

Configure an MCP host:

```bash
platypus-mcp bootstrap codex
platypus-mcp bootstrap claude
platypus-mcp bootstrap opencode
platypus-mcp bootstrap pi
```

For a fresh project, initialize the repository guidance at the same time:

```bash
platypus-mcp bootstrap pi --root /path/to/project --init-project
```

That writes host configuration and creates project-local guidance such as
`AGENTS.md`, `CLAUDE.md`, `WORKFLOW.md`, `platy.yaml`, and `backlog/` in the
target root. Existing files are preserved by default. For Pi specifically,
bootstrap writes `.pi/settings.json` with the `npm:platypus-pi` package entry
instead of writing a generic `.mcp.json` file; the Pi package supplies the
`platypus_*` tools and forwards them to the Rust MCP tool CLI.

Useful bootstrap options:

```bash
platypus-mcp bootstrap codex --dry-run
platypus-mcp bootstrap codex --check
platypus-mcp bootstrap codex --global
platypus-mcp bootstrap codex --root /path/to/project
platypus-mcp bootstrap codex --root /path/to/project --init-project
platypus-mcp bootstrap codex --root /path/to/project --init-project --project-name "My App"
platypus-mcp bootstrap pi --root /path/to/project --check --init-project
```

By default `bootstrap <host>` only wires the MCP server into the selected host.
Add `--init-project` when you also want the repository guidance files created.
Use host-only bootstrap for already initialized repositories or when you only
want to update MCP client configuration. Hosts without a verified local
instruction-file convention are steered through MCP server instructions,
resources, prompts, and tool descriptions.

Smoke-test the stdio tool contract against the current directory:

```bash
make smoke
make smoke-queue
make smoke-storage
```

Use `ROOT=/path/to/project` to point those smoke checks at another project:

```bash
make smoke ROOT=/path/to/project
```

## Harness Startup

After configuring Codex, Claude, opencode, or another MCP host, start each new
project session with the same deterministic opening sequence:

1. List MCP resources and prompts.
2. Read `platypus://guidance/workflow`,
   `platypus://guidance/project-status`, and
   `platypus://guidance/tool-preload`, plus
   `platypus://tools/core-schemas` when schema preloading is supported, or the equivalent prompts
   `platypus-workflow`, `platypus-project-status`, and
   `platypus-tool-preload`.
3. If the client supports deferred schema preloading, call `inspect_toolsets`
   or read `platypus://tools/core-schemas` / `platypus-tool-preload` for
   advisory discovery metadata. These entries are search hints and selectors
   unless the host explicitly supports automatic schema registration. Toolsets
   are not required workflow steps or separate MCP servers.
4. Call `inspect_session`. If the client cannot use that broad snapshot, call
   `doctor_snapshot`, `inspect_status`, `inspect_workflow_config`,
   `inspect_queue_status`, then `inspect_work_queue` as needed.

Tool schemas are delivered by the MCP client and may be deferred until a tool
is discovered or selected. Schema preloading is a client convenience, not a
Platypus requirement or guarantee; when it is awkward, call the same tools on
demand.
`inspect_session` and `inspect_work_queue` also return
`schemas_likely_needed_next` with 1-4 likely next tool schemas. Claude entries
include literal ToolSearch selectors and a response-level batch selector.
Codex-style text-search hosts should use `codex_tool_search_query` or
response-level `host_neutral_tool_search_query`; opencode and other callers can
use `tool_name` or `host_neutral_query` values with their own discovery UI.
Claude Code can load deferred schemas with ToolSearch selectors such as
`select:mcp__platypus__inspect_session`; the prefix comes from the configured
MCP server name.

The direct-work quick path is `inspect_session` -> queue inspection -> host
file edits -> `complete_backlog_item`. Call `inspect_toolsets` only when the
host needs compact discovery metadata for a workflow phase. When queue output
says `prepare_work_optional=true`, `prepare_work` is optional and only returns
response-local guidance. Use worker handoff only when the backlog item or
workflow policy says `execution_path=worker_handoff`.

## Codex

Use the server name `platypus` so tool calls are easy to recognize.

```toml
[mcp_servers.platypus]
command = "platypus-mcp"
args = []
env = { PLATYPUS_MCP_ROOT = "/path/to/project" }
```

For development inside this repository, `.codex/config.toml` contains a
project-local configuration that uses the installed `platypus-mcp` binary.

Source checkout configuration:

```toml
[mcp_servers.platypus]
command = "cargo"
args = ["run", "--manifest-path", "/path/to/platypus-mcp/Cargo.toml", "--quiet"]
env = { PLATYPUS_MCP_ROOT = "/path/to/project" }
```

## Claude

Claude uses the same stdio command shape:

```json
{
  "mcpServers": {
    "platypus": {
      "command": "platypus-mcp",
      "args": [],
      "env": {
        "PLATYPUS_MCP_ROOT": "/path/to/project"
      }
    }
  }
}
```

## Opencode

Opencode uses the bootstrap-generated `opencode.json` shape. Prefer bootstrap
so the `$schema` and MCP server block are written consistently:

```bash
platypus-mcp bootstrap opencode --root /path/to/project
```

For a fresh project:

```bash
platypus-mcp bootstrap opencode --root /path/to/project --init-project
```

## Pi

Pi loads packages from project settings, so Platypus uses `.pi/settings.json`
rather than a shared MCP config file:

```bash
platypus-mcp bootstrap pi --root /path/to/project --init-project
```

The generated settings add `npm:platypus-pi` to the `packages` array while
preserving existing Pi settings. Use `--check --init-project` to verify that the
project has Pi settings and Platypus guidance files:

```bash
platypus-mcp bootstrap pi --root /path/to/project --check --init-project
```

If the check reports that the binary cannot be resolved, install the server with
`cargo install platypus-mcp`, install a Pi npm package that contains a bundled
platform binary, or set `PLATYPUS_MCP_BIN` to a working `platypus-mcp` path.

## First Project Smoke Flow

After configuring the client, ask the MCP host to follow the Harness Startup
sequence above. If `inspect_session` cannot provide the detail needed for a
smoke check, ask the host to:

1. Call `doctor_snapshot`.
2. Call `inspect_status`.
3. Call `storage_capability_probe`.
4. Draft or import backlog work. Successful typed creation validates inline;
   call `inspect_work_queue` next, and reserve `validate_backlog` for manual
   markdown edits or explicit audit checks.

If you did not use `--init-project`, first ask the host to call
`init_project`.

These tools should return structured JSON envelopes with `status`, `summary`,
`next_action` for normal continuation, and `recovery_action` when recovery
guidance is needed.

## Security Defaults

- Credentials stay in the MCP host or approved provider plugin, not in
  Platypus backlog, evidence, events, or Git history.
- External report sending is split into a local draft, explicit approval, and a
  separate dispatch-result record.
- Runtime state lives under the configured project root in `.platy/`.

## Release

The `Publish` GitHub Actions workflow publishes the Rust crate to crates.io and
the Pi integration package to npm when a GitHub release is published. It can
also be run manually as a dry run.

Repository setup:

1. Create a crates.io API token.
2. Add it as the GitHub repository secret `CARGO_REGISTRY_TOKEN`.
3. Create an npm automation token with publish rights for `platypus-pi`.
4. Add it as the GitHub repository secret `NPM_TOKEN`.
5. Publish a GitHub release for the version in `Cargo.toml` and `package.json`.

The workflow always runs `make check`, `make publish-dry-run`, per-platform npm
binary validation, combined npm package validation, and `make
npm-publish-dry-run` before upload steps. npm publishing waits for the crate
publish job, so a crates.io failure prevents the npm package from being
published.

Manual `workflow_dispatch` runs default to `dry_run=true`, which validates both
release paths without uploading. Set `dry_run=false` only when both
`CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` are configured and an actual publish is
intended.

Release uploads are idempotent by package version. If a release partially
publishes, for example crates.io succeeds but npm fails because the token needs
to be replaced, update the secret and rerun the workflow. The workflow validates
the package again, treats an already-published crate as complete, skips
already-published npm versions, and continues with the remaining upload.
