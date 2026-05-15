export const PI_COMMAND_NAMES = [
	"platy",
	"platypus-status",
	"platy-refresh",
	"platy-ready",
	"platy-next",
	"platy-plan",
	"platy-start",
	"platy-complete",
	"platy-doctor",
	"platy-hide",
	"platy-show",
];

export function buildPlanPrompt() {
	return [
		"Shape the current project goal into concrete Platypus backlog items.",
		"First call platypus_inspect_session and inspect repository context if needed.",
		"Then call platypus_create_backlog_items with concrete titles, goals, acceptance criteria, owned_surfaces, execution_path, and planning_gate.",
		"Present the created items and the next ready item. Ask for missing product direction instead of inventing details.",
	].join(" ");
}

export function buildStartPrompt(item) {
	return [
		`Work on Platypus backlog item ${item.id}: ${item.title}.`,
		"Call platypus_get_backlog_item or platypus_inspect_work_queue if you need the acceptance criteria.",
		"Make only the necessary project changes, run verification, then call platypus_complete_backlog_item with item_id, summary, changed_files, verification_status, verification_summary, and verification_refs.",
	].join(" ");
}

export function buildShortcutStartPrompt(item) {
	return `Work on Platypus backlog item ${item.id}: ${item.title}. Complete it with platypus_complete_backlog_item after verification.`;
}

export function buildCompletePrompt(item) {
	const target = item ? `${item.id} (${item.title})` : "the current direct-ready Platypus item";
	return `If implementation and verification are complete, call platypus_complete_backlog_item for ${target} with item_id, summary, changed_files, verification_status, verification_summary, and verification_refs. If anything is missing, explain what remains first.`;
}

export function noReadyItemMessage() {
	return "No ready Platypus item to start. Use /platy-plan to create work or /platy-doctor to inspect recovery guidance.";
}
