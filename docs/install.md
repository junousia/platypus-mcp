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
command = "cargo"
args = ["run", "--manifest-path", "/path/to/platypus-mcp-rs/Cargo.toml", "--quiet"]
env = { PLATYPUS_MCP_ROOT = "/path/to/project" }
```

For development inside this repository, `.codex/config.toml` contains a
project-local configuration that runs the checked-out source directly.

## Claude

Claude uses the same stdio command shape:

```json
{
  "mcpServers": {
    "platypus": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/path/to/platypus-mcp-rs/Cargo.toml",
        "--quiet"
      ],
      "env": {
        "PLATYPUS_MCP_ROOT": "/path/to/project"
      }
    }
  }
}
```

## First Project Smoke Flow

After configuring the client, ask the MCP host to:

1. Call `init_project`.
2. Call `doctor_snapshot`.
3. Call `inspect_status`.
4. Call `storage_capability_probe`.
5. Draft or import backlog work, then call `validate_backlog`.

These tools should return structured JSON envelopes with `status`, `summary`,
and `next_action` when recovery guidance is needed.

## Security Defaults

- Credentials stay in the MCP host or approved provider plugin, not in
  Platypus backlog, evidence, events, or Git history.
- External report sending is split into a local draft, explicit approval, and a
  separate dispatch-result record.
- Runtime state lives under the configured project root in `.platy/`.
