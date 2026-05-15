export const STORY_REVIEW_SECTIONS = [
	"Blocking issues",
	"Improvement suggestions",
	"Approved revision",
];

function hasText(value) {
	return typeof value === "string" && value.trim().length > 0 && !/\bTBD\b|not specified/i.test(value);
}

function asArray(value) {
	return Array.isArray(value) ? value : [];
}

export function reviewStoryDraft(draft) {
	const blocking = [];
	const suggestions = [];
	const goal = draft?.goal ?? "";
	const title = draft?.title ?? "";
	const acceptance = asArray(draft?.acceptance);
	const ownedSurfaces = asArray(draft?.owned_surfaces);
	const dependsOn = draft?.depends_on;
	const openDependencies = asArray(draft?.open_dependencies);

	if (!hasText(title)) blocking.push("Add a concrete title.");
	if (!hasText(goal) || goal.trim().length < 16) blocking.push("Clarify the goal with a concrete outcome.");
	if (acceptance.length === 0 || acceptance.some((item) => !hasText(item))) {
		blocking.push("Add testable acceptance criteria.");
	}
	if (ownedSurfaces.length === 0 || ownedSurfaces.some((item) => !hasText(item))) {
		blocking.push("State clear owned surfaces.");
	}
	if (!Array.isArray(dependsOn)) blocking.push("State dependencies explicitly, even when the list is empty.");
	if (openDependencies.length > 0) blocking.push(`Resolve or explain open dependencies: ${openDependencies.join(", ")}.`);

	if (!hasText(draft?.execution_path)) suggestions.push("Choose execution_path once the implementation mode is clear.");
	if (!hasText(draft?.planning_gate)) suggestions.push("Choose planning_gate based on complexity and review needs.");
	if (asArray(draft?.expected_evidence).length === 0) suggestions.push("State expected completion evidence.");
	if (/\b(all|entire|complete|full|everything)\b/i.test(`${title} ${goal}`) || ownedSurfaces.length > 5) {
		suggestions.push("Consider splitting this broad story into smaller independently reviewable items.");
	}

	return {
		status: blocking.length > 0 ? "blocked" : "reviewable",
		blocking,
		suggestions,
	};
}

export function formatStoryReview(result) {
	const blocking = result.blocking.length > 0 ? result.blocking : ["No blocking issues found."];
	const suggestions = result.suggestions.length > 0 ? result.suggestions : ["No improvement suggestions found."];
	return [
		`Story review: ${result.status}`,
		"Blocking issues:",
		...blocking.map((item) => `- ${item}`),
		"Improvement suggestions:",
		...suggestions.map((item) => `- ${item}`),
	].join("\n");
}

export function buildStoryReviewPrompt(input = "") {
	const trimmed = String(input ?? "").trim();
	const subject = trimmed.length > 0
		? `Review this story or backlog item reference: ${trimmed}`
		: "Ask the user for the story draft or backlog item id to review.";
	return [
		subject,
		"First call platypus_inspect_session and inspect current project direction and engineering standards.",
		"If the input is an existing item id, call platypus_get_backlog_item before reviewing it.",
		"Check concrete goal, testable acceptance criteria, clear owned surfaces, explicit dependencies, appropriate planning mode, execution path, and expected completion evidence.",
		"Present separate sections named Blocking issues and Improvement suggestions. Do not silently rewrite user intent.",
		"For approved revisions, use platypus_create_backlog_items with preview=true before creating new work, or platypus_update_backlog_item for existing items.",
	].join(" ");
}
