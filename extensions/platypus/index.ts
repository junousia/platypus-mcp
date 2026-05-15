import { existsSync } from "node:fs";
import { resolve } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Text, truncateToWidth } from "@earendil-works/pi-tui";
import { Type } from "typebox";

type JsonObject = Record<string, unknown>;

type PlatypusTool = {
	name: string;
	description: string;
	promptSnippet?: string;
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
const DEFAULT_TIMEOUT_MS = 120_000;
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

function commandForTool(cwd: string, toolName: string, params: JsonObject): { command: string; args: string[] } {
	const cleanParams = { ...params };
	delete cleanParams.root;

	const payload = JSON.stringify(cleanParams);
	const configuredBinary = process.env.PLATYPUS_MCP_BIN;
	if (configuredBinary) {
		return {
			command: configuredBinary,
			args: ["tool", "--root", cwd, toolName, payload],
		};
	}

	const manifestPath = resolve(PACKAGE_ROOT, "Cargo.toml");
	if (existsSync(manifestPath)) {
		return {
			command: "cargo",
			args: ["run", "--manifest-path", manifestPath, "--quiet", "--", "tool", "--root", cwd, toolName, payload],
		};
	}

	return {
		command: "platypus-mcp",
		args: ["tool", "--root", cwd, toolName, payload],
	};
}

function parseTimeout(): number {
	const configured = process.env.PLATYPUS_PI_TIMEOUT_MS;
	if (!configured) return DEFAULT_TIMEOUT_MS;
	const parsed = Number(configured);
	return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_TIMEOUT_MS;
}

function formatResult(toolName: string, stdout: string, stderr: string): { text: string; details: JsonObject } {
	const trimmed = stdout.trim();
	let details: JsonObject = {};
	if (trimmed) {
		try {
			const parsed = JSON.parse(trimmed);
			if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
				details = parsed as JsonObject;
			}
		} catch {
			details = { raw_stdout: trimmed };
		}
	}
	if (stderr.trim()) {
		details = { ...details, stderr: stderr.trim() };
	}

	return {
		text: trimmed || `Platypus tool ${toolName} completed with no stdout.`,
		details,
	};
}

async function runPlatypusTool(pi: ExtensionAPI, ctx: ExtensionContext, toolName: string, params: JsonObject) {
	const { command, args } = commandForTool(ctx.cwd, toolName, params);
	const result = await pi.exec(command, args, {
		signal: ctx.signal,
		timeout: parseTimeout(),
	});

	const formatted = formatResult(toolName, result.stdout ?? "", result.stderr ?? "");
	if (result.code !== 0) {
		return {
			content: [
				{
					type: "text" as const,
					text: `Platypus tool ${toolName} failed with exit code ${result.code}.\n\n${formatted.text}`,
				},
			],
			details: {
				...formatted.details,
				status: "failed",
				exit_code: result.code,
				command,
			},
			isError: true,
		};
	}

	return {
		content: [{ type: "text" as const, text: formatted.text }],
		details: formatted.details,
	};
}

function asObject(value: unknown): JsonObject | undefined {
	return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonObject) : undefined;
}

function asArray(value: unknown): unknown[] {
	return Array.isArray(value) ? value : [];
}

function asString(value: unknown): string | undefined {
	return typeof value === "string" && value.length > 0 ? value : undefined;
}

function asNumber(value: unknown): number | undefined {
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function itemFromQueueEntry(entry: JsonObject): ReadyItem | undefined {
	const candidate = asObject(entry.candidate) ?? entry;
	const id = asString(candidate.item_id) ?? asString(entry.item_id);
	if (!id) return undefined;
	return {
		id,
		title: asString(candidate.title) ?? asString(entry.title) ?? "Untitled backlog item",
		priority: asString(candidate.priority) ?? asString(entry.priority),
		state: asString(entry.queue_state) ?? asString(candidate.queue_state),
		recommendedTool: asString(entry.recommended_tool),
		reason: asString(entry.reason),
	};
}

function snapshotFromDetails(details: unknown): PlatypusSnapshot | undefined {
	const envelope = asObject(details);
	if (!envelope) return undefined;
	const data = asObject(envelope.data) ?? envelope;
	const queue = asObject(data.queue);
	const status = asObject(data.status);
	const inventory = asObject(queue?.inventory);
	const items = asArray(queue?.items).map((item) => asObject(item)).filter(Boolean) as JsonObject[];
	const readyItems = items.map(itemFromQueueEntry).filter(Boolean) as ReadyItem[];
	const blockedItems = asArray(inventory?.dependency_blocked_items)
		.map((item) => asObject(item))
		.filter(Boolean)
		.map((item) => itemFromQueueEntry(item as JsonObject))
		.filter(Boolean) as ReadyItem[];

	const ready = asNumber(queue?.ready_count) ?? asNumber(inventory?.runnable_count) ?? asNumber(status?.runnable_backlog_items) ?? readyItems.length;
	const blocked = asNumber(queue?.blocked_count) ?? asNumber(inventory?.dependency_blocked_count) ?? blockedItems.length;
	const active = asNumber(queue?.active_count) ?? asNumber(inventory?.active_lifecycle_count) ?? 0;

	return {
		ready,
		blocked,
		active,
		closed: asNumber(inventory?.closed_count),
		total: asNumber(inventory?.total_count) ?? asNumber(status?.backlog_items),
		pendingIntegration: asNumber(inventory?.pending_integration_count),
		root: asString(data.root) ?? asString(status?.root),
		status: envelope.status === "completed" || envelope.ok === true ? "ok" : envelope.status === "failed" ? "error" : "unknown",
		summary: asString(data.summary) ?? asString(envelope.summary),
		nextAction: asString(envelope.next_action) ?? asString(data.reason) ?? asString(queue?.reason),
		recommendedTool: asString(data.recommended_tool) ?? asString(queue?.recommended_tool),
		readyItems,
		blockedItems,
		updatedAt: Date.now(),
	};
}

function formatRelativeTime(timestamp?: number): string {
	if (!timestamp) return "not refreshed";
	const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
	if (seconds < 5) return "just now";
	if (seconds < 60) return `${seconds}s ago`;
	const minutes = Math.round(seconds / 60);
	return `${minutes}m ago`;
}

function compactStatus(snapshot?: PlatypusSnapshot): string {
	if (!snapshot) return "platypus: not inspected";
	if (snapshot.error) return `platypus: error`;
	return `platypus: ${snapshot.ready} ready · ${snapshot.blocked} blocked · ${snapshot.active} active`;
}

function nextItemLine(snapshot: PlatypusSnapshot): string | undefined {
	const item = snapshot.readyItems[0];
	if (!item) return snapshot.nextAction;
	const priority = item.priority ? ` ${item.priority}` : "";
	const state = item.state ? ` ${item.state}` : "";
	return `${item.id}${priority}${state} — ${item.title}`;
}

function renderPlainDashboard(snapshot?: PlatypusSnapshot, expanded = false): string {
	if (!snapshot) return "Platypus backlog has not been inspected yet. Use /platy-refresh.";
	if (snapshot.error) return `Platypus unavailable: ${snapshot.error}\nTry /platy-refresh or inspect the Platypus doctor output.`;

	const bits = [`${snapshot.ready} ready`, `${snapshot.blocked} blocked`, `${snapshot.active} active`];
	if (snapshot.pendingIntegration !== undefined) bits.push(`${snapshot.pendingIntegration} integration`);
	if (snapshot.closed !== undefined && snapshot.total !== undefined) bits.push(`${snapshot.closed}/${snapshot.total} closed`);

	const lines = [`Platypus backlog: ${bits.join(" · ")}`];
	const next = nextItemLine(snapshot);
	if (next) lines.push(`▶ ${next}`);
	if (snapshot.recommendedTool) lines.push(`Next tool: ${snapshot.recommendedTool}`);
	if (snapshot.nextAction) lines.push(`Next: ${snapshot.nextAction}`);
	if (snapshot.blockedItems.length > 0) {
		const blocked = snapshot.blockedItems.slice(0, expanded ? 8 : 4).map((item) => item.id).join(", ");
		const suffix = snapshot.blockedItems.length > (expanded ? 8 : 4) ? " …" : "";
		lines.push(`Blocked: ${blocked}${suffix}`);
	}
	lines.push(`Updated: ${formatRelativeTime(snapshot.updatedAt)}`);

	if (expanded && snapshot.readyItems.length > 1) {
		lines.push("", "Ready items:");
		for (const item of snapshot.readyItems.slice(1, 10)) {
			lines.push(`  ${item.id} ${item.priority ?? ""} ${item.state ?? ""} — ${item.title}`.replace(/\s+/g, " "));
		}
	}

	return lines.join("\n");
}

function shouldShowGuidance(prompt: string): boolean {
	return /\b(backlog|platypus|platy|what\s+next|next\s+item|queue|status|complete|completion)\b/i.test(prompt);
}

function guidanceForSnapshot(snapshot?: PlatypusSnapshot): string | undefined {
	if (!snapshot || snapshot.error) return undefined;
	const lines = ["## Current Platypus Queue Snapshot", renderPlainDashboard(snapshot, false)];
	if (snapshot.ready > 0) {
		lines.push(
			"When working a direct_ready Platypus item, use the direct loop: inspect acceptance criteria, edit the manager workspace, verify, then call platypus_complete_backlog_item with summary, changed_files, and verification_status.",
			"Call platypus_prepare_work only when response-local guidance or worker handoff is useful.",
		);
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
			const text = renderPlainDashboard(snapshot, false);
			return text.split("\n").map((line, index) => {
				const styled = index === 0 ? theme.fg("accent", line) : line.startsWith("▶") ? theme.fg("success", line) : line.startsWith("Blocked:") ? theme.fg("warning", line) : theme.fg("dim", line);
				return truncateToWidth(styled, Math.max(1, width));
			});
		},
	}));
}

function renderToolDashboard(result: { details?: unknown; content?: Array<{ text?: string }> }, theme: { fg: (color: string, text: string) => string }) {
	const snapshot = snapshotFromDetails(result.details);
	if (snapshot) {
		return new Text(renderPlainDashboard(snapshot, true).split("\n").map((line, index) => {
			if (index === 0) return theme.fg("accent", line);
			if (line.startsWith("▶")) return theme.fg("success", line);
			if (line.startsWith("Blocked:")) return theme.fg("warning", line);
			return line;
		}).join("\n"), 0, 0);
	}

	const text = result.content?.map((part) => part.text).filter(Boolean).join("\n") ?? "Platypus tool completed.";
	return new Text(text, 0, 0);
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
		if (notify && ctx.hasUI) ctx.ui.notify(renderPlainDashboard(snapshot, true), snapshot.error ? "error" : "info");
		return snapshot;
	};

	for (const tool of platypusTools) {
		pi.registerTool({
			name: `platypus_${tool.name}`,
			label: `Platypus ${tool.name}`,
			description: tool.description,
			promptSnippet: tool.promptSnippet,
			parameters: passthroughParameters,
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
			if (ctx.hasUI) ctx.ui.notify(renderPlainDashboard(latestSnapshot, true), latestSnapshot?.error ? "error" : "info");
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
				? renderPlainDashboard({ ...latestSnapshot, blockedItems: [] }, true)
				: "No runnable Platypus backlog item is currently ready.";
			if (ctx.hasUI) ctx.ui.notify(text, latestSnapshot?.readyItems[0] ? "info" : "warning");
		},
	});

	pi.registerCommand("platy-start", {
		description: "Ask the agent to work on the next ready Platypus item",
		handler: async (_args, ctx) => {
			if (!latestSnapshot) await refreshSnapshot(ctx);
			const item = latestSnapshot?.readyItems[0];
			if (!item) {
				if (ctx.hasUI) ctx.ui.notify("No ready Platypus item to start.", "warning");
				return;
			}
			const prompt = `Work on Platypus backlog item ${item.id}: ${item.title}. Inspect its acceptance criteria, make only the necessary project changes, run verification, then complete the item with platypus_complete_backlog_item.`;
			pi.sendUserMessage(prompt, ctx.isIdle() ? undefined : { deliverAs: "followUp" });
		},
	});

	pi.registerCommand("platy-complete", {
		description: "Prompt the agent to complete the current direct Platypus item",
		handler: async (_args, ctx) => {
			const item = latestSnapshot?.readyItems[0];
			const target = item ? `${item.id} (${item.title})` : "the current direct-ready Platypus item";
			pi.sendUserMessage(
				`If implementation and verification are complete, call platypus_complete_backlog_item for ${target} with a concise summary, changed_files, and verification_status. If anything is missing, explain what remains first.`,
				ctx.isIdle() ? undefined : { deliverAs: "followUp" },
			);
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
				if (ctx.hasUI) ctx.ui.notify("No ready Platypus item to start.", "warning");
				return;
			}
			pi.sendUserMessage(`Work on Platypus backlog item ${item.id}: ${item.title}.`, ctx.isIdle() ? undefined : { deliverAs: "followUp" });
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

	pi.on("before_agent_start", async (event) => {
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

		const dynamicGuidance = shouldShowGuidance(event.prompt) ? guidanceForSnapshot(latestSnapshot) : undefined;
		if (!dynamicGuidance || dynamicGuidance === lastInjectedPrompt) return { systemPrompt: base };
		lastInjectedPrompt = dynamicGuidance;
		return { systemPrompt: `${base}\n${dynamicGuidance}\n` };
	});
}
