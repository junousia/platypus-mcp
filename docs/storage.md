# Storage Boundary

Platypus MCP uses a domain-shaped project state boundary. MCP tools and product
logic should depend on Platypus operations such as dispatching work, preparing a
worker assignment, completing execution, replaying events, and reconciling a
project. They should not depend on SQL, database connections, table-shaped
repositories, or row-level query concepts.

SQLite is the local-first reference backend. It is not the product boundary.

## Decision

Use a `ProjectState` boundary built around Platypus domain commands and
snapshots. Backend implementations own persistence, indexing, locking,
transactions, migrations, and event ordering internally.

The public storage interface should be implementation independent. It should
not look like a SQL repository interface hidden behind traits. Product code
should call operations such as:

- `dispatch_next_work`
- `prepare_worker_assignment`
- `start_worker_execution`
- `record_worker_event`
- `complete_worker_execution`
- `record_verification_evidence`
- `record_finding`
- `reconcile_project`

Those methods are domain operations with backend-neutral command and result
types. A backend may implement them with SQLite transactions, an in-memory
state machine, a remote service, an event log, or a future shared database, but
callers must not need to know which representation is used.

## Alternatives Considered

- SQL-shaped repository traits: useful as an intermediate migration step, but
  they leak table and query structure into product logic and make non-SQL
  backends mimic SQLite.
- SQLx: strong typed query support, but async-first and heavier than the current
  local SQLite stdio server needs. It still keeps the public model close to SQL
  unless hidden behind domain operations.
- Diesel: mature and strongly typed, but schema management and generated query
  code would add complexity without solving the domain-boundary problem.
- SeaORM: broad ORM feature set, but too much framework for the current local
  state database and still SQL-model oriented.
- Raw rusqlite everywhere: simple initially, but grows hard to audit as tools
  share records, events, approvals, findings, worktrees, and evidence.

## Target Boundary

The intended architecture is:

```text
MCP tool
  -> domain service
    -> ProjectState trait
      -> backend implementation
```

`ProjectState` is responsible for preserving product invariants:

- one active task per backlog item
- claim and worker assignment happen atomically
- worker start, progress, and completion are ordered and recoverable
- event replay order is stable
- approvals and leases fail closed
- evidence and findings attach to the right backlog item or task
- reconciliation can explain unfinished, unverified, or unintegrated work

The MCP layer should validate request shape, call one domain operation, and
translate the result into the standard `ActionResult` envelope. It should not
assemble storage mutations manually.

## Atomic Domain Operations

These operations must be atomic by contract. A backend may use SQL
transactions, compare-and-swap updates, append-only event commits, or service
transactions internally, but callers see one all-or-nothing operation.

- **Dispatch work:** choose the next runnable backlog item and create a queued
  task without creating duplicates.
- **Prepare assignment:** claim a queued or claimed task, create or record the
  worktree, generate the bundle, persist the assignment, and record the audit
  events.
- **Start execution:** move an assignment and task into the running state and
  attach the worker session.
- **Record worker event:** append a bounded, ordered progress/tool/result event
  for a running assignment.
- **Complete execution:** persist worker result metadata, finish the task, and
  append result events consistently.
- **Resolve approval:** move a pending approval to an approved or denied state
  once, with responder metadata.
- **Acquire lease:** grant ownership only when no conflicting active lease
  exists.
- **Integrate result:** update integration evidence and task/project audit state
  consistently with the Git operation.

Read operations such as queue, task, or assignment inspection, event replay,
findings validation, and reconciliation should return domain snapshots rather
than backend rows.

The initial Rust boundary lives in `src/state/`. It defines `ProjectState` as a
compile-time contract only; existing tools still use the current SQLite-backed
runtime path until later migration items move behavior behind the trait.

Current `ProjectState` atomic methods:

- `dispatch_work`
- `prepare_assignment`
- `start_execution`
- `append_worker_event`
- `complete_execution`
- `resolve_approval`
- `acquire_lease`
- `integrate_result`

Current `ProjectState` read methods:

- `describe_backend`
- `inspect_task`
- `inspect_assignment`
- `replay_events`
- `list_findings`
- `validate_findings`
- `reconcile_project`

## Current Migration State

The current codebase still contains SQL-shaped repository traits and direct
SQLite use in some product modules. They are migration scaffolding, not the
final boundary. New product logic should not add new direct SQL or new
table-shaped public stores.

The migration path is:

1. Define `ProjectState` domain commands, snapshots, and errors.
2. Implement `SqliteProjectState` while preserving current behavior. The
   current shell lives under `src/state/sqlite/`, opens the project through the
   existing storage initializer, exposes backend capabilities, supports task
   inspection and safe-action reads, and returns explicit unsupported errors
   for behavior that is not migrated yet.
3. Move task and assignment lifecycle tools behind `ProjectState`.
4. Move guidance, inspection, evidence, findings, reconciliation, and workspace
   metadata behind `ProjectState`.
5. Add a memory backend and contract tests shared by every backend.
6. Add a guard that blocks `rusqlite` and SQL query construction outside the
   SQLite backend implementation.

`MemoryProjectState` is test/support infrastructure. It is intentionally
non-durable, does not run migrations, and should not become the production
default. Its job is to run backend-neutral `ProjectState` contract tests so
SQLite behavior does not accidentally become the product contract.

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

## Backend Independence Rules

The public boundary must avoid SQL-like terms and behavior. Public traits,
command names, and result names should not expose:

- connections, transactions, rows, tables, or query builders
- `insert`, `update`, `select`, or `where` as product concepts
- SQLite error variants or provider-specific database errors
- schema version details except through backend capability and migration
  diagnostics
- storage paths except as safe project metadata or backend configuration

The allowed implementation-specific area is the backend module, currently the
SQLite implementation. During migration the legacy `src/storage/` module may
still contain SQLite code. The desired end state is that `rusqlite`,
`query_row`, `prepare`, and raw SQL strings appear only in SQLite backend
modules and their focused tests.

`tests/sql_isolation.rs` enforces the current allowlist during
`bazel test //...`. Permanent SQLite-specific locations are:

- `src/state/sqlite/mod.rs`
- `src/storage/mod.rs`
- `src/storage/probe.rs`
- `src/storage/repository.rs`
- `src/storage/schema.rs`
- `src/storage/traits.rs`

The temporary migration allowlist is intentionally small:

- `src/assignments/store.rs`
- `src/tasks.rs`

These temporary entries exist only because assignment and task persistence
helpers still bridge older tool paths during the `ProjectState` migration. Do
not add new product or MCP tool modules to the allowlist; move new persistence
behavior into a backend module instead.

## Backend Capabilities

Backends should report capabilities explicitly. This avoids pretending every
backend has identical deployment or coordination properties.

Useful capability categories:

- durable state
- transactional lifecycle operations
- stable event replay cursors
- lease/conflict enforcement
- migration support
- shared multi-host coordination
- external sync/reporting support
- offline/local-only operation

Core MCP tools should require the capabilities they need and return structured
recovery guidance when a configured backend cannot support a requested action.

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

## Adding Runtime Behavior

When adding a new runtime domain:

1. Define the domain operation and snapshot shape in the `ProjectState`
   boundary.
2. Decide whether the operation is atomic by contract.
3. Implement it in each supported backend or mark the missing capability
   explicitly.
4. Keep backend-specific persistence details private to the backend module.
5. Add contract tests that run against every backend implementation.
6. Add MCP tool tests for structured success, skipped, failed, and recovery
   paths.

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

## Plugin-Friendly Backend Shape

The first implementation can be compile-time registered backends. Dynamic
plugins are not required yet. The boundary should still be shaped so a future
backend provider can implement Platypus operations without copying the SQLite
schema.

```text
BackendFactory
  name
  validate_config
  describe_capabilities
  open_project -> ProjectState
```

This keeps a future Postgres, Git-backed, remote, or hosted backend focused on
Platypus semantics instead of table compatibility.

## Backend Evaluation Checklist

Before adding a second backend, run a capability probe through the
implementation-independent state boundary:

1. Initialize a project-scoped runtime namespace.
2. Create, replay, and order events from two simulated clients.
3. Acquire a lease from one client and verify a second client receives a
   structured conflict.
4. Retry one mutating operation and verify deterministic idempotency or conflict
   output.
5. Run a migration check twice and verify repeatability.
6. Reconcile a project snapshot using only `ProjectState` snapshots.

The current probe is exposed as `storage_capability_probe`. It resolves the
project root for identity reporting, then runs the backend checks against an
isolated in-memory SQLite database so the diagnostic does not mutate project
runtime state.

Passing this checklist is the first implementation step toward shared runtime
storage. It keeps local-first SQLite support intact while preventing any future
backend from weakening the MCP contract.
