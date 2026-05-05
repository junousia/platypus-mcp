---
name: mcp-tool-safety
description: Use when adding or modifying MCP tools, file access, project-root handling, command execution, approvals, event replay, redaction, or mutating behavior.
---

# MCP Tool Safety

Use this when adding or modifying MCP tools.

## Rules

- Tools must stay inside the configured project root.
- Resolve paths before checking containment.
- Block path traversal and symlink escapes.
- Keep outputs bounded.
- Avoid arbitrary shell execution.
- If shell execution is needed, use explicit allowlists, timeouts, and audit
  metadata.
- Return tool errors as structured results; do not panic on user input.
- Redact secrets from file, config, event, and command output.
- Mutating tools must be obvious from their name and response.
- Approval, audit, and policy behavior must fail closed when required runtime
  context is missing.
- Prefer typed `serde` request/response structs with `JsonSchema` derives.

## Required Tests

Cover successful safe paths, attempted path escapes, missing files, invalid
input, output size limits, denied mutations, and recovery guidance for failed
tools.

Run:

```bash
cargo check
cargo test
```
