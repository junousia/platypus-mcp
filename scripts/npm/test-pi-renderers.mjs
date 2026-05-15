#!/usr/bin/env node
import assert from "node:assert/strict";
import {
	renderDashboardLines,
	renderToolResultLines,
	snapshotFromDetails,
} from "../../extensions/platypus/renderers.mjs";

const queueEnvelope = {
	status: "completed",
	data: {
		queue: {
			ready_count: 1,
			blocked_count: 2,
			active_count: 0,
			recommended_tool: "complete_backlog_item",
			inventory: {
				closed_count: 3,
				total_count: 6,
				pending_integration_count: 1,
				dependency_blocked_items: [
					{ item_id: "MCP-126", title: "Blocked workflow", priority: "P0" },
				],
			},
			items: [
				{
					queue_state: "direct_ready",
					candidate: {
						item_id: "MCP-125",
						title: "Build reusable Pi workflow renderers",
						priority: "P0",
					},
				},
			],
		},
	},
	next_action: "Work MCP-125 next.",
};

const snapshot = snapshotFromDetails(queueEnvelope);
assert.equal(snapshot.ready, 1);
assert.equal(snapshot.blocked, 2);
assert.equal(snapshot.pendingIntegration, 1);
assert.equal(snapshot.readyItems[0].id, "MCP-125");

const dashboard = renderDashboardLines({ ...snapshot, updatedAt: Date.now() }, { expanded: true, now: Date.now() }).join("\n");
assert.match(dashboard, /1 ready · 2 blocked · 0 active · 1 integration · 3\/6 closed/);
assert.match(dashboard, /MCP-125 P0 direct_ready/);
assert.match(dashboard, /Blocked: MCP-126/);

const completion = renderToolResultLines({
	details: {
		action: "complete_backlog_item",
		status: "completed",
		data: {
			compact: {
				status_line: "done MCP-125 closed; next=MCP-126",
				next_ready_item_id: "MCP-126",
				generated_evidence: [{ id: "EVD-001" }],
			},
		},
	},
});
assert.deepEqual(completion, [
	"✓ done MCP-125 closed; next=MCP-126",
	"Evidence: EVD-001",
	"Next ready: MCP-126",
]);

const skipped = renderToolResultLines({
	details: {
		action: "validate_backlog",
		status: "skipped",
		summary: "Backlog validation skipped.",
		next_action: "Run init_project first.",
	},
});
assert.deepEqual(skipped, [
	"○ validate backlog",
	"Backlog validation skipped.",
	"Next: Run init_project first.",
]);

const failed = renderToolResultLines({
	details: {
		action: "doctor_snapshot",
		status: "failed",
		summary: "Project root is not initialized.",
		next_action: "Run platypus_init_project.",
	},
});
assert.deepEqual(failed, [
	"! doctor snapshot failed",
	"Project root is not initialized.",
	"Next: Run platypus_init_project.",
]);

const empty = renderDashboardLines(undefined).join("\n");
assert.match(empty, /not been inspected/);

console.log("pi renderer fixtures ok");
