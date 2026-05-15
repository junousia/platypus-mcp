---
name: verification
description: Use before declaring Rust MCP implementation work complete; run and report formatting, build, and tests or explain the blocker.
---

# Verification

Use this before declaring implementation work complete.

## Required Commands

```bash
bazel test //...
bazel build --config=release //:platypus_mcp_binary_tar //:release_metadata_tar
```

If Bazel is unavailable locally, report that blocker and run the closest Cargo
fallback:

```bash
cargo fmt --check
cargo check
cargo test
```

## Report Format

Include commands run, pass/fail result, important failures, files changed, and
remaining risks or gaps.
