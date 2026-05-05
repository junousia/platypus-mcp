---
name: verification
description: Use before declaring Rust MCP implementation work complete; run and report formatting, build, and tests or explain the blocker.
---

# Verification

Use this before declaring implementation work complete.

## Required Commands

```bash
cargo fmt --check
cargo check
cargo test
```

If `cargo fmt` is unavailable, report the missing `rustfmt` component and still
run:

```bash
cargo check
cargo test
```

## Report Format

Include commands run, pass/fail result, important failures, files changed, and
remaining risks or gaps.
