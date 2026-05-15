use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let args = Args::parse();
    let version = release_version(&args.workspace_status);
    let source = fs::read_to_string(&args.input)
        .unwrap_or_else(|error| fatal(format!("could not read {}: {error}", args.input.display())));
    let stamped = match args.input.file_name().and_then(|value| value.to_str()) {
        Some("Cargo.toml") => stamp_cargo_manifest(&source, &args.package, &version),
        Some("Cargo.lock") => stamp_cargo_lock(&source, &args.package, &version),
        Some("package.json") => stamp_package_json(&source, &args.package, &version),
        other => fatal(format!("unsupported manifest type: {other:?}")),
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| fatal(format!("could not create {}: {error}", parent.display())));
    }
    fs::write(&args.output, stamped)
        .unwrap_or_else(|error| fatal(format!("could not write {}: {error}", args.output.display())));
}

#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    package: String,
    workspace_status: PathBuf,
}

impl Args {
    fn parse() -> Self {
        let mut input = None;
        let mut output = None;
        let mut package = None;
        let mut workspace_status = None;
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .unwrap_or_else(|| fatal(format!("missing value for {flag}")));
            match flag.as_str() {
                "--input" => input = Some(PathBuf::from(value)),
                "--output" => output = Some(PathBuf::from(value)),
                "--package" => package = Some(value),
                "--workspace-status" => workspace_status = Some(PathBuf::from(value)),
                _ => fatal(format!("unknown argument: {flag}")),
            }
        }
        Self {
            input: input.unwrap_or_else(|| fatal("missing --input")),
            output: output.unwrap_or_else(|| fatal("missing --output")),
            package: package.unwrap_or_else(|| fatal("missing --package")),
            workspace_status: workspace_status.unwrap_or_else(|| fatal("missing --workspace-status")),
        }
    }
}

fn release_version(workspace_status: &Path) -> String {
    let Ok(source) = fs::read_to_string(workspace_status) else {
        return "0.0.0-dev".to_string();
    };
    for line in source.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        if matches!(key, "STABLE_BUILD_VERSION" | "BUILD_SCM_VERSION") {
            let version = value.strip_prefix('v').unwrap_or(value);
            if is_release_semver(version) {
                return version.to_string();
            }
        }
    }
    "0.0.0-dev".to_string()
}

fn stamp_cargo_manifest(source: &str, package: &str, version: &str) -> String {
    let mut lines: Vec<String> = source.lines().map(ToString::to_string).collect();
    let mut in_package = false;
    let mut in_target_package = false;
    for line in &mut lines {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            in_package = stripped == "[package]";
            in_target_package = false;
            continue;
        }
        if !in_package {
            continue;
        }
        if stripped.starts_with("name") {
            in_target_package = stripped == format!("name = \"{package}\"");
        }
        if in_target_package && stripped.starts_with("version") {
            *line = format!("version = \"{version}\"");
            return finish_lines(lines);
        }
    }
    fatal(format!("could not stamp Cargo manifest package {package:?}"))
}

fn stamp_cargo_lock(source: &str, package: &str, version: &str) -> String {
    let mut lines: Vec<String> = source.lines().map(ToString::to_string).collect();
    let mut in_package = false;
    let mut in_target_package = false;
    for line in &mut lines {
        let stripped = line.trim();
        if stripped == "[[package]]" {
            in_package = true;
            in_target_package = false;
            continue;
        }
        if stripped.starts_with('[') && stripped.ends_with(']') && stripped != "[[package]]" {
            in_package = false;
            in_target_package = false;
            continue;
        }
        if !in_package {
            continue;
        }
        if stripped.starts_with("name") {
            in_target_package = stripped == format!("name = \"{package}\"");
        }
        if in_target_package && stripped.starts_with("version") {
            *line = format!("version = \"{version}\"");
            return finish_lines(lines);
        }
    }
    fatal(format!("could not stamp Cargo lock package {package:?}"))
}

fn stamp_package_json(source: &str, package: &str, version: &str) -> String {
    let name_line = format!("  \"name\": \"{package}\",");
    if !source.lines().any(|line| line.trim_end() == name_line) {
        return fatal(format!("package.json does not describe package {package:?}"));
    }
    let mut changed = false;
    let lines = source
        .lines()
        .map(|line| {
            if !changed && line.trim_start().starts_with("\"version\":") {
                changed = true;
                "  \"version\": \"".to_string() + version + "\","
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    if !changed {
        return fatal("could not stamp package.json version");
    }
    finish_lines(lines)
}

fn finish_lines(lines: Vec<String>) -> String {
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn is_release_semver(value: &str) -> bool {
    let (core, suffix) = value.split_once('-').unwrap_or((value, ""));
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        && suffix.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
}

fn fatal<T>(message: impl std::fmt::Display) -> T {
    eprintln!("{message}");
    std::process::exit(2);
}
