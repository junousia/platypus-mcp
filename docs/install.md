# Install And Client Configuration

Platypus MCP is currently distributed as a Rust stdio MCP server. The stable
product boundary is the MCP tool contract, not a custom UI or daemon.

## Prerequisites

- Rust toolchain with `cargo`
- Git for worktree-based lifecycle tools
- An MCP host such as Codex or Claude

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

Configure an MCP host:

```bash
platypus-mcp bootstrap codex
platypus-mcp bootstrap claude
platypus-mcp bootstrap opencode
platypus-mcp bootstrap pi
```

For a fresh project, initialize the repository guidance at the same time:

```bash
platypus-mcp bootstrap codex --root /path/to/project --init-project
```

That writes host MCP configuration and creates project-local guidance such as
`AGENTS.md`, `CLAUDE.md`, `WORKFLOW.md`, `platy.yaml`, and `backlog/` in the
target root. Existing files are preserved by default.

Useful bootstrap options:

```bash
platypus-mcp bootstrap codex --dry-run
platypus-mcp bootstrap codex --check
platypus-mcp bootstrap codex --global
platypus-mcp bootstrap codex --root /path/to/project
platypus-mcp bootstrap codex --root /path/to/project --init-project
platypus-mcp bootstrap codex --root /path/to/project --init-project --project-name "My App"
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

## First Project Smoke Flow

After configuring the client, ask the MCP host to:

1. Call `doctor_snapshot`.
2. Call `inspect_status`.
3. Call `storage_capability_probe`.
4. Draft or import backlog work, then call `validate_backlog`.

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

## Crates.io Release

The `Publish` GitHub Actions workflow publishes `platypus-mcp` to crates.io
when a GitHub release is published. It can also be run manually as a dry run.

Repository setup:

1. Create a crates.io API token.
2. Add it as the GitHub repository secret `CARGO_REGISTRY_TOKEN`.
3. Publish a GitHub release for the version in `Cargo.toml`.

The workflow always runs `make check` and `make publish-dry-run` before the
upload step.
