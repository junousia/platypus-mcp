export const PI_COMMAND_NAMES = [
	"platy",
	"platypus-status",
	"platy-refresh",
	"platy-ready",
	"platy-next",
	"platy-plan",
	"platy-steer",
	"platy-direction",
	"platy-direction-revise",
	"platy-standards",
	"platy-standards-revise",
	"platy-story-review",
	"platy-plan-review",
	"platy-review-result",
	"platy-start",
	"platy-complete",
	"platy-doctor",
	"platy-hide",
	"platy-show",
];

export function buildPlanPrompt() {
	return [
		"Start a Platypus project intake for the current planning request.",
		"First call platypus_inspect_session, platypus_inspect_workflow_config, and inspect repository files, Git/scaffold state, existing docs, durable project direction, and engineering standards.",
		"Review key Platypus defaults with the user: merge_style, require_clean_manager_workspace, require_verification_evidence, auto_commit_artifacts_default, default_path, direct_planning_gate, and worker_planning_gate.",
		"Before any mutation, summarize sections named: What I understood, Known facts, Open questions, Assumptions, Proposed first milestone, Proposed backlog shape, and Things I will not do yet.",
		"Ask clarifying questions and wait for explicit user approval before calling platypus_create_backlog_items or writing durable planning artifacts.",
		"Do not start implementation, edit project files, commit, or call completion tools from this planning prompt.",
	].join(" ");
}

export function buildStartPrompt(item) {
	return [
		`Work on Platypus backlog item ${item.id}: ${item.title}.`,
		"Call platypus_get_backlog_item or platypus_inspect_work_queue if you need the acceptance criteria.",
		"Inspect docs/engineering.md if present and follow its implementation structure, verification expectations, and definition of done.",
		"Make only the necessary project changes, run verification, then call platypus_complete_backlog_item with item_id, summary, changed_files, verification_status, verification_summary, and verification_refs.",
	].join(" ");
}

export function buildShortcutStartPrompt(item) {
	return `Work on Platypus backlog item ${item.id}: ${item.title}. Complete it with platypus_complete_backlog_item after verification.`;
}

export function buildCompletePrompt(item) {
	const target = item ? `${item.id} (${item.title})` : "the current direct-ready Platypus item";
	return `If implementation and verification are complete and the project-specific definition of done in docs/engineering.md has been satisfied or explicitly addressed, call platypus_complete_backlog_item for ${target} with item_id, summary, changed_files, verification_status, verification_summary, and verification_refs. If anything is missing, explain what remains first.`;
}

export function noReadyItemMessage() {
	return "No ready Platypus item to start. Use /platy-plan to create work or /platy-doctor to inspect recovery guidance.";
}
