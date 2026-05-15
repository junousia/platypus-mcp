#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";

const requiredFiles = new Set([
  "package.json",
  "extensions/platypus/index.ts",
]);

const requiredMetadata = {
  binaryName: "platypus-mcp",
  binaryFallbackOrder: [
    "PLATYPUS_MCP_BIN",
    "package-local bin/ or vendor/ platform binary",
    "development Cargo.toml checkout",
    "platypus-mcp on PATH",
  ],
};

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
  const platformPackages = metadata.platformBinaryPackages;
  if (!platformPackages || typeof platformPackages !== "object" || Object.keys(platformPackages).length === 0) {
    metadataErrors.push("platypusMcp.platformBinaryPackages must document supported optional binary packages");
  }
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
