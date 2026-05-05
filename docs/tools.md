# MCP Tool Contract

Platypus tools return typed JSON with a shared envelope:

- `action`: stable tool action name
- `status`: `completed`, `skipped`, or `failed`
- `summary`: concise human-readable result
- `next_action`: optional recovery or continuation instruction
- `data`: typed result payload
- `error`: optional failure detail

## Available Tools

### `ping`

Health check. Returns the supplied `message` or `pong`.

### `inspect_status`

Inspects the configured project root and reports whether `platy.yaml`,
`backlog/`, and `.git` exist, plus backlog item counts.

### `project_status`

Compatibility alias for `inspect_status`.

### `list_backlog`

Returns runnable backlog candidates. Items are runnable when they are `todo` or
`ready` and all dependencies are `done`.

### `validate_backlog`

Validates backlog item and epic frontmatter, required sections, dependency
references, and basic enum values.

### `draft_backlog_items`

Generates a deterministic three-step backlog draft from a goal: shape,
implement, and verify.

### `create_backlog_item`

Creates one structured backlog item under `backlog/items/`. Required semantic
fields are `title`, `goal`, `implementation_contract` or `contract`, and at
least one `acceptance` criterion.

### Runtime And Finding Tools

These tools are intentionally present but return `skipped` until durable storage
and runtime integration are implemented:

- `dispatch_next_work`
- `inspect_task_events`
- `send_worker_guidance`
- `list_findings`
- `validate_findings`
- `update_finding_disposition`

Keeping these names stable lets MCP clients discover the intended product shape
without guessing hidden commands.
