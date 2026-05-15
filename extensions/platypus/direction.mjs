import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const PROJECT_DIRECTION_FILES = [
	"docs/product.md",
	"docs/architecture.md",
	"docs/testing.md",
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
