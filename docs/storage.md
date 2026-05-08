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

## State Ownership

Repository-owned declarative state stays in Git:

- backlog items under `backlog/items/`
- task plans under `backlog/plans/`
- workflow/config files such as `platy.yaml` and `WORKFLOW.md`
- documentation, evidence artifacts, and implementation commits

Runtime-owned mutable state belongs behind the storage boundary:

- task attempts and task events
- worker assignments and execution metadata
- approvals and responses
- runtime transitions for lifecycle/state changes
- findings and dispositions
- verification evidence records
- future leases, client/session metadata, and distributed coordination records

Backlog files should describe intent and acceptance. They should not accumulate
runtime-only fields such as attempts, status, completion timestamps, approvals,
or worker results.

## Portable Boundary

SQLite is the reference backend, not the product boundary. Feature modules
should use typed storage traits and repository APIs instead of depending on
SQLite table details or `rusqlite` errors directly.

Current portable traits:

- `ApprovalStore`: approval creation, listing, lookup, and response updates.
- `EventStore`: project event recording and replay, including task-event
  projection.
- `TaskStore`: task creation, lookup, lifecycle updates, and task event replay.
- `TransitionStore`: append-only runtime transition recording and replay.

The traits return `RepositoryResult<T>` with a backend-neutral
`RepositoryError`. Tool modules should handle `NotFound`, `Conflict`, and
backend failures without matching SQLite-specific error variants. The SQLite
repository maps its native errors into those portable categories.

## Append-Only Transitions

Current-state tables such as `tasks` and `approvals` are materialized views of
runtime state. Important lifecycle mutations should also write an append-only
transition record with:

- domain, such as `task` or `approval`
- entity id, such as a task id or approval id
- stable transition type, such as `task_created` or `approval_approved`
- safe summary and bounded payload
- creation timestamp and replay cursor

Migrated writes update current state and append the transition in the same
SQLite transaction. Invalid or duplicate mutations should not append successful
transition records. `events_replay` includes runtime transitions alongside
project and task events so a host can explain how an entity reached its current
state without reading hidden storage tables directly.

## Adding Runtime Repositories

When adding a new runtime domain:

1. Define typed insert/query records and a store trait.
2. Implement the trait for the SQLite repository.
3. Keep row mapping and SQL in `src/storage/` or a domain-specific store module.
4. Make MCP feature modules validate inputs, call the store trait, and translate
   `RepositoryError` into structured tool results.
5. Add tests for one write path, one read path, and at least one failure path.

This keeps local SQLite reliable while preserving the option to add a shared
runtime backend later.
