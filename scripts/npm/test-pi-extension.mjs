#!/usr/bin/env node
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
	PI_COMMAND_NAMES,
	buildCompletePrompt,
	buildPlanPrompt,
	buildShortcutStartPrompt,
	buildStartPrompt,
	noReadyItemMessage,
} from "../../extensions/platypus/commands.mjs";
import {
	PROJECT_DIRECTION_FILES,
	buildDirectionPrompt,
	buildEngineeringStandardsPrompt,
	readProjectDirectionSummary,
} from "../../extensions/platypus/direction.mjs";
import {
	commandForTool,
	formatResult,
	missingBinaryGuidance,
	parseTimeout,
	runPlatypusTool,
} from "../../extensions/platypus/runtime.mjs";
import {
	buildImplementationPlanReviewPrompt,
	buildPostImplementationReviewPrompt,
	buildStoryReviewPrompt,
	formatImplementationResultReview,
	formatStoryReview,
	implementationPlanReviewExpectation,
	reviewImplementationResult,
	reviewStoryDraft,
} from "../../extensions/platypus/review.mjs";

const packageRoot = process.cwd();
const projectRoot = "/tmp/pi-project";

const formattedJson = formatResult(
	"inspect_status",
	JSON.stringify({ action: "inspect_status", status: "completed", summary: "ok" }),
	"",
);
assert.equal(formattedJson.details.action, "inspect_status");
assert.match(formattedJson.text, /inspect_status/);

const formattedText = formatResult("inspect_status", "plain output", "stderr text");
assert.equal(formattedText.details.raw_stdout, "plain output");
assert.equal(formattedText.details.stderr, "stderr text");

assert.equal(parseTimeout(undefined), 120_000);
assert.equal(parseTimeout("2500"), 2500);
assert.equal(parseTimeout("invalid"), 120_000);

const command = commandForTool(
	projectRoot,
	packageRoot,
	"inspect_status",
	{ root: "/must/not/forward", limit: 3 },
	{ env: { PLATYPUS_MCP_BIN: "/opt/platypus-mcp" } },
);
assert.equal(command.command, "/opt/platypus-mcp");
assert.deepEqual(command.args.slice(0, 4), ["tool", "--root", projectRoot, "inspect_status"]);
assert.deepEqual(JSON.parse(command.args[4]), { limit: 3 });

const calls = [];
const fakePi = {
	async exec(commandName, args, options) {
		calls.push({ commandName, args, options });
		return {
			code: 0,
			stdout: JSON.stringify({ action: "inspect_status", status: "completed", summary: "ready" }),
			stderr: "",
		};
	},
};
const ctx = { cwd: projectRoot, signal: "signal-token" };
const success = await runPlatypusTool(
	fakePi,
	ctx,
	"inspect_status",
	{ root: "/ignore-me", limit: 5 },
	{ packageRoot, env: { PLATYPUS_MCP_BIN: "/opt/platypus-mcp" }, timeoutMs: "1234" },
);
assert.equal(success.isError, undefined);
assert.equal(success.details.summary, "ready");
assert.equal(calls[0].commandName, "/opt/platypus-mcp");
assert.equal(calls[0].options.timeout, 1234);
assert.equal(calls[0].options.signal, "signal-token");
assert.deepEqual(JSON.parse(calls[0].args.at(-1)), { limit: 5 });

const failed = await runPlatypusTool(
	{
		async exec() {
			return {
				code: 7,
				stdout: JSON.stringify({ action: "doctor_snapshot", status: "completed", summary: "needs setup" }),
				stderr: "diagnostic stderr",
			};
		},
	},
	ctx,
	"doctor_snapshot",
	{},
	{ packageRoot, env: { PLATYPUS_MCP_BIN: "/opt/platypus-mcp" } },
);
assert.equal(failed.isError, true);
assert.equal(failed.details.status, "failed");
assert.equal(failed.details.exit_code, 7);
assert.match(failed.content[0].text, /failed with exit code 7/);
assert.equal(failed.details.stderr, "diagnostic stderr");

const scratchPackageRoot = mkdtempSync(join(tmpdir(), "platypus-pi-runtime-"));
try {
	const missing = await runPlatypusTool(
		{
			async exec() {
				throw new Error("ENOENT");
			},
		},
		ctx,
		"inspect_status",
		{},
		{ packageRoot: scratchPackageRoot, env: {}, exists: () => false },
	);
	assert.equal(missing.isError, true);
	assert.equal(missing.details.command, "platypus-mcp");
	assert.match(missing.content[0].text, /Tried binary resolution order/);
	assert.match(missing.content[0].text, /cargo install platypus-mcp/);
} finally {
	rmSync(scratchPackageRoot, { recursive: true, force: true });
}

assert.match(missingBinaryGuidance("platypus-mcp"), /PLATYPUS_MCP_BIN/);

const item = { id: "MCP-127", title: "Add Pi extension test harness" };
assert.match(buildPlanPrompt(), /platypus_create_backlog_items/);
assert.match(buildStartPrompt(item), /MCP-127: Add Pi extension test harness/);
assert.match(buildStartPrompt(item), /platypus_complete_backlog_item/);
assert.match(buildStartPrompt(item), /docs\/engineering\.md/);
assert.match(buildShortcutStartPrompt(item), /after verification/);
assert.match(buildCompletePrompt(item), /MCP-127 \(Add Pi extension test harness\)/);
assert.match(buildCompletePrompt(item), /definition of done/);
assert.match(buildCompletePrompt(undefined), /current direct-ready Platypus item/);
assert.match(noReadyItemMessage(), /No ready Platypus item/);
assert.match(buildDirectionPrompt(), /product domain/);
assert.match(buildDirectionPrompt(), /Do not keep direction only in chat/);
assert.match(buildDirectionPrompt({ revision: true }), /Review and revise/);
assert.match(buildEngineeringStandardsPrompt(), /module boundaries/);
assert.match(buildEngineeringStandardsPrompt(), /docs\/engineering\.md/);
assert.match(buildEngineeringStandardsPrompt({ revision: true }), /Review and revise/);
assert.match(buildStoryReviewPrompt("MCP-132"), /platypus_get_backlog_item/);
assert.match(buildStoryReviewPrompt("draft a better auth story"), /Blocking issues/);
assert.match(buildStoryReviewPrompt(), /Ask the user/);
assert.match(buildImplementationPlanReviewPrompt(), /next ready Platypus item/);
assert.match(buildImplementationPlanReviewPrompt("MCP-133"), /platypus_get_backlog_item/);
assert.match(buildImplementationPlanReviewPrompt("MCP-133"), /platypus_write_task_plan/);
const directPlan = implementationPlanReviewExpectation({ execution_path: "direct_edit", planning_gate: "none" });
assert.equal(directPlan.mode, "direct_response_local");
assert.equal(directPlan.durable_task_plan_required, false);
const durablePlan = implementationPlanReviewExpectation({ execution_path: "worker_handoff", planning_gate: "task_plan" });
assert.equal(durablePlan.mode, "durable_task_plan");
assert.equal(durablePlan.durable_task_plan_required, true);
assert.match(buildPostImplementationReviewPrompt("MCP-135"), /platypus_get_backlog_item/);
assert.match(buildPostImplementationReviewPrompt("MCP-135"), /platypus_list_findings/);
assert.match(buildPostImplementationReviewPrompt("MCP-135"), /platypus_complete_backlog_item/);
assert.match(buildPostImplementationReviewPrompt("task 123"), /platypus_finish_work/);
const cleanResultReview = reviewImplementationResult({
	changed_files: ["extensions/platypus/review.mjs"],
	verification_status: "passed",
	verification_refs: ["npm test"],
	findings_reviewed: true,
});
assert.equal(cleanResultReview.status, "ready_to_complete");
assert.equal(cleanResultReview.completion_tool, "platypus_complete_backlog_item");
assert.match(formatImplementationResultReview(cleanResultReview), /Ready to complete/);
const findingResultReview = reviewImplementationResult({
	changed_files: ["src/lib.rs"],
	verification_status: "passed",
	verification_refs: ["cargo test"],
	findings: [{ title: "Follow-up" }],
});
assert.equal(findingResultReview.status, "needs_action");
assert.match(formatImplementationResultReview(findingResultReview), /platypus_record_finding/);
const followUpResultReview = reviewImplementationResult({
	execution_path: "worker_handoff",
	changed_files: ["src/lib.rs"],
	verification_status: "passed",
	verification_refs: ["cargo test"],
	findings_reviewed: true,
	follow_up_items: [{ title: "Improve docs" }],
});
assert.equal(followUpResultReview.completion_tool, "platypus_finish_work");
assert.match(formatImplementationResultReview(followUpResultReview), /platypus_create_backlog_items/);

const vagueReview = reviewStoryDraft({ title: "Auth", goal: "Do auth" });
assert.equal(vagueReview.status, "blocked");
assert.match(formatStoryReview(vagueReview), /Add testable acceptance criteria/);
const acceptableReview = reviewStoryDraft({
	title: "Add login form",
	goal: "Allow registered users to sign in with email and password.",
	acceptance: ["Successful login reaches the dashboard."],
	owned_surfaces: ["src/auth"],
	depends_on: [],
	execution_path: "direct_edit",
	planning_gate: "none",
	expected_evidence: ["make check"],
});
assert.equal(acceptableReview.status, "reviewable");
assert.deepEqual(acceptableReview.blocking, []);
const dependencyBlockedReview = reviewStoryDraft({
	title: "Add user settings",
	goal: "Let signed-in users edit notification preferences.",
	acceptance: ["Settings persist after reload."],
	owned_surfaces: ["src/settings"],
	depends_on: ["MCP-001"],
	open_dependencies: ["MCP-001"],
	expected_evidence: ["make check"],
});
assert.equal(dependencyBlockedReview.status, "blocked");
assert.match(formatStoryReview(dependencyBlockedReview), /MCP-001/);
const broadReview = reviewStoryDraft({
	title: "Build complete platform",
	goal: "Implement the entire customer portal and all admin workflows.",
	acceptance: ["Portal and admin flows work."],
	owned_surfaces: ["frontend", "backend", "infra", "auth", "billing", "admin"],
	depends_on: [],
	expected_evidence: ["make check"],
});
assert.equal(broadReview.status, "reviewable");
assert.match(formatStoryReview(broadReview), /Consider splitting/);

const directionRoot = mkdtempSync(join(tmpdir(), "platypus-pi-direction-"));
try {
	const missingDirection = readProjectDirectionSummary(directionRoot);
	assert.equal(missingDirection.status, "missing");
	assert.deepEqual(missingDirection.missing, PROJECT_DIRECTION_FILES);
	mkdirSync(join(directionRoot, "docs"), { recursive: true });
	writeFileSync(join(directionRoot, "docs", "product.md"), "# Product Direction\n\n## Product Goal\nBuild durable project tooling.\n\n## Users\n- Maintainers\n");
	let partialDirection = readProjectDirectionSummary(directionRoot);
	assert.equal(partialDirection.status, "partial");
	assert.match(partialDirection.text, /durable project tooling/);
	assert.deepEqual(partialDirection.missing, ["docs/architecture.md", "docs/testing.md", "docs/engineering.md"]);
	writeFileSync(join(directionRoot, "docs", "architecture.md"), "# Architecture Direction\n\n## Stack\n- Rust MCP server\n");
	writeFileSync(join(directionRoot, "docs", "testing.md"), "# Testing Direction\n\n## Verification\n- make check\n");
	writeFileSync(join(directionRoot, "docs", "engineering.md"), "# Engineering Standards\n\n## Definition Of Done\n- Implementation is verified\n");
	const completeDirection = readProjectDirectionSummary(directionRoot);
	assert.equal(completeDirection.status, "ready");
	assert.match(completeDirection.text, /docs\/architecture\.md/);
	assert.match(completeDirection.text, /make check/);
	assert.match(completeDirection.text, /Implementation is verified/);
} finally {
	rmSync(directionRoot, { recursive: true, force: true });
}

const extensionSource = readFileSync("extensions/platypus/index.ts", "utf8");
for (const commandName of PI_COMMAND_NAMES) {
	assert.match(extensionSource, new RegExp(`registerCommand\\("${commandName}"`));
}
for (const toolName of [
	"inspect_session",
	"inspect_status",
	"inspect_work_queue",
	"create_backlog_items",
	"update_backlog_item",
	"write_task_plan",
	"validate_task_plan",
	"inspect_task_plan",
	"list_task_plans",
	"prepare_work",
	"complete_backlog_item",
	"record_finding",
	"doctor_snapshot",
]) {
	assert.match(extensionSource, new RegExp(`name:\\s*"${toolName}"`));
	assert.match(extensionSource, new RegExp(`${toolName}:\\s*(?:Type\\.|noArgs\\()`));
}
assert.match(extensionSource, /registerTool\(\{/);
assert.match(extensionSource, /name:\s*"platypus_call_tool"/);
assert.match(extensionSource, /promptSnippet/);
assert.match(extensionSource, /addAutocompleteProvider/);

console.log("pi extension harness ok");
