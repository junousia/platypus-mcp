# Platypus MCP Diagrams

This document collects the main architectural and workflow diagrams from
different perspectives. Keep these diagrams aligned with the MCP tool contract
when adding new lifecycle states, durable records, or host-facing tools.

Unless a node or edge is labelled as future or conceptual, diagrams describe
the current intended MCP tool contract.

## System Boundary

Platypus is the deterministic local state boundary. MCP hosts own conversation
and model turns; Platypus owns project state transitions.

```mermaid
flowchart LR
    user["User"]
    host["MCP host<br/>Codex, Claude, or another client"]
    platy["Platypus MCP server"]
    repo["Project repository"]
    state[".platy/platypus.sqlite3"]
    git["Git history"]
    worker["Worker harness"]

    user --> host
    host -->|MCP requests| platy
    platy -->|read/write files| repo
    platy -->|runtime records| state
    platy -->|branches and trailers| git
    host -->|assignment bundle| worker
    worker -->|progress, findings, result| platy
```

## Task State Machine

Task status is runtime state. It belongs in the local store, not in backlog
markdown. The cancellation and retry transitions are planned lifecycle states,
not current tool behavior.

```mermaid
stateDiagram-v2
    [*] --> queued: dispatch_ready_work or dispatch_next_work
    queued --> claimed: claim_next_task
    claimed --> running: start_worker_task
    running --> completed: complete_worker_task completed
    running --> failed: complete_worker_task failed
    running --> cancelled: future cancellation
    completed --> integrated: integrate_worker_result
    failed --> queued: future retry
    cancelled --> queued: future resume/retry
    integrated --> reconciled: reconcile_project clean
    reconciled --> [*]

    running --> running: record_worker_progress
    running --> running: send_worker_guidance
```

## Host Workflow Sequence

The host should use `next_safe_action` to avoid guessing which lifecycle tool
is safe at each point.

```mermaid
sequenceDiagram
    participant Host as MCP host
    participant Platy as Platypus MCP
    participant Worker as Worker harness
    participant Git as Git worktree
    participant State as Runtime store

    Host->>Platy: next_safe_action
    Platy-->>Host: dispatch_ready_work
    Host->>Platy: dispatch_ready_work
    Platy->>State: create queued task and assignment
    Platy->>Git: create/record worktree
    Platy-->>Host: assignment bundle
    Host->>Worker: run bundle in worktree
    Worker->>Platy: record_worker_progress
    Worker->>Platy: complete_worker_task
    Host->>Platy: record_verification_evidence
    Host->>Platy: integrate_worker_result
    Platy->>Git: merge according to workflow policy
    Platy->>State: record event and evidence
    Host->>Platy: reconcile_project
```

## Integration Gates

Managed integration is intentionally strict. It should fail closed with a
structured recovery path when required evidence or workspace safety is missing.

```mermaid
flowchart TD
    start["integrate_worker_result"] --> task{"task completed?"}
    task -->|no| stopTask["skip: complete worker task first"]
    task -->|yes| tree{"recorded worktree?"}
    tree -->|no| stopTree["skip: inspect or recreate worktree"]
    tree -->|yes| verification{"verification evidence present?"}
    verification -->|no| stopVerify["skip: record verification evidence"]
    verification -->|yes| manager{"manager workspace clean?"}
    manager -->|no| stopManager["skip: clean or commit manager changes"]
    manager -->|yes| branch{"worker branch has changes?"}
    branch -->|no| stopBranch["skip: inspect worker output"]
    branch -->|yes| merge{"configured merge style"}
    merge -->|merge_commit| mergeCommit["merge --no-ff --no-commit"]
    merge -->|fast_forward| fastForward["merge --ff-only"]
    merge -->|squash| squash["merge --squash"]
    mergeCommit --> commit["closure commit"]
    fastForward --> commit
    squash --> commit
    commit --> audit["record integration event and commit evidence"]
```

## Durable State Model

Backlog markdown remains declarative. Runtime records are keyed from backlog
items and task ids, then reconciled against Git trailers and evidence. This is
a conceptual model: `GIT_TRAILER` is derived from Git history, not stored as a
SQLite table.

```mermaid
erDiagram
    BACKLOG_ITEM ||--o{ TASK : dispatches
    TASK ||--o{ TASK_EVENT : records
    TASK ||--o| WORKTREE : owns
    TASK ||--o{ EVIDENCE : supports
    BACKLOG_ITEM ||--o{ FINDING : discovers
    TASK ||--o{ FINDING : reports
    BACKLOG_ITEM ||--o{ GIT_TRAILER : closes

    BACKLOG_ITEM {
        string id
        string title
        string priority
        string area
    }
    TASK {
        string id
        string source_item_id
        string status
        string worker
    }
    WORKTREE {
        string task_id
        string path
        string branch
        string base_ref
    }
    EVIDENCE {
        string id
        string kind
        string source_task_id
        string summary
    }
    FINDING {
        string id
        string source_item_id
        string status
        string owner
    }
    GIT_TRAILER {
        string key
        string value
        string commit
    }
```

## Runtime Backend Modes

Local SQLite mode is the reference implementation. Product code should target
the domain-shaped `ProjectState` boundary, while SQLite owns schema, SQL,
transactions, row mapping, and migrations internally.

```mermaid
flowchart LR
    host["MCP host"]
    server["Platypus MCP server"]
    state["ProjectState<br/>domain operations"]
    sqlite["SqliteProjectState<br/>.platy/platypus.sqlite3"]
    repo["project repository"]
    git["Git history"]
    worker["worker harness"]

    host -->|stdio MCP| server
    server -->|dispatch, assign, complete, replay| state
    state -->|private SQL and transactions| sqlite
    server -->|backlog, plans, worktrees| repo
    server -->|trailers and branches| git
    host -->|bundle| worker
    worker -->|result tools| server
```

A future shared backend keeps the MCP tool contract stable while moving
runtime coordination behind another `ProjectState` implementation. Repository
files and Git history remain the source of executable intent and integration
proof.

```mermaid
flowchart LR
    hostA["Host A"]
    hostB["Host B"]
    serverA["Platypus MCP server A"]
    serverB["Platypus MCP server B"]
    stateA["ProjectState A"]
    stateB["ProjectState B"]
    shared["shared backend<br/>events, tasks, approvals, leases"]
    repo["project repository"]
    git["Git history"]
    workerA["worker A"]
    workerB["worker B"]

    hostA --> serverA
    hostB --> serverB
    serverA --> stateA
    serverB --> stateB
    stateA --> shared
    stateB --> shared
    shared -->|ordered replay and leases| serverA
    shared -->|ordered replay and leases| serverB
    serverA --> repo
    serverB --> repo
    serverA --> git
    serverB --> git
    hostA --> workerA
    hostB --> workerB
    workerA --> serverA
    workerB --> serverB
```

## ProjectState Boundary

The public storage boundary is domain-shaped. MCP tools call Platypus
operations; backends decide how to persist and coordinate those operations.

```mermaid
flowchart TB
    tool["MCP tool handler"]
    service["Domain service"]
    state["ProjectState trait"]
    sqlite["SqliteProjectState"]
    memory["MemoryProjectState<br/>tests"]
    future["Future backend<br/>Postgres, remote, or event log"]

    tool -->|validate request and format ActionResult| service
    service -->|domain command| state
    state --> sqlite
    state --> memory
    state --> future

    sqlite -->|private| sql["SQL, migrations, rows"]
    memory -->|private| mem["in-memory snapshots"]
    future -->|private| remote["backend-specific protocol"]
```

The boundary must not expose SQL-like concepts such as tables, rows, query
builders, or database transactions. Atomic operations are part of the
`ProjectState` contract.

## Tool Responsibility Map

Each tool group should own a narrow part of the lifecycle. New tools should fit
one of these lanes or justify a new lane.

```mermaid
flowchart TB
    subgraph Project["Project setup"]
        init["init_project"]
        doctor["doctor_snapshot"]
        config["configure_agent_profile<br/>inspect_workflow_config"]
    end

    subgraph Planning["Backlog planning"]
        draft["draft_backlog_items"]
        create["create_backlog_item"]
        validate["validate_backlog"]
        list["list_backlog"]
    end

    subgraph Runtime["Task runtime"]
        next["next_safe_action"]
        dispatch["dispatch_ready_work"]
        handoff["prepare_worker_handoff<br/>low-level fallback"]
        start["start_worker_task"]
        progress["record_worker_progress"]
        complete["complete_worker_task"]
    end

    subgraph Review["Review and integration"]
        diff["inspect_worktree_changes"]
        verify["record_verification_evidence"]
        integrate["integrate_worker_result"]
        cleanup["worktree_cleanup"]
    end

    subgraph Governance["Governance"]
        findings["record/list/validate findings"]
        evidence["record/list evidence"]
        approvals["approval_list/respond"]
        events["events_replay"]
        reconcile["reconcile_project"]
    end

    Project --> Planning --> Runtime --> Review --> Governance
    Governance --> Runtime
```

## External Intake And Reporting

External systems can seed or receive context, but the local backlog snapshot is
the executable contract. Provider writes should be explicit, reviewable actions
with approval and audit metadata.

```mermaid
flowchart LR
    tracker["External tracker<br/>GitHub, Linear, Jira"]
    host["MCP host<br/>approved provider access"]
    import["import or draft<br/>provider-neutral records"]
    backlog["local backlog snapshot<br/>backlog/items"]
    runtime["runtime execution<br/>tasks, worktrees, evidence"]
    report["approved report draft<br/>summary, PR, verification"]
    approval{"approval policy"}
    writeback["provider write-back<br/>comment, label, status"]

    tracker --> host --> import --> backlog --> runtime --> report --> approval
    approval -->|approved| writeback --> tracker
    approval -->|denied or unavailable| backlog
```

## Reconciliation Perspective

Reconciliation is the product guardrail. It decides whether the project state
can be considered handled, independent of what a chat message claimed.

```mermaid
flowchart LR
    tasks["task records"]
    worktrees["worktree state"]
    evidence["verification and commit evidence"]
    findings["finding dispositions"]
    trailers["Git closure trailers"]
    reconcile["reconcile_project"]
    clean["clean project snapshot"]
    gaps["structured gaps"]

    tasks --> reconcile
    worktrees --> reconcile
    evidence --> reconcile
    findings --> reconcile
    trailers --> reconcile
    reconcile -->|all required proof present| clean
    reconcile -->|missing proof or unresolved work| gaps
```

## Roadmap Dependency View

The current next work should strengthen the proof loop around the integration
path before adding richer host guidance.

```mermaid
flowchart TD
    mcp019["MCP-019<br/>workflow integration config"]
    mcp020["MCP-020<br/>managed worker result integration"]
    mcp023["MCP-023<br/>repository workflow docs"]
    mcp021["MCP-021<br/>evidence and reconciliation coverage"]
    mcp022["MCP-022<br/>full lifecycle smoke scenario"]
    mcp024["MCP-024<br/>MCP resources and prompts"]

    mcp019 --> mcp020
    mcp020 --> mcp021
    mcp023 --> mcp021
    mcp021 --> mcp022
    mcp023 --> mcp024
    mcp022 --> mcp024
```
