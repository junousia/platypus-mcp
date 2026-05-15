#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";

const requiredFiles = new Set([
  "package.json",
  "extensions/platypus/index.ts",
  "extensions/platypus/renderers.mjs",
]);

const requiredMetadata = {
  binaryName: "platypus-mcp",
  binaryFallbackOrder: [
    "PLATYPUS_MCP_BIN",
    "package-local bin/ or vendor/ platform binary",
    "development Cargo.toml checkout",
    "platypus-mcp on PATH",
  ],
  packageLocalBinaryLayouts: [
    "bin/{binary}",
    "bin/{platform}/{binary}",
    "vendor/{platform}/{binary}",
  ],
};

const requiredCoreTypedTools = [
  "inspect_session",
  "inspect_status",
  "inspect_work_queue",
  "inspect_queue_status",
  "init_project",
  "list_backlog",
  "validate_backlog",
  "get_backlog_item",
  "create_backlog_item",
  "create_backlog_items",
  "prepare_work",
  "complete_backlog_item",
  "finish_work",
  "record_evidence",
  "list_findings",
  "validate_findings",
  "update_finding_disposition",
  "events_replay",
  "doctor_snapshot",
  "inspect_workflow_config",
];

const forbiddenPrefixes = [
  "target/",
  ".platy/",
  ".git/",
  ".env",
  "backlog/",
  ".agents/",
];

const npmCache = process.env.npm_config_cache
  ?? process.env.NPM_CONFIG_CACHE
  ?? join(process.cwd(), ".platy", "npm-cache");
mkdirSync(npmCache, { recursive: true });

const scratchDir = join(process.cwd(), ".platy", "npm-package");
mkdirSync(scratchDir, { recursive: true });
const packOutputPath = join(scratchDir, "pack-dry-run.json");
const packOutput = openSync(packOutputPath, "w");
const result = spawnSync("npm", ["pack", "--dry-run", "--json"], {
  cwd: process.cwd(),
  encoding: "utf8",
  stdio: ["ignore", packOutput, "pipe"],
  env: {
    ...process.env,
    npm_config_cache: npmCache,
  },
});
closeSync(packOutput);

if (result.status !== 0) {
  process.stderr.write(result.stderr);
  process.exit(result.status ?? 1);
}

let packs;
try {
  packs = JSON.parse(readFileSync(packOutputPath, "utf8"));
} catch (error) {
  console.error("Could not parse npm pack --dry-run --json output.");
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
} finally {
  try {
    unlinkSync(packOutputPath);
  } catch {
    // Best-effort cleanup of validation scratch output.
  }
}

const pack = packs?.[0];
const files = Array.isArray(pack?.files) ? pack.files.map((file) => file.path) : [];
const fileSet = new Set(files);
const missing = [...requiredFiles].filter((file) => !fileSet.has(file));
const forbidden = files.filter((file) => forbiddenPrefixes.some((prefix) => file === prefix || file.startsWith(prefix)));

let metadataErrors = [];
const metadata = JSON.parse(readFileSync("package.json", "utf8")).platypusMcp;
if (!metadata || typeof metadata !== "object") {
  metadataErrors.push("package.json missing platypusMcp metadata");
} else {
  if (metadata.binaryName !== requiredMetadata.binaryName) {
    metadataErrors.push(`platypusMcp.binaryName must be ${requiredMetadata.binaryName}`);
  }
  const fallbackOrder = Array.isArray(metadata.binaryFallbackOrder) ? metadata.binaryFallbackOrder : [];
  if (JSON.stringify(fallbackOrder) !== JSON.stringify(requiredMetadata.binaryFallbackOrder)) {
    metadataErrors.push(`platypusMcp.binaryFallbackOrder must be ${requiredMetadata.binaryFallbackOrder.join(" -> ")}`);
  }
  const packageLocalBinaryLayouts = Array.isArray(metadata.packageLocalBinaryLayouts) ? metadata.packageLocalBinaryLayouts : [];
  for (const layout of requiredMetadata.packageLocalBinaryLayouts) {
    if (!packageLocalBinaryLayouts.includes(layout)) {
      metadataErrors.push(`platypusMcp.packageLocalBinaryLayouts must include ${layout}`);
    }
  }
  const platformPackages = metadata.platformBinaryPackages;
  if (!platformPackages || typeof platformPackages !== "object" || Object.keys(platformPackages).length === 0) {
    metadataErrors.push("platypusMcp.platformBinaryPackages must document supported optional binary packages");
  }
}

const extensionSource = readFileSync("extensions/platypus/index.ts", "utf8");
const typedToolSetMatch = extensionSource.match(/CORE_TYPED_TOOL_NAMES\s*=\s*new Set<string>\(\[([\s\S]*?)\]\)/);
if (!typedToolSetMatch) {
  metadataErrors.push("extensions/platypus/index.ts must define CORE_TYPED_TOOL_NAMES");
} else {
  const typedToolSetSource = typedToolSetMatch[1];
  for (const toolName of requiredCoreTypedTools) {
    if (!typedToolSetSource.includes(`"${toolName}"`)) {
      metadataErrors.push(`Core Pi tool ${toolName} must be listed in CORE_TYPED_TOOL_NAMES`);
    }
  }
}
for (const toolName of requiredCoreTypedTools) {
  const typedObjectPattern = new RegExp(`\\b${toolName}:\\s*(?:Type\\.|noArgs\\()`);
  if (!typedObjectPattern.test(extensionSource)) {
    metadataErrors.push(`Core Pi tool ${toolName} must define explicit typed parameters`);
  }
  const directPassthroughPattern = new RegExp(`name:\\s*"${toolName}"[\\s\\S]{0,500}parameters:\\s*passthroughParameters`);
  if (directPassthroughPattern.test(extensionSource)) {
    metadataErrors.push(`Core Pi tool ${toolName} must not register passthroughParameters directly`);
  }
}
if (!extensionSource.includes("platypus_call_tool")) {
  metadataErrors.push("Pi extension must keep platypus_call_tool as the generic escape hatch");
}

const packedFiles = Array.isArray(pack?.files) ? pack.files : [];
const binaryCandidates = files.filter((file) => file.startsWith("bin/") || file.startsWith("vendor/"));
const executableBinaryCandidates = packedFiles
  .filter((file) => typeof file?.path === "string" && binaryCandidates.includes(file.path))
  .filter((file) => typeof file?.mode === "number" && (file.mode & 0o111) !== 0)
  .map((file) => file.path);
const requireBinary = process.env.PLATYPUS_NPM_REQUIRE_BINARY === "1";
if (requireBinary && binaryCandidates.length === 0) {
  metadataErrors.push("PLATYPUS_NPM_REQUIRE_BINARY=1 requires at least one bin/ or vendor/ binary in the package");
} else if (requireBinary && executableBinaryCandidates.length === 0) {
  metadataErrors.push("PLATYPUS_NPM_REQUIRE_BINARY=1 requires at least one executable bin/ or vendor/ binary in the package");
}

const requiredPlatforms = (process.env.PLATYPUS_NPM_REQUIRED_PLATFORMS ?? "")
  .split(",")
  .map((platform) => platform.trim())
  .filter(Boolean);
for (const platform of requiredPlatforms) {
  const platformCandidates = [
    `bin/${platform}/${requiredMetadata.binaryName}`,
    `vendor/${platform}/${requiredMetadata.binaryName}`,
  ];
  const packedPlatformBinaries = packedFiles.filter((file) => platformCandidates.includes(file.path));
  if (packedPlatformBinaries.length === 0) {
    metadataErrors.push(`Required platform binary missing for ${platform}; expected ${platformCandidates.join(" or ")}`);
    continue;
  }
  if (!packedPlatformBinaries.some((file) => typeof file?.mode === "number" && (file.mode & 0o111) !== 0)) {
    metadataErrors.push(`Required platform binary for ${platform} must be executable`);
  }
}

if (missing.length > 0 || forbidden.length > 0 || metadataErrors.length > 0) {
  if (missing.length > 0) console.error(`Missing required npm package files: ${missing.join(", ")}`);
  if (forbidden.length > 0) console.error(`Forbidden npm package files: ${forbidden.join(", ")}`);
  for (const error of metadataErrors) console.error(error);
  process.exit(1);
}

console.log(`npm package dry run ok: ${files.length} file(s), ${pack?.size ?? "unknown"} bytes.`);
if (binaryCandidates.length === 0) {
  console.log("No package-local MCP binary is present in this source checkout; release builds may add bin/ or vendor/ platform binaries before packing.");
} else {
  console.log(`Included package-local binary candidate(s): ${binaryCandidates.join(", ")}`);
}
if (requiredPlatforms.length > 0) {
  console.log(`Required platform binary validation passed: ${requiredPlatforms.join(", ")}`);
}
