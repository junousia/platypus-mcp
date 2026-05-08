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
- leases for project/task ownership and coordination
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
- `LeaseStore`: project and task lease acquisition, renewal, release, and
  active-conflict inspection.

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

## Leases

Leases model temporary ownership for project-level and task-level operations.
They include scope, target id, owner, status, timestamps, expiry, and metadata.
Local SQLite enforces the first coordination path, and future shared backends
must preserve the same semantics:

- active unexpired leases block conflicting owners
- expired or released leases do not block work
- renew and release require the current owner
- lifecycle tools return structured recovery guidance when a lease blocks an
  operation

The first guarded lifecycle path is dispatch: an active project lease for
`project/root` blocks `dispatch_next_work`.

## Distributed Backend Contract

Any future non-SQLite runtime backend must preserve the same public MCP
semantics as the local reference implementation. A backend is acceptable only
when tool callers can keep using the same request/response schemas, recovery
guidance, event replay, and reconciliation rules.

Required guarantees:

- **Transactions:** each lifecycle mutation that updates current state and
  appends audit state must commit atomically or fail without partial success.
- **Event ordering:** project events, task events, and runtime transitions must
  have a stable per-project replay order. Cursors must be monotonic for a given
  project.
- **Idempotency:** retried mutating requests need stable conflict behavior.
  Repeating an already-applied operation should either return the existing
  result or a structured conflict that tells the host what to inspect next.
- **Lease semantics:** active unexpired leases must block conflicting owners
  across all connected hosts and workers. Expiry, renewal, and release must be
  based on backend-observed time, not one client's wall clock alone.
- **Project identity:** every runtime record must be scoped to an explicit
  project identity derived from the project root/config. Two repositories must
  not share runtime records unless explicitly configured to do so.
- **Migrations:** schema changes must be versioned, forward-only by default,
  and safe to run repeatedly. A failed migration must leave the previous
  version usable or clearly mark the backend unavailable.
- **Conflict handling:** duplicate tasks, duplicate approvals, stale leases,
  and incompatible runtime versions must return backend-neutral
  `RepositoryError` categories instead of leaking provider-specific errors.
- **Recovery:** the backend must support enough inspection to explain pending
  approvals, active leases, queued/running tasks, recent events, and unfinished
  worker handoffs after a host crash.

SQLite currently satisfies these requirements for one local project by using
local transactions and a single `.platy/platypus.sqlite3` file. Shared backends
such as Postgres would need to satisfy the same contract with stronger
cross-process coordination and deployment/version checks.

## Backend Evaluation Checklist

Before adding a second backend, implement a small capability probe behind the
existing repository traits:

1. Initialize a project-scoped runtime namespace.
2. Create, replay, and order events from two simulated clients.
3. Acquire a lease from one client and verify a second client receives a
   structured conflict.
4. Retry one mutating operation and verify deterministic idempotency or conflict
   output.
5. Run a migration check twice and verify repeatability.
6. Reconcile a project snapshot using only trait-level data.

The current probe is exposed as `storage_capability_probe`. It resolves the
project root for identity reporting, then runs the backend checks against an
isolated in-memory SQLite database so the diagnostic does not mutate project
runtime state.

Passing this checklist is the first implementation step toward shared runtime
storage. It keeps local-first SQLite support intact while preventing any future
backend from weakening the MCP contract.
