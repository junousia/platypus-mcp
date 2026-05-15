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
	readProjectDirectionSummary,
} from "../../extensions/platypus/direction.mjs";
import {
	commandForTool,
	formatResult,
	missingBinaryGuidance,
	parseTimeout,
	runPlatypusTool,
} from "../../extensions/platypus/runtime.mjs";

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
assert.match(buildShortcutStartPrompt(item), /after verification/);
assert.match(buildCompletePrompt(item), /MCP-127 \(Add Pi extension test harness\)/);
assert.match(buildCompletePrompt(undefined), /current direct-ready Platypus item/);
assert.match(noReadyItemMessage(), /No ready Platypus item/);
assert.match(buildDirectionPrompt(), /product domain/);
assert.match(buildDirectionPrompt(), /Do not keep direction only in chat/);
assert.match(buildDirectionPrompt({ revision: true }), /Review and revise/);

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
	assert.deepEqual(partialDirection.missing, ["docs/architecture.md", "docs/testing.md"]);
	writeFileSync(join(directionRoot, "docs", "architecture.md"), "# Architecture Direction\n\n## Stack\n- Rust MCP server\n");
	writeFileSync(join(directionRoot, "docs", "testing.md"), "# Testing Direction\n\n## Verification\n- make check\n");
	const completeDirection = readProjectDirectionSummary(directionRoot);
	assert.equal(completeDirection.status, "ready");
	assert.match(completeDirection.text, /docs\/architecture\.md/);
	assert.match(completeDirection.text, /make check/);
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
	"prepare_work",
	"complete_backlog_item",
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
