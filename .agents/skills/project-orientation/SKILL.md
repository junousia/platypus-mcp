---
name: project-orientation
description: Use when starting work in this Rust MCP repository or when a task needs orientation across MCP tools, config, Cargo, tests, or repository workflow.
---

# Project Orientation

Use this when starting unfamiliar work in this repository.

## Steps

1. Read `AGENTS.md`.
2. Inspect `MODULE.bazel`, `BUILD.bazel`, `Cargo.toml`, and `Cargo.lock`.
3. Inspect `README.md`.
4. Inspect `src/main.rs` and any nearby modules for the target area.
5. Check `git status --short --untracked-files=all`.
6. For MCP behavior, identify the request/response structs and RMCP tool
   annotations before editing.

## Output

When reporting orientation findings, include relevant files, current behavior,
likely change points, and the verification command. Keep this concise.
