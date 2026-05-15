# Bazel Build Graph

Platypus MCP uses Bazel as the primary build and release graph.

Common commands:

```bash
bazel test //...
bazel build //...
bazel build --config=release //:platypus_mcp_binary_tar //:release_metadata_tar
```

Rust dependencies are ingested from `Cargo.toml` and `Cargo.lock` through
`rules_rust` crate-universe. Bazel 9 resolves Bzlmod dependencies during normal
build, test, and fetch commands. After dependency changes, prefetch external
repositories with:

```bash
bazel fetch //...
```

Release stamping is driven by `tools/bazel/workspace_status.sh`. Tags like
`v0.2.1` produce stamped release manifests with version `0.2.1`; non-tagged
developer builds stamp `0.0.0-dev`.
