function asObject(value) {
	return value && typeof value === "object" && !Array.isArray(value) ? value : undefined;
}

function asArray(value) {
	return Array.isArray(value) ? value : [];
}

function asString(value) {
	return typeof value === "string" && value.length > 0 ? value : undefined;
}

function asNumber(value) {
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function itemFromQueueEntry(entry) {
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

export function snapshotFromDetails(details) {
	const envelope = asObject(details);
	if (!envelope) return undefined;
	const data = asObject(envelope.data) ?? envelope;
	const queue = asObject(data.queue);
	const status = asObject(data.status);
	const inventory = asObject(queue?.inventory);
	if (!queue && !status) return undefined;
	const items = asArray(queue?.items).map((item) => asObject(item)).filter(Boolean);
	const readyItems = items.map(itemFromQueueEntry).filter(Boolean);
	const blockedItems = asArray(inventory?.dependency_blocked_items)
		.map((item) => asObject(item))
		.filter(Boolean)
		.map((item) => itemFromQueueEntry(item))
		.filter(Boolean);

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

function formatRelativeTime(timestamp, now = Date.now()) {
	if (!timestamp) return "not refreshed";
	const seconds = Math.max(0, Math.round((now - timestamp) / 1000));
	if (seconds < 5) return "just now";
	if (seconds < 60) return `${seconds}s ago`;
	const minutes = Math.round(seconds / 60);
	return `${minutes}m ago`;
}

export function compactStatus(snapshot) {
	if (!snapshot) return "platypus: not inspected";
	if (snapshot.error) return "platypus: error";
	return `platypus: ${snapshot.ready} ready · ${snapshot.blocked} blocked · ${snapshot.active} active`;
}

function nextItemLine(snapshot) {
	const item = snapshot.readyItems[0];
	if (!item) return snapshot.nextAction;
	const priority = item.priority ? ` ${item.priority}` : "";
	const state = item.state ? ` ${item.state}` : "";
	return `${item.id}${priority}${state} — ${item.title}`;
}

export function renderDashboardLines(snapshot, options = {}) {
	const { expanded = false, now = Date.now() } = options;
	if (!snapshot) return ["Platypus backlog has not been inspected yet. Use /platy-refresh."];
	if (snapshot.error) {
		return [
			`Platypus unavailable: ${snapshot.error}`,
			"Try /platy-refresh or inspect the Platypus doctor output.",
		];
	}

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
	lines.push(`Updated: ${formatRelativeTime(snapshot.updatedAt, now)}`);

	if (expanded && snapshot.readyItems.length > 1) {
		lines.push("", "Ready items:");
		for (const item of snapshot.readyItems.slice(1, 10)) {
			lines.push(`  ${item.id} ${item.priority ?? ""} ${item.state ?? ""} — ${item.title}`.replace(/\s+/g, " "));
		}
	}

	return lines;
}

function statusGlyph(status) {
	if (status === "completed" || status === "ok") return "✓";
	if (status === "skipped") return "○";
	if (status === "failed" || status === "error") return "!";
	return "•";
}

function titleFromAction(action) {
	return String(action ?? "platypus")
		.replace(/^platypus_/, "")
		.replace(/_/g, " ");
}

function contentText(result) {
	return result?.content?.map((part) => part?.text).filter(Boolean).join("\n");
}

function renderCompletionLines(data) {
	const compact = asObject(data.compact);
	const lines = [`✓ ${asString(compact?.status_line) ?? asString(data.summary) ?? "backlog item completed"}`];
	const evidence = asArray(compact?.generated_evidence ?? data.generated_evidence).map((item) => asObject(item)).filter(Boolean);
	if (evidence.length > 0) {
		lines.push(`Evidence: ${evidence.map((item) => asString(item.id)).filter(Boolean).join(", ")}`);
	}
	const next = asString(compact?.next_ready_item_id);
	if (next) lines.push(`Next ready: ${next}`);
	return lines;
}

function renderBacklogCreationLines(data, envelope) {
	const created = asArray(data.created_items ?? data.items ?? data.created).map((item) => asObject(item)).filter(Boolean);
	const lines = [`✓ ${asString(envelope.summary) ?? asString(data.summary) ?? "backlog items created"}`];
	for (const item of created.slice(0, 8)) {
		const id = asString(item.id) ?? asString(item.item_id);
		const title = asString(item.title) ?? asString(item.path);
		if (id || title) lines.push(`  • ${[id, title].filter(Boolean).join(" — ")}`);
	}
	if (created.length > 8) lines.push(`  + ${created.length - 8} more item(s)`);
	return lines;
}

function renderErrorLines(envelope, fallbackText) {
	const data = asObject(envelope.data) ?? {};
	const action = asString(envelope.action) ?? asString(data.action);
	const reason = asString(envelope.summary) ?? asString(data.reason) ?? fallbackText ?? "Platypus request failed.";
	const next = asString(envelope.next_action) ?? asString(data.next_action);
	const lines = [`! ${titleFromAction(action)} failed`, reason];
	if (next) lines.push(`Next: ${next}`);
	return lines;
}

function renderEnvelopeLines(envelope, fallbackText) {
	const data = asObject(envelope.data) ?? {};
	const action = asString(envelope.action) ?? asString(data.action);
	const status = asString(envelope.status) ?? asString(data.status);
	if (status === "failed" || status === "error") return renderErrorLines(envelope, fallbackText);
	if (action === "complete_backlog_item") return renderCompletionLines(data);
	if (action === "create_backlog_item" || action === "create_backlog_items") return renderBacklogCreationLines(data, envelope);

	const summary = asString(envelope.summary) ?? asString(data.summary) ?? fallbackText ?? "Platypus tool completed.";
	const lines = [`${statusGlyph(status)} ${titleFromAction(action)}`, summary];
	const next = asString(envelope.next_action) ?? asString(data.next_action);
	if (next) lines.push(`Next: ${next}`);
	return lines;
}

export function renderToolResultLines(result, options = {}) {
	const snapshot = snapshotFromDetails(result?.details);
	if (snapshot) return renderDashboardLines(snapshot, options);

	const envelope = asObject(result?.details);
	const fallbackText = contentText(result);
	if (envelope) return renderEnvelopeLines(envelope, fallbackText);
	return [fallbackText ?? "Platypus tool completed."];
}

export function shouldShowGuidance(prompt) {
	return /\b(backlog|platypus|platy|what\s+next|next\s+item|queue|status|complete|completion)\b/i.test(prompt);
}
