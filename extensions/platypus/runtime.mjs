import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

export const DEFAULT_TIMEOUT_MS = 120_000;

const defaultRequire = createRequire(import.meta.url);

export function executableName(platform = process.platform) {
	return platform === "win32" ? "platypus-mcp.exe" : "platypus-mcp";
}

function existingBinaryPath(path, exists) {
	return exists(path) ? path : undefined;
}

export function packageBinaryPath(packageRoot, options = {}) {
	const platform = options.platform ?? process.platform;
	const arch = options.arch ?? process.arch;
	const exists = options.exists ?? existsSync;
	const resolvePackage = options.resolvePackage
		?? ((packageName) => defaultRequire.resolve(`${packageName}/package.json`, { paths: [packageRoot] }));
	const name = executableName(platform);
	const platformKey = `${platform}-${arch}`;
	const candidates = [
		join(packageRoot, "bin", name),
		join(packageRoot, "bin", platformKey, name),
		join(packageRoot, "vendor", platformKey, name),
	];
	for (const candidate of candidates) {
		const found = existingBinaryPath(candidate, exists);
		if (found) return found;
	}

	const optionalPackageNames = [
		`@platypus/mcp-${platformKey}`,
		`platypus-mcp-${platformKey}`,
	];
	for (const packageName of optionalPackageNames) {
		try {
			const packageJsonPath = resolvePackage(packageName);
			const optionalPackageRoot = resolve(packageJsonPath, "..");
			const optionalCandidates = [
				join(optionalPackageRoot, "bin", name),
				join(optionalPackageRoot, name),
			];
			for (const candidate of optionalCandidates) {
				const found = existingBinaryPath(candidate, exists);
				if (found) return found;
			}
		} catch {
			// Optional platform package is not installed for this platform.
		}
	}
	return undefined;
}

export function missingBinaryGuidance(command) {
	return [
		`Could not run Platypus MCP binary (${command}).`,
		"Tried binary resolution order:",
		"1. PLATYPUS_MCP_BIN override.",
		"2. Package-local prebuilt binary under bin/ or vendor/ for this platform.",
		"3. Development checkout Cargo.toml fallback.",
		"4. platypus-mcp on PATH.",
		"Install platypus-mcp with `cargo install platypus-mcp`, install a Platypus npm package that includes the platform binary, or set PLATYPUS_MCP_BIN to a working binary path.",
	].join("\n");
}

export function commandForTool(cwd, packageRoot, toolName, params, options = {}) {
	const cleanParams = { ...params };
	delete cleanParams.root;

	const payload = JSON.stringify(cleanParams);
	const env = options.env ?? process.env;
	const configuredBinary = env.PLATYPUS_MCP_BIN;
	if (configuredBinary) {
		return {
			command: configuredBinary,
			args: ["tool", "--root", cwd, toolName, payload],
		};
	}

	const packagedBinary = packageBinaryPath(packageRoot, options);
	if (packagedBinary) {
		return {
			command: packagedBinary,
			args: ["tool", "--root", cwd, toolName, payload],
		};
	}

	const exists = options.exists ?? existsSync;
	const manifestPath = resolve(packageRoot, "Cargo.toml");
	if (exists(manifestPath)) {
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

export function parseTimeout(value) {
	if (!value) return DEFAULT_TIMEOUT_MS;
	const parsed = Number(value);
	return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_TIMEOUT_MS;
}

export function formatResult(toolName, stdout, stderr) {
	const trimmed = stdout.trim();
	let details = {};
	if (trimmed) {
		try {
			const parsed = JSON.parse(trimmed);
			if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
				details = parsed;
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

export async function runPlatypusTool(pi, ctx, toolName, params, options = {}) {
	const packageRoot = options.packageRoot;
	if (!packageRoot) throw new Error("runPlatypusTool requires packageRoot.");
	const { command, args } = commandForTool(ctx.cwd, packageRoot, toolName, params, options);
	let result;
	try {
		result = await pi.exec(command, args, {
			signal: ctx.signal,
			timeout: parseTimeout(options.timeoutMs ?? (options.env ?? process.env).PLATYPUS_PI_TIMEOUT_MS),
		});
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		const guidance = missingBinaryGuidance(command);
		return {
			content: [
				{
					type: "text",
					text: `${guidance}\n\nUnderlying error: ${message}`,
				},
			],
			details: {
				status: "failed",
				command,
				error: message,
			},
			isError: true,
		};
	}

	const formatted = formatResult(toolName, result.stdout ?? "", result.stderr ?? "");
	if (result.code !== 0) {
		return {
			content: [
				{
					type: "text",
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
		content: [{ type: "text", text: formatted.text }],
		details: formatted.details,
	};
}
