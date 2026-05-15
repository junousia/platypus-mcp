---
name: rust-workflow
description: Use when adding, changing, documenting, or running Rust development commands, dependencies, Cargo configuration, formatting, or tests.
---

# Rust Workflow

Use this when changing Rust code, Cargo/Bazel metadata, or developer commands.

## Rules

- Prefer Bazel commands for repository-level build and test work.
- Keep dependencies purposeful and avoid framework churn.
- Use `cargo add` only when available and appropriate; otherwise edit
  `Cargo.toml` deliberately.
- Commit `Cargo.lock` for this application/server repository.
- Keep generated build output under `target/` and `bazel-*` out of Git.
- Use `bazel test //:rustfmt_test` for formatting checks.
- Use `bazel test //...` for behavior.
- Cargo commands are fallback/debugging tools and registry-boundary checks.

## Verification

Run:

```bash
bazel test //...
bazel build --config=release //:platypus_mcp_binary_tar //:release_metadata_tar
```

If Bazel is unavailable locally, report that blocker and run
`cargo fmt --check`, `cargo check`, and `cargo test` as fallback evidence.
