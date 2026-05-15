import { resolve } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Text, truncateToWidth } from "@earendil-works/pi-tui";
import { Type } from "typebox";
import {
	buildCompletePrompt,
	buildPlanPrompt,
	buildShortcutStartPrompt,
	buildStartPrompt,
	noReadyItemMessage,
} from "./commands.mjs";
import {
	buildEngineeringStandardsPrompt,
	buildDirectionPrompt,
	readProjectDirectionSummary,
} from "./direction.mjs";
import {
	compactStatus,
	renderDashboardLines,
	renderToolResultLines,
	shouldShowGuidance,
	snapshotFromDetails,
} from "./renderers.mjs";
import {
	buildImplementationPlanReviewPrompt,
	buildPostImplementationReviewPrompt,
	buildStoryReviewPrompt,
} from "./review.mjs";
import { runPlatypusTool as runPlatypusToolRuntime } from "./runtime.mjs";

type JsonObject = Record<string, unknown>;

type PlatypusTool = {
	name: string;
	description: string;
	promptSnippet?: string;
	parameters?: unknown;
};

type ReadyItem = {
	id: string;
	title: string;
	priority?: string;
	state?: string;
	recommendedTool?: string;
	reason?: string;
};

type PlatypusSnapshot = {
	ready: number;
	blocked: number;
	active: number;
	closed?: number;
	total?: number;
	pendingIntegration?: number;
	root?: string;
	status?: "ok" | "error" | "unknown";
	summary?: string;
	nextAction?: string;
	recommendedTool?: string;
	readyItems: ReadyItem[];
	blockedItems: ReadyItem[];
	updatedAt?: number;
	error?: string;
};

const PACKAGE_ROOT = resolve(import.meta.dirname, "../..");
const STATUS_KEY = "platypus";
const WIDGET_KEY = "platypus-queue";

const passthroughParameters = Type.Object(
	{},
	{
		additionalProperties: true,
		description:
			"Arguments forwarded to the matching Platypus MCP tool. The project root is fixed to the current pi working directory and cannot be overridden.",
	},
);

const noArgs = (description: string) =>
	Type.Object(
		{},
		{
			additionalProperties: false,
			description,
		},
	);

const optionalLimit = (maximum = 200) =>
	Type.Optional(
		Type.Integer({
			description: `Maximum number of records to return, from 1 to ${maximum}.`,
			minimum: 1,
			maximum,
		}),
	);

const stringList = (description: string) =>
	Type.Optional(
		Type.Array(Type.String({ minLength: 1 }), {
			description,
			default: [],
		}),
	);

const itemId = (description = "Backlog item identifier, for example MCP-123.") =>
	Type.String({
		description,
		pattern: "^[A-Z]+-[0-9]{3}$",
	});

const taskId = Type.String({ description: "Task identifier returned by Platypus work preparation or dispatch." });
const assignmentId = Type.String({ description: "Worker assignment identifier returned by Platypus work preparation." });

const priority = Type.Union([Type.Literal("P0"), Type.Literal("P1"), Type.Literal("P2")], {
	description: "Backlog priority. Use P0 for urgent/foundational work, P1 for important follow-up, and P2 for lower-priority polish.",
});

const itemType = Type.Union(
	[Type.Literal("foundation"), Type.Literal("feature"), Type.Literal("safety"), Type.Literal("ux"), Type.Literal("test"), Type.Literal("docs")],
	{ description: "Backlog item type." },
);

const executionPath = Type.Union([Type.Literal("direct_edit"), Type.Literal("worker_handoff")], {
	description: "Durable execution path. direct_edit means edit the manager workspace; worker_handoff means prepare isolated handoff state.",
});

const planningGate = Type.Union([Type.Literal("none"), Type.Literal("task_plan"), Type.Literal("approved_task_plan")], {
	description: "Planning gate required before work can start.",
});

const verificationStatus = Type.Union(
	[Type.Literal("passed"), Type.Literal("failed"), Type.Literal("skipped"), Type.Literal("not_run")],
	{ description: "Verification status for the completed work." },
);

const taskPlanMode = Type.Union([Type.Literal("minimal"), Type.Literal("standard"), Type.Literal("full")], {
	description: "Task plan mode. Use minimal for small direct work, standard for normal planned work, and full for broader review-sensitive work.",
});

const workerStatus = Type.Union([Type.Literal("completed"), Type.Literal("failed"), Type.Literal("cancelled")], {
	description: "Terminal status for a worker handoff result.",
});

const evidenceKind = Type.Union(
	[
		Type.Literal("commit"),
		Type.Literal("verification"),
		Type.Literal("file_summary"),
		Type.Literal("worker_finding"),
		Type.Literal("manager_disposition"),
		Type.Literal("external_report"),
		Type.Literal("note"),
	],
	{ description: "Evidence kind." },
);

const findingSeverity = Type.Union([Type.Literal("low"), Type.Literal("medium"), Type.Literal("high"), Type.Literal("critical")], {
	description: "Severity of a worker-reported or manager-recorded finding.",
});

const findingDisposition = Type.Union(
	[
		Type.Literal("open"),
		Type.Literal("accepted"),
		Type.Literal("resolved"),
		Type.Literal("rejected"),
		Type.Literal("deferred"),
		Type.Literal("duplicate"),
	],
	{ description: "Disposition state for a finding." },
);

const detailLevel = Type.Union([Type.Literal("compact"), Type.Literal("verbose")], {
	description: "Response detail level. Use compact for normal workflow and verbose when debugging.",
});

const externalRef = Type.Object(
	{
		kind: Type.Optional(Type.String({ description: "Reference type, for example issue, pr, url, or note." })),
		id: Type.Optional(Type.String({ description: "Provider-specific external identifier." })),
		url: Type.Optional(Type.String({ description: "Canonical URL for this external reference." })),
		title: Type.Optional(Type.String({ description: "Short label for this external reference." })),
	},
	{
		additionalProperties: true,
		description: "External reference attached to a backlog item.",
	},
);

const findingInput = Type.Object(
	{
		title: Type.String({ description: "Human-readable finding title." }),
		summary: Type.String({ description: "What was found and why it matters." }),
		severity: Type.Optional(findingSeverity),
		required: Type.Optional(Type.Boolean({ description: "Whether this finding must be dispositioned before final integration." })),
		owner: Type.Optional(Type.String({ description: "Owner or responsible party." })),
		evidence_refs: stringList("Evidence references that support this finding."),
	},
	{
		additionalProperties: false,
		description: "Follow-up finding discovered during worker execution.",
	},
);

const backlogItemShape = {
	client_key: Type.Optional(Type.String({ description: "Caller-local key used by depends_on_keys inside the same batch." })),
	depends_on_keys: stringList("Client keys from this same batch that this item depends on."),
	id: Type.Optional(itemId("Explicit backlog item id. Usually omit and let Platypus allocate one.")),
	id_prefix: Type.Optional(Type.String({ description: "Uppercase prefix used when allocating an id.", pattern: "^[A-Z]+$" })),
	title: Type.Optional(Type.String({ description: "Human-readable title. Omit when goal is enough for Platypus to derive a usable title." })),
	priority: Type.Optional(priority),
	type: Type.Optional(itemType),
	area: Type.Optional(Type.String({ description: "Primary area or product surface." })),
	epic: Type.Optional(Type.String({ description: "Existing epic id. Defaults to general when omitted." })),
	depends_on: stringList("Existing backlog item ids that must be complete before this item."),
	owned_surfaces: stringList("Relative paths or top-level areas expected to change."),
	external_refs: Type.Optional(Type.Array(externalRef, { description: "External references attached to this item." })),
	execution_path: Type.Optional(executionPath),
	planning_gate: Type.Optional(planningGate),
	goal: Type.Optional(Type.String({ description: "Goal text that drives this item. Omit only when title already describes the work clearly." })),
	implementation_contract: Type.Optional(Type.String({ description: "Specific implementation contract. Do not invent fake details." })),
	contract: Type.Optional(Type.String({ description: "Alias for implementation_contract. Provide only one of these fields." })),
	acceptance: stringList("Acceptance criteria for this item."),
	notes: Type.Optional(Type.String({ description: "Optional notes." })),
};

const backlogUpdateShape = {
	item_id: itemId("Existing backlog item id to update."),
	title: Type.Optional(Type.String({ description: "Updated human-readable title." })),
	priority: Type.Optional(priority),
	type: Type.Optional(itemType),
	area: Type.Optional(Type.String({ description: "Updated primary area or product surface." })),
	epic: Type.Optional(Type.String({ description: "Existing epic id. Create the epic before updating when needed." })),
	depends_on: Type.Optional(Type.Array(itemId(), { description: "Replacement dependency list. Use an empty list when there are no dependencies." })),
	owned_surfaces: Type.Optional(Type.Array(Type.String({ minLength: 1 }), { description: "Replacement owned surfaces list." })),
	external_refs: Type.Optional(Type.Array(externalRef, { description: "Replacement external references." })),
	execution_path: Type.Optional(executionPath),
	planning_gate: Type.Optional(planningGate),
	goal: Type.Optional(Type.String({ description: "Updated goal text." })),
	implementation_contract: Type.Optional(Type.String({ description: "Updated implementation contract." })),
	contract: Type.Optional(Type.String({ description: "Alias for implementation_contract. Provide only one of these fields." })),
	acceptance: Type.Optional(Type.Array(Type.String({ minLength: 1 }), { description: "Replacement acceptance criteria." })),
	notes: Type.Optional(Type.String({ description: "Updated notes. Empty string removes notes." })),
	force_closed: Type.Optional(Type.Boolean({ description: "Allow updating an item already closed by Git trailer or direct completion." })),
};

const taskPlanRequirement = Type.Object(
	{
		id: Type.String({ description: "Stable requirement id, for example R1." }),
		text: Type.String({ description: "Requirement text." }),
	},
	{ additionalProperties: false, description: "Task plan requirement." },
);

const taskPlanDesign = Type.Object(
	{
		summary: Type.String({ description: "Implementation design summary." }),
		owned_surfaces: Type.Array(Type.String({ minLength: 1 }), { description: "Owned files, modules, or directories for this plan." }),
		notes: Type.Optional(Type.Union([Type.String(), Type.Null()], { description: "Optional design notes." })),
	},
	{ additionalProperties: false, description: "Task plan design section." },
);

const plannedTask = Type.Object(
	{
		id: Type.String({ description: "Stable task id, for example MCP-123-T001." }),
		title: Type.String({ description: "Task title." }),
		goal: Type.String({ description: "Task goal." }),
		requirement_refs: Type.Array(Type.String({ minLength: 1 }), { description: "Requirement ids covered by this task." }),
		depends_on: Type.Array(Type.String({ minLength: 1 }), { description: "Task ids that must complete first." }),
		owned_surfaces: Type.Array(Type.String({ minLength: 1 }), { description: "Owned files, modules, or directories for this task." }),
		verification: Type.Array(Type.String({ minLength: 1 }), { description: "Verification commands or evidence for this task." }),
		acceptance: Type.Array(Type.String({ minLength: 1 }), { description: "Task acceptance criteria." }),
		notes: Type.Optional(Type.Union([Type.Array(Type.String({ minLength: 1 })), Type.Null()], { description: "Optional task notes." })),
	},
	{ additionalProperties: false, description: "Executable task slice inside a task plan." },
);

const taskPlanFile = Type.Object(
	{
		item_id: itemId("Backlog item id for this task plan."),
		version: Type.Optional(Type.Integer({ description: "Task plan schema version. Defaults to 1.", minimum: 1 })),
		mode: taskPlanMode,
		requirements: Type.Array(taskPlanRequirement, { description: "Requirements captured by the task plan.", minItems: 1 }),
		design: taskPlanDesign,
		tasks: Type.Array(plannedTask, { description: "Planned implementation tasks.", minItems: 1 }),
	},
	{ additionalProperties: false, description: "Strict task plan file content." },
);

const CORE_TYPED_TOOL_NAMES = new Set<string>([
	"inspect_session",
	"inspect_status",
	"inspect_work_queue",
	"inspect_queue_status",
	"init_project",
	"list_backlog",
	"validate_backlog",
	"get_backlog_item",
	"create_backlog_item",
	"create_backlog_items",
	"update_backlog_item",
	"write_task_plan",
	"validate_task_plan",
	"inspect_task_plan",
	"list_task_plans",
	"prepare_work",
	"complete_backlog_item",
	"finish_work",
	"record_evidence",
	"record_finding",
	"list_findings",
	"validate_findings",
	"update_finding_disposition",
	"events_replay",
	"doctor_snapshot",
	"inspect_workflow_config",
]);

const coreToolParameters: Record<string, unknown> = {
	inspect_session: Type.Object(
		{
			limit: optionalLimit(),
			detail: Type.Optional(detailLevel),
		},
		{ additionalProperties: false, description: "Inspect session startup state and queue guidance." },
	),
	inspect_status: noArgs("Inspect high-level Platypus project status."),
	inspect_work_queue: Type.Object(
		{
			limit: optionalLimit(),
		},
		{ additionalProperties: false, description: "Inspect ready, blocked, and active backlog work." },
	),
	inspect_queue_status: Type.Object(
		{
			limit: optionalLimit(50),
		},
		{ additionalProperties: false, description: "Inspect compact queue counts and top items." },
	),
	init_project: Type.Object(
		{
			project_name: Type.Optional(Type.String({ description: "Project name written into Platypus configuration." })),
			overwrite: Type.Optional(Type.Boolean({ description: "Overwrite existing project guidance files." })),
		},
		{ additionalProperties: false, description: "Initialize Platypus project guidance files in Pi's current working directory." },
	),
	list_backlog: Type.Object(
		{
			limit: optionalLimit(),
		},
		{ additionalProperties: false, description: "List Platypus backlog items." },
	),
	validate_backlog: Type.Object(
		{
			include_errors: Type.Optional(Type.Boolean({ description: "Include validation error details." })),
		},
		{ additionalProperties: false, description: "Validate backlog files." },
	),
	get_backlog_item: Type.Object(
		{
			item_id: itemId(),
		},
		{ additionalProperties: false, description: "Inspect one backlog item." },
	),
	create_backlog_item: Type.Object(backlogItemShape, {
		additionalProperties: false,
		description: "Create one backlog item. The project root is fixed by Pi and must not be supplied.",
	}),
	create_backlog_items: Type.Object(
		{
			id_prefix: Type.Optional(Type.String({ description: "Default uppercase id prefix for items without explicit ids.", pattern: "^[A-Z]+$" })),
			items: Type.Array(Type.Object(backlogItemShape, { additionalProperties: false }), {
				description: "Backlog items to create atomically. If any item is invalid, no items are written.",
				minItems: 1,
			}),
			preview: Type.Optional(Type.Boolean({ description: "Preview without writing files." })),
			detail: Type.Optional(detailLevel),
		},
		{ additionalProperties: false, description: "Create multiple backlog items atomically." },
	),
	update_backlog_item: Type.Object(backlogUpdateShape, {
		additionalProperties: false,
		description: "Update one existing backlog item after review. The project root is fixed by Pi and must not be supplied.",
	}),
	write_task_plan: Type.Object(
		{
			item_id: itemId("Backlog item id for this task plan."),
			plan: taskPlanFile,
			overwrite: Type.Optional(Type.Boolean({ description: "Overwrite an existing task plan." })),
		},
		{ additionalProperties: false, description: "Write and validate a strict task plan." },
	),
	validate_task_plan: Type.Object(
		{
			item_id: Type.Optional(itemId("Optional backlog item id to validate one task plan.")),
			include_errors: Type.Optional(Type.Boolean({ description: "Include validation error details." })),
		},
		{ additionalProperties: false, description: "Validate strict task plan files." },
	),
	inspect_task_plan: Type.Object(
		{
			item_id: itemId("Backlog item id for the task plan to inspect."),
		},
		{ additionalProperties: false, description: "Read one committed task plan." },
	),
	list_task_plans: Type.Object(
		{
			item_id: Type.Optional(itemId("Optional backlog item id filter.")),
			include_errors: Type.Optional(Type.Boolean({ description: "Include validation error details." })),
		},
		{ additionalProperties: false, description: "List committed task plans." },
	),
	prepare_work: Type.Object(
		{
			item_id: Type.Optional(itemId()),
			max_tasks: optionalLimit(10),
			worker: Type.Optional(Type.String({ description: "Worker name to record for a worker handoff." })),
			claimant: Type.Optional(Type.String({ description: "Name recorded as the task claimant." })),
			execution_mode: Type.Optional(Type.Union([Type.Literal("auto"), Type.Literal("manual_handoff")], { description: "Execution mode to prepare." })),
			auto_commit_artifacts: Type.Optional(Type.Boolean({ description: "Auto-commit only Platypus planning artifacts when they are the only workspace changes." })),
			verification_command: stringList("Verification command to run or record for this item."),
			include_queue_snapshot: Type.Optional(Type.Boolean({ description: "Include the full queue snapshot in the response." })),
		},
		{ additionalProperties: false, description: "Prepare direct guidance or a worker handoff." },
	),
	complete_backlog_item: Type.Object(
		{
			item_id: itemId(),
			summary: Type.String({ description: "Human-readable summary of the completed direct work." }),
			changed_files: stringList("Files changed by the direct work, relative to the project root."),
			verification_status: Type.Optional(verificationStatus),
			verification_summary: Type.Optional(Type.String({ description: "Summary of verification performed." })),
			verification_refs: stringList("Commands, files, commits, URLs, or evidence ids supporting verification."),
			evidence_refs: stringList("Existing evidence identifiers or references supporting completion."),
			finding_refs: stringList("Finding identifiers reviewed for this completion."),
			record_auto_evidence: Type.Optional(Type.Boolean({ description: "Automatically record completion and verification evidence. Defaults to true." })),
			commit: Type.Optional(Type.Boolean({ description: "Create a Git closure commit with Platypus trailers." })),
			commit_message: Type.Optional(Type.String({ description: "Optional closure commit subject when commit is true." })),
			detail: Type.Optional(detailLevel),
		},
		{ additionalProperties: false, description: "Complete a direct-edit backlog item." },
	),
	finish_work: Type.Object(
		{
			item_id: Type.Optional(itemId("Backlog item id for direct-work recovery guidance.")),
			assignment_id: Type.Optional(assignmentId),
			task_id: Type.Optional(taskId),
			status: Type.Optional(workerStatus),
			summary: Type.String({ description: "Human-readable worker result summary." }),
			changed_files: stringList("Files changed by the worker, relative to the task worktree."),
			verification_status: Type.Optional(verificationStatus),
			verification_summary: Type.Optional(Type.String({ description: "Summary of verification performed by the worker." })),
			verification_refs: stringList("Commands, files, commits, URLs, or evidence ids supporting verification."),
			findings: Type.Optional(Type.Array(findingInput, { description: "Findings or follow-up work discovered during implementation." })),
			findings_reviewed: Type.Optional(Type.Boolean({ description: "Set true only when the worker explicitly checked for follow-up findings and found none." })),
			auto_start_if_prepared: Type.Optional(Type.Boolean({ description: "Allow prepared assignments to be auto-started before completion." })),
			integrate_if_ready: Type.Optional(Type.Boolean({ description: "Integrate the completed task when verification and finding gates permit it." })),
			allow_unverified: Type.Optional(Type.Boolean({ description: "Explicitly permit integration without separate verification evidence." })),
			integration_strategy: Type.Optional(Type.Union([Type.Literal("merge_commit"), Type.Literal("fast_forward"), Type.Literal("squash"), Type.Literal("apply_changed_files")], { description: "Integration strategy override." })),
			cleanup_after: Type.Optional(Type.Boolean({ description: "Remove the task worktree after successful integration when clean." })),
		},
		{ additionalProperties: false, description: "Finish worker-handoff work and optionally integrate it." },
	),
	record_evidence: Type.Object(
		{
			id: Type.Optional(Type.String({ description: "Explicit evidence id. Usually omit." })),
			source_item_id: Type.Optional(itemId("Source backlog item id.")),
			source_task_id: Type.Optional(taskId),
			kind: evidenceKind,
			summary: Type.String({ description: "Human-readable evidence summary." }),
			refs: stringList("References such as commands, files, commits, URLs, or evidence ids."),
			metadata: Type.Optional(Type.Object({}, { additionalProperties: true, description: "Free-form evidence metadata." })),
		},
		{ additionalProperties: false, description: "Record traceability evidence." },
	),
	record_finding: Type.Object(
		{
			id: Type.Optional(Type.String({ description: "Explicit finding id. Usually omit so Platypus allocates one." })),
			source_item_id: Type.Optional(itemId("Source backlog item id.")),
			source_task_id: Type.Optional(taskId),
			source_finding_ref: Type.Optional(Type.String({ description: "External or worker-local finding reference." })),
			title: Type.String({ description: "Human-readable finding title." }),
			summary: Type.String({ description: "Finding summary, risk, limitation, or required follow-up." }),
			severity: Type.Optional(findingSeverity),
			required: Type.Optional(Type.Boolean({ description: "Whether this finding must be dispositioned before final integration." })),
			evidence_refs: stringList("Evidence references supporting this finding."),
			metadata: Type.Optional(Type.Object({}, { additionalProperties: true, description: "Free-form finding metadata." })),
		},
		{ additionalProperties: false, description: "Record a Platypus finding or follow-up discovered during review." },
	),
	list_findings: Type.Object(
		{
			source_item_id: Type.Optional(itemId("Source backlog item id.")),
			source_task_id: Type.Optional(taskId),
			status: Type.Optional(findingDisposition),
			limit: optionalLimit(100),
		},
		{ additionalProperties: false, description: "List findings." },
	),
	validate_findings: Type.Object(
		{
			source_item_id: Type.Optional(itemId("Source backlog item id.")),
			source_task_id: Type.Optional(taskId),
		},
		{ additionalProperties: false, description: "Validate required finding disposition state." },
	),
	update_finding_disposition: Type.Object(
		{
			finding_id: Type.String({ description: "Finding identifier." }),
			status: findingDisposition,
			owner: Type.Optional(Type.String({ description: "Owner or responsible party." })),
			disposition_reason: Type.Optional(Type.String({ description: "Reason for the disposition decision." })),
			evidence_refs: stringList("Evidence references supporting this disposition."),
			metadata: Type.Optional(Type.Object({}, { additionalProperties: true, description: "Free-form disposition metadata." })),
		},
		{ additionalProperties: false, description: "Update a finding disposition." },
	),
	events_replay: Type.Object(
		{
			task_id: Type.Optional(taskId),
			scope: Type.Optional(Type.String({ description: "Event scope filter, for example project, task, backlog, or evidence." })),
			limit: optionalLimit(),
		},
		{ additionalProperties: false, description: "Replay recent Platypus events." },
	),
	doctor_snapshot: noArgs("Inspect diagnostics and recovery guidance."),
	inspect_workflow_config: noArgs("Inspect workflow execution and integration policy."),
};

function parametersForTool(name: string): unknown {
	const parameters = coreToolParameters[name];
	if (CORE_TYPED_TOOL_NAMES.has(name) && !parameters) {
		throw new Error(`Platypus Pi core tool ${name} is missing typed parameters.`);
	}
	return parameters ?? passthroughParameters;
}

const platypusTools: PlatypusTool[] = [
	{
		name: "inspect_session",
		description: "Inspect Platypus workflow, project status, queue state, guidance, and likely next actions.",
		promptSnippet: "Inspect Platypus project-management state and recommended next actions.",
	},
	{
		name: "inspect_toolsets",
		description: "Inspect compact Platypus toolset discovery metadata for startup, planning, execution, handoff, evidence, or recovery.",
	},
	{
		name: "inspect_status",
		description: "Inspect high-level Platypus project status.",
	},
	{
		name: "inspect_work_queue",
		description: "Inspect ready, blocked, running, and recently closed Platypus backlog work.",
		promptSnippet: "Inspect executable Platypus backlog work and blockers.",
	},
	{
		name: "inspect_queue_status",
		description: "Inspect compact Platypus work-queue status.",
	},
	{
		name: "init_project",
		description: "Initialize Platypus project-management files in the current project.",
	},
	{
		name: "list_backlog",
		description: "List Platypus backlog items.",
	},
	{
		name: "validate_backlog",
		description: "Validate Platypus backlog files and report issues.",
	},
	{
		name: "get_backlog_item",
		description: "Get detailed state for a single Platypus backlog item.",
	},
	{
		name: "create_backlog_item",
		description: "Create one Platypus backlog item from typed arguments.",
	},
	{
		name: "create_backlog_items",
		description: "Create multiple Platypus backlog items from typed arguments.",
	},
	{
		name: "update_backlog_item",
		description: "Update one reviewed Platypus backlog item from typed arguments.",
	},
	{
		name: "write_task_plan",
		description: "Write and validate one strict Platypus task plan.",
	},
	{
		name: "validate_task_plan",
		description: "Validate Platypus task plan files.",
	},
	{
		name: "inspect_task_plan",
		description: "Inspect one Platypus task plan.",
	},
	{
		name: "list_task_plans",
		description: "List Platypus task plans.",
	},
	{
		name: "prepare_work",
		description: "Prepare a Platypus backlog item for direct execution or worker handoff.",
		promptSnippet: "Prepare Platypus work when response-local guidance or worker handoff is needed.",
	},
	{
		name: "complete_backlog_item",
		description: "Complete a direct-edit Platypus backlog item with summary and verification metadata.",
		promptSnippet: "Complete direct Platypus backlog work after implementation and verification.",
	},
	{
		name: "dispatch_ready_work",
		description: "Dispatch ready Platypus backlog work according to project policy.",
	},
	{
		name: "finish_work",
		description: "Finish worker-handoff Platypus work and record worker completion details.",
	},
	{
		name: "inspect_integration_gates",
		description: "Inspect Platypus integration gates for worker results awaiting integration.",
	},
	{
		name: "integrate_worker_result",
		description: "Record integration outcome for a Platypus worker result.",
	},
	{
		name: "record_evidence",
		description: "Record Platypus evidence for verification, findings, handoff, or recovery.",
	},
	{
		name: "record_finding",
		description: "Record a Platypus finding, limitation, risk, or required follow-up.",
	},
	{
		name: "list_findings",
		description: "List Platypus findings.",
	},
	{
		name: "validate_findings",
		description: "Validate Platypus findings state.",
	},
	{
		name: "update_finding_disposition",
		description: "Update disposition for a Platypus finding.",
	},
	{
		name: "events_replay",
		description: "Replay Platypus project-management events.",
	},
	{
		name: "doctor_snapshot",
		description: "Inspect a Platypus diagnostic snapshot for setup and recovery.",
	},
	{
		name: "inspect_workflow_config",
		description: "Inspect Platypus workflow integration policy for the current project.",
	},
];

async function runPlatypusTool(pi: ExtensionAPI, ctx: ExtensionContext, toolName: string, params: JsonObject) {
	return runPlatypusToolRuntime(pi, ctx, toolName, params, { packageRoot: PACKAGE_ROOT });
}

function renderDashboardText(snapshot?: PlatypusSnapshot, expanded = false): string {
	return renderDashboardLines(snapshot, { expanded }).join("\n");
}

function guidanceForSnapshot(snapshot?: PlatypusSnapshot): string | undefined {
	if (!snapshot || snapshot.error) return undefined;
	const lines = ["## Current Platypus Queue Snapshot", renderDashboardText(snapshot, false)];
	if (snapshot.ready > 0) {
		lines.push(
			"When working a direct_ready Platypus item, use the direct loop: inspect acceptance criteria, edit the manager workspace, verify, then call platypus_complete_backlog_item with summary, changed_files, and verification_status.",
			"Call platypus_prepare_work only when response-local guidance or worker handoff is useful.",
		);
	} else if ((snapshot.total ?? 0) === 0) {
		lines.push(
			"The backlog is empty. Ask the user for the product goal if needed, then call platypus_create_backlog_items with concrete items, acceptance criteria, owned_surfaces, execution_path, and planning_gate.",
			"Do not invent implementation details that are not implied by the user goal or repository state.",
		);
	} else {
		lines.push("No item is ready right now. Use platypus_inspect_work_queue or platypus_doctor_snapshot to explain blockers and the next safe action.");
	}
	return lines.join("\n");
}

function updateUi(ctx: ExtensionContext, snapshot: PlatypusSnapshot | undefined, widgetVisible: boolean) {
	if (!ctx.hasUI) return;
	const theme = ctx.ui.theme;
	if (!snapshot) {
		ctx.ui.setStatus(STATUS_KEY, theme.fg("dim", "platypus: idle"));
		ctx.ui.setWidget(WIDGET_KEY, undefined);
		return;
	}

	if (snapshot.error) {
		ctx.ui.setStatus(STATUS_KEY, theme.fg("error", "platypus: error"));
	} else if (snapshot.ready > 0) {
		ctx.ui.setStatus(STATUS_KEY, theme.fg("success", compactStatus(snapshot)));
	} else if (snapshot.blocked > 0) {
		ctx.ui.setStatus(STATUS_KEY, theme.fg("warning", compactStatus(snapshot)));
	} else {
		ctx.ui.setStatus(STATUS_KEY, theme.fg("dim", compactStatus(snapshot)));
	}

	if (!widgetVisible) {
		ctx.ui.setWidget(WIDGET_KEY, undefined);
		return;
	}

	ctx.ui.setWidget(WIDGET_KEY, (_tui, theme) => ({
		invalidate() {},
		render(width: number) {
			const text = renderDashboardText(snapshot, false);
			return text.split("\n").map((line, index) => {
				const styled = index === 0 ? theme.fg("accent", line) : line.startsWith("▶") ? theme.fg("success", line) : line.startsWith("Blocked:") ? theme.fg("warning", line) : theme.fg("dim", line);
				return truncateToWidth(styled, Math.max(1, width));
			});
		},
	}));
}

function renderToolDashboard(result: { details?: unknown; content?: Array<{ text?: string }> }, theme: { fg: (color: string, text: string) => string }) {
	const lines = renderToolResultLines(result, { expanded: true });
	return new Text(lines.map((line: string, index: number) => {
		if (index === 0 && line.startsWith("!")) return theme.fg("error", line);
		if (line.startsWith("▶") || line.startsWith("✓")) return theme.fg("success", line);
		if (line.startsWith("Blocked:") || line.startsWith("Next:")) return theme.fg("warning", line);
		if (index === 0) return theme.fg("accent", line);
		return line;
	}).join("\n"), 0, 0);
}

export default function platypusPiExtension(pi: ExtensionAPI) {
	let latestSnapshot: PlatypusSnapshot | undefined;
	let widgetVisible = true;
	let lastInjectedPrompt = "";

	const applySnapshot = (ctx: ExtensionContext, snapshot: PlatypusSnapshot | undefined) => {
		latestSnapshot = snapshot;
		updateUi(ctx, latestSnapshot, widgetVisible);
	};

	const refreshSnapshot = async (ctx: ExtensionContext, notify = false) => {
		const result = await runPlatypusTool(pi, ctx, "inspect_session", {});
		const snapshot = snapshotFromDetails(result.details) ?? {
			ready: 0,
			blocked: 0,
			active: 0,
			status: "error" as const,
			readyItems: [],
			blockedItems: [],
			updatedAt: Date.now(),
			error: result.content[0]?.text ?? "inspect_session returned an unrecognized response.",
		};
		applySnapshot(ctx, snapshot);
		if (notify && ctx.hasUI) ctx.ui.notify(renderDashboardText(snapshot, true), snapshot.error ? "error" : "info");
		return snapshot;
	};

	for (const tool of platypusTools) {
		pi.registerTool({
			name: `platypus_${tool.name}`,
			label: `Platypus ${tool.name}`,
			description: tool.description,
			promptSnippet: tool.promptSnippet,
			parameters: tool.parameters ?? parametersForTool(tool.name),
			async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
				const result = await runPlatypusTool(pi, ctx, tool.name, params as JsonObject);
				const snapshot = snapshotFromDetails(result.details);
				if (snapshot) applySnapshot(ctx, snapshot);
				return result;
			},
			renderCall(args, theme) {
				const suffix = Object.keys((args as JsonObject) ?? {}).length > 0 ? " with args" : "";
				return new Text(theme.fg("toolTitle", `platypus_${tool.name}`) + theme.fg("dim", suffix), 0, 0);
			},
			renderResult(result, _options, theme) {
				return renderToolDashboard(result, theme);
			},
		});
	}

	pi.registerTool({
		name: "platypus_call_tool",
		label: "Platypus Call Tool",
		description: "Call any Platypus MCP tool by its unprefixed MCP tool name. Use only when no dedicated platypus_* Pi tool is available.",
		parameters: Type.Object({
			name: Type.String({ description: "Unprefixed Platypus MCP tool name, for example inspect_session." }),
			arguments: Type.Optional(
				Type.Object({}, { additionalProperties: true, description: "Arguments forwarded to the MCP tool." }),
			),
		}),
		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			const input = params as { name: string; arguments?: JsonObject };
			const result = await runPlatypusTool(pi, ctx, input.name, input.arguments ?? {});
			const snapshot = snapshotFromDetails(result.details);
			if (snapshot) applySnapshot(ctx, snapshot);
			return result;
		},
		renderResult(result, _options, theme) {
			return renderToolDashboard(result, theme);
		},
	});

	pi.registerCommand("platy", {
		description: "Show compact Platypus backlog status",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			if (ctx.hasUI) ctx.ui.notify(renderDashboardText(latestSnapshot, true), latestSnapshot?.error ? "error" : "info");
		},
	});

	pi.registerCommand("platypus-status", {
		description: "Inspect Platypus session state for the current project",
		handler: async (_args, ctx) => {
			await refreshSnapshot(ctx, true);
		},
	});

	pi.registerCommand("platy-refresh", {
		description: "Refresh Platypus backlog status",
		handler: async (_args, ctx) => {
			await refreshSnapshot(ctx, true);
		},
	});

	pi.registerCommand("platy-ready", {
		description: "Show the next runnable Platypus backlog item",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const text = latestSnapshot?.readyItems[0]
				? renderDashboardText({ ...latestSnapshot, blockedItems: [] }, true)
				: "No runnable Platypus backlog item is currently ready. Use /platy-plan to ask the agent to shape backlog items or /platy-doctor for recovery guidance.";
			if (ctx.hasUI) ctx.ui.notify(text, latestSnapshot?.readyItems[0] ? "info" : "warning");
		},
	});

	pi.registerCommand("platy-next", {
		description: "Show the current Platypus next action",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const text = latestSnapshot?.nextAction ?? renderDashboardText(latestSnapshot, true);
			if (ctx.hasUI) ctx.ui.notify(text, "info");
		},
	});

	pi.registerCommand("platy-plan", {
		description: "Ask the agent to create concrete Platypus backlog items from the current goal",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			pi.sendUserMessage(buildPlanPrompt(), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-direction", {
		description: "Capture durable project direction before backlog planning",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			pi.sendUserMessage(buildDirectionPrompt(), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-direction-revise", {
		description: "Revise existing durable project direction",
		handler: async (_args, ctx) => {
			pi.sendUserMessage(buildDirectionPrompt({ revision: true }), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-standards", {
		description: "Capture durable project engineering standards",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			pi.sendUserMessage(buildEngineeringStandardsPrompt(), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-standards-revise", {
		description: "Revise durable project engineering standards",
		handler: async (_args, ctx) => {
			pi.sendUserMessage(buildEngineeringStandardsPrompt({ revision: true }), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-story-review", {
		description: "Review a story draft or backlog item before execution",
		handler: async (args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const input = Array.isArray(args) ? args.join(" ") : String(args ?? "");
			pi.sendUserMessage(buildStoryReviewPrompt(input), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-plan-review", {
		description: "Review implementation planning before work starts",
		handler: async (args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const input = Array.isArray(args) ? args.join(" ") : String(args ?? "");
			pi.sendUserMessage(buildImplementationPlanReviewPrompt(input), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-review-result", {
		description: "Review implemented work, findings, and completion evidence",
		handler: async (args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const input = Array.isArray(args) ? args.join(" ") : String(args ?? "");
			pi.sendUserMessage(buildPostImplementationReviewPrompt(input), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-start", {
		description: "Ask the agent to work on the next ready Platypus item",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const item = latestSnapshot?.readyItems[0];
			if (!item) {
				if (ctx.hasUI) ctx.ui.notify(noReadyItemMessage(), "warning");
				return;
			}
			pi.sendUserMessage(buildStartPrompt(item), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-complete", {
		description: "Prompt the agent to complete the current direct Platypus item",
		handler: async (_args, ctx) => {
			const item = latestSnapshot?.readyItems[0];
			pi.sendUserMessage(buildCompletePrompt(item), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-doctor", {
		description: "Run Platypus diagnostics and show recovery guidance",
		handler: async (_args, ctx) => {
			const result = await runPlatypusTool(pi, ctx, "doctor_snapshot", {});
			const lines = renderToolResultLines(result, { expanded: true });
			if (ctx.hasUI) ctx.ui.notify(lines.join("\n"), result.isError ? "error" : "info");
		},
	});

	pi.registerCommand("platy-hide", {
		description: "Hide the Platypus backlog widget",
		handler: async (_args, ctx) => {
			widgetVisible = false;
			updateUi(ctx, latestSnapshot, widgetVisible);
		},
	});

	pi.registerCommand("platy-show", {
		description: "Show the Platypus backlog widget",
		handler: async (_args, ctx) => {
			widgetVisible = true;
			if (!latestSnapshot) await refreshSnapshot(ctx);
			updateUi(ctx, latestSnapshot, widgetVisible);
		},
	});

	pi.registerShortcut("ctrl+shift+b", {
		description: "Toggle Platypus backlog widget",
		handler: async (ctx) => {
			widgetVisible = !widgetVisible;
			if (widgetVisible && !latestSnapshot) await refreshSnapshot(ctx);
			updateUi(ctx, latestSnapshot, widgetVisible);
		},
	});

	pi.registerShortcut("ctrl+shift+n", {
		description: "Start next Platypus ready item",
		handler: async (ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const item = latestSnapshot?.readyItems[0];
			if (!item) {
				if (ctx.hasUI) ctx.ui.notify(noReadyItemMessage(), "warning");
				return;
			}
			pi.sendUserMessage(buildShortcutStartPrompt(item), ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerShortcut("ctrl+shift+r", {
		description: "Refresh Platypus backlog status",
		handler: async (ctx) => {
			await refreshSnapshot(ctx, true);
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		if (ctx.hasUI) {
			ctx.ui.setStatus(STATUS_KEY, ctx.ui.theme.fg("dim", "platypus: loading"));
			ctx.ui.addAutocompleteProvider((current) => ({
				async getSuggestions(lines, line, col, options) {
					const beforeCursor = (lines[line] ?? "").slice(0, col);
					const match = beforeCursor.match(/(?:^|[\s])#?(MCP-[\w-]*)$/i);
					if (!match) return current.getSuggestions(lines, line, col, options);
					const prefix = match[1] ?? "";
					const items = [...(latestSnapshot?.readyItems ?? []), ...(latestSnapshot?.blockedItems ?? [])]
						.filter((item) => item.id.toLowerCase().startsWith(prefix.toLowerCase()))
						.map((item) => ({ value: item.id, label: item.id, description: item.title }));
					return items.length > 0 ? { prefix, items } : current.getSuggestions(lines, line, col, options);
				},
				applyCompletion(lines, line, col, item, prefix) {
					return current.applyCompletion(lines, line, col, item, prefix);
				},
				shouldTriggerFileCompletion(lines, line, col) {
					return current.shouldTriggerFileCompletion?.(lines, line, col) ?? true;
				},
			}));
		}
		void refreshSnapshot(ctx).catch((error: unknown) => {
			const snapshot: PlatypusSnapshot = {
				ready: 0,
				blocked: 0,
				active: 0,
				status: "error",
				readyItems: [],
				blockedItems: [],
				updatedAt: Date.now(),
				error: error instanceof Error ? error.message : String(error),
			};
			applySnapshot(ctx, snapshot);
		});
	});

	pi.on("session_shutdown", async (_event, ctx) => {
		if (!ctx.hasUI) return;
		ctx.ui.setStatus(STATUS_KEY, undefined);
		ctx.ui.setWidget(WIDGET_KEY, undefined);
	});

	pi.on("tool_result", async (event, ctx) => {
		if (!event.toolName.startsWith("platypus_")) return;
		const snapshot = snapshotFromDetails(event.details);
		if (snapshot) applySnapshot(ctx, snapshot);
		if (["platypus_complete_backlog_item", "platypus_finish_work", "platypus_integrate_worker_result"].includes(event.toolName)) {
			void refreshSnapshot(ctx).catch(() => undefined);
		}
	});

	pi.on("before_agent_start", async (event, ctx) => {
		const base = `${event.systemPrompt}

## Platypus MCP Integration

This pi session has first-class Platypus project-management tools registered with the \`platypus_\` prefix. Each tool forwards to the Platypus MCP server with \`PLATYPUS_MCP_ROOT\` fixed to the current pi working directory.

Workflow guidance:
- For fresh project context, call \`platypus_inspect_session\` before planning or executing project-management work.
- If broad inspection is unavailable or insufficient, fall back to \`platypus_doctor_snapshot\`, \`platypus_inspect_status\`, \`platypus_inspect_workflow_config\`, \`platypus_inspect_queue_status\`, and \`platypus_inspect_work_queue\`.
- Prefer direct work completion with \`platypus_complete_backlog_item\` after implementation and verification.
- Call \`platypus_prepare_work\` only when response-local guidance is useful or worker handoff is needed.
- Mutating Platypus tools must be easy to identify in the response. Summarize the state transition and verification or recovery action after calling them.
- Do not expose hidden chain-of-thought. It is fine to expose safe status, tool calls, events, evidence, and summaries.
`;

		if (!shouldShowGuidance(event.prompt)) return { systemPrompt: base };
		const projectDirection = ctx?.cwd ? readProjectDirectionSummary(ctx.cwd).text : undefined;
		const dynamicGuidance = [guidanceForSnapshot(latestSnapshot), projectDirection].filter(Boolean).join("\n\n");
		if (!dynamicGuidance || dynamicGuidance === lastInjectedPrompt) return { systemPrompt: base };
		lastInjectedPrompt = dynamicGuidance;
		return { systemPrompt: `${base}\n${dynamicGuidance}\n` };
	});
}
