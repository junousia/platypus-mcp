---
name: commit-policy
description: Use when preparing, reviewing, or creating commits; enforce scoped commits, verification, secret checks, and clean commit messages.
---

# Commit Policy

Use this when preparing, reviewing, or creating commits.

## Rules

- Commit only when the user asks for a commit or a meaningful checkpoint is
  appropriate during longer work.
- Inspect `git status --short --untracked-files=all` before staging.
- Review the relevant diff before staging.
- Stage only files that belong to the completed task.
- Preserve unrelated user changes.
- Never commit `.env`, secrets, local caches, generated build output, or
  `target/`.
- Commit `Cargo.lock`.
- Run verification before committing unless the user asks to skip it or a
  blocker prevents it.
- Use concise imperative commit messages, for example
  `Add MCP project status tool`.

## Commit Flow

1. Review `git status --short --untracked-files=all`.
2. Review the relevant diff.
3. Run verification:
   - `cargo fmt --check` when available
   - `cargo check`
   - `cargo test`
4. Stage only task-related files.
5. Commit with a concise imperative subject.
6. Report commit SHA, verification status, and remaining dirty files.
