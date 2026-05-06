# Storage Boundary

Platypus MCP keeps SQLite as the local durable store and uses `rusqlite` behind
a repository boundary in `src/storage/`.

## Decision

Use typed repository modules before adopting a full ORM.

This keeps the project close to SQLite semantics while removing scattered
handwritten SQL from MCP feature modules. The repository layer owns row mapping,
query shape, and write/update operations. Feature modules should validate tool
inputs, call repository methods, and translate repository errors into structured
MCP results.

## Alternatives Considered

- SQLx: strong typed query support, but async-first and heavier than the current
  local SQLite stdio server needs.
- Diesel: mature and strongly typed, but schema management and generated query
  code would add complexity before the tool contract stabilizes.
- SeaORM: broad ORM feature set, but too much framework for a small local
  project state database.
- Raw rusqlite everywhere: simple initially, but grows hard to audit as tools
  share records, events, approvals, findings, worktrees, and evidence.

## Current Boundary

- `storage::Repository` creates typed sub-repositories.
- `storage::ApprovalRepository` owns approval reads, creation, and response
  updates.
- `storage::EventRepository` owns durable control event writes and event replay
  reads, including task-event replay projection.

New runtime tables should first expose repository methods, then MCP tools should
call those methods rather than embedding SQL in feature modules.
