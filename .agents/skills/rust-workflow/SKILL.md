---
name: rust-workflow
description: Use when adding, changing, documenting, or running Rust development commands, dependencies, Cargo configuration, formatting, or tests.
---

# Rust Workflow

Use this when changing Rust code, Cargo metadata, or developer commands.

## Rules

- Prefer `cargo` commands unless a repository Makefile is added later.
- Keep dependencies purposeful and avoid framework churn.
- Use `cargo add` only when available and appropriate; otherwise edit
  `Cargo.toml` deliberately.
- Commit `Cargo.lock` for this application/server repository.
- Keep generated build output under `target/` and out of Git.
- Use `cargo fmt` for formatting when `rustfmt` is installed.
- Use `cargo check` for fast validation and `cargo test` for behavior.

## Verification

Run:

```bash
cargo fmt --check
cargo check
cargo test
```

If `rustfmt` is unavailable, report that blocker and still run `cargo check`
and `cargo test`.
