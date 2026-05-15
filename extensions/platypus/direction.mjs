import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const PROJECT_DIRECTION_FILES = [
	"docs/product.md",
	"docs/architecture.md",
	"docs/testing.md",
	"docs/engineering.md",
];

const MAX_FILE_BYTES = 16_384;
const MAX_SUMMARY_LINES_PER_FILE = 5;

function asNonEmptyLine(line) {
	const trimmed = line.trim();
	if (!trimmed || trimmed === "---") return undefined;
	if (/^<!--/.test(trimmed)) return undefined;
	return trimmed;
}

function summarizeFile(path, content) {
	const lines = content
		.split(/\r?\n/)
		.map(asNonEmptyLine)
		.filter(Boolean)
		.filter((line) => !/^#\s+/.test(line))
		.slice(0, MAX_SUMMARY_LINES_PER_FILE);
	if (lines.length === 0) return undefined;
	return `- ${path}: ${lines.join(" / ")}`;
}

export function readProjectDirectionSummary(cwd, options = {}) {
	const exists = options.exists ?? existsSync;
	const readFile = options.readFile ?? readFileSync;
	const lines = [];
	const missing = [];
	for (const relative of PROJECT_DIRECTION_FILES) {
		const path = join(cwd, relative);
		if (!exists(path)) {
			missing.push(relative);
			continue;
		}
		const raw = readFile(path);
		const content = Buffer.isBuffer(raw) ? raw.subarray(0, MAX_FILE_BYTES).toString("utf8") : String(raw).slice(0, MAX_FILE_BYTES);
		const summary = summarizeFile(relative, content);
		if (summary) lines.push(summary);
	}
	if (lines.length === 0) {
		return {
			status: missing.length === PROJECT_DIRECTION_FILES.length ? "missing" : "empty",
			text: "No durable project direction has been captured yet.",
			missing,
		};
	}
	return {
		status: missing.length > 0 ? "partial" : "ready",
		text: ["## Durable Project Direction", ...lines].join("\n"),
		missing,
	};
}

export function buildDirectionPrompt(options = {}) {
	const revision = options.revision === true;
	const opening = revision
		? "Review and revise the durable Platypus project direction."
		: "Guide the user through durable Platypus project direction setup.";
	return [
		opening,
		"First call platypus_inspect_session and inspect existing direction files if present.",
		`Use these files as the durable source of truth: ${PROJECT_DIRECTION_FILES.join(", ")}.`,
		"Ask concise questions covering product domain, target users, stack preferences, deployment target, testing expectations, UI/UX quality bar, security/privacy concerns, performance constraints, and preferred repository structure.",
		"Write or update the direction files with the answers. Do not keep direction only in chat.",
		"After updating files, summarize the captured direction and then use it when shaping backlog items.",
	].join(" ");
}

export function buildEngineeringStandardsPrompt(options = {}) {
	const revision = options.revision === true;
	const opening = revision
		? "Review and revise the durable Platypus engineering standards."
		: "Guide the user through durable Platypus engineering standards setup.";
	return [
		opening,
		"First call platypus_inspect_session and inspect docs/engineering.md plus product, architecture, and testing direction if present.",
		"Ask concise questions covering module boundaries, code organization, test strategy, required verification commands, UI design language, commit and PR expectations, evidence expectations, and definition of done.",
		"Write or update docs/engineering.md with explicit project-specific standards. Do not hardcode a stack or keep standards only in chat.",
		"After updating standards, summarize the definition of done and use it when planning, starting, reviewing, and completing work.",
	].join(" ");
}

export function analyzeProductSteeringProposal(proposal = {}) {
	const current = String(proposal.current_direction ?? "").trim();
	const requested = String(proposal.proposed_direction ?? "").trim();
	const approved = proposal.approved === true;
	const rejected = proposal.rejected === true;
	const affectedDocs = new Set(["docs/product.md"]);
	const unresolved = [];
	const tradeoffs = [];

	if (!requested) unresolved.push("State the proposed product direction.");
	if (/stack|architecture|migrate|switch|replace|database|backend|frontend|deployment/i.test(requested)) {
		affectedDocs.add("docs/architecture.md");
	}
	if (/test|quality|verification|coverage/i.test(requested)) {
		affectedDocs.add("docs/testing.md");
		affectedDocs.add("docs/engineering.md");
	}
	if (/ui|ux|design|accessibility|workflow|developer experience/i.test(requested)) {
		affectedDocs.add("docs/engineering.md");
	}
	if (current && /\b(replace|instead|migrate|switch|drop|remove)\b/i.test(requested)) {
		tradeoffs.push("The proposed direction may conflict with existing captured direction; preserve the old context and explain the change.");
	}
	if (!approved && !rejected && requested) unresolved.push("Get user approval before writing durable guidance or backlog updates.");
	if (rejected) unresolved.push("Do not persist rejected steering changes; summarize why the proposal was rejected.");

	return {
		status: rejected ? "rejected" : unresolved.length > 0 ? "needs_decision" : "approved_to_persist",
		affected_docs: [...affectedDocs],
		affected_backlog: requested ? ["Review open and runnable backlog items for stale goals, dependencies, acceptance criteria, and owned surfaces."] : [],
		unresolved_decisions: unresolved,
		tradeoffs,
	};
}

export function buildProductSteeringPrompt(input = "") {
	const trimmed = String(input ?? "").trim();
	const subject = trimmed.length > 0
		? `Steer the Platypus project direction toward: ${trimmed}`
		: "Ask the user what product direction should change before editing durable project guidance.";
	return [
		subject,
		"First call platypus_inspect_session and platypus_inspect_work_queue, then inspect current durable direction files and docs/engineering.md.",
		"Present a concise steering proposal with sections: Current direction, Proposed direction, Affected docs, Affected backlog items, Tradeoffs, Unresolved decisions, and Exact changes to persist.",
		"Preserve old context when useful. Do not silently overwrite project intent; explain what changes, what stays, and why.",
		"Ask for approval before writing files or backlog updates. If the user rejects the proposal, summarize the rejected path and do not persist changes.",
		"After approval, update repository guidance files directly and use typed Platypus tools for backlog changes: platypus_update_backlog_item for existing items, platypus_create_backlog_items for approved follow-up work, and platypus_record_finding for unresolved implications that must stay visible.",
		"Finish by summarizing persisted docs, changed backlog items, new findings or follow-up items, and remaining decisions.",
	].join(" ");
}
