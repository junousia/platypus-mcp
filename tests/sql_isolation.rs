use std::{
    fs,
    path::{Path, PathBuf},
};

const ALLOWED_SQL_PATHS: &[&str] = &[
    "src/state/sqlite/mod.rs",
    "src/storage/mod.rs",
    "src/storage/probe.rs",
    "src/storage/repository.rs",
    "src/storage/schema.rs",
    "src/storage/traits.rs",
    // Temporary migration internals. Do not add new product modules here.
    "src/assignments/store.rs",
    "src/tasks.rs",
];

const DISALLOWED_MARKERS: &[&str] = &["use rusqlite", "rusqlite::", ".prepare(", ".query_row("];

#[test]
fn sql_and_rusqlite_usage_stays_in_approved_backend_modules() {
    let root = source_root();
    let mut violations = Vec::new();

    visit_rs_files(&root, &root.join("src"), &mut |relative, source| {
        if is_allowed_sql_path(relative) {
            return;
        }
        let normalized_source = source.to_lowercase();
        for marker in DISALLOWED_MARKERS {
            if normalized_source.contains(marker) {
                violations.push(format!("{relative}: contains `{marker}`"));
            }
        }
        for sql in string_literals(source).filter(|value| looks_like_sql(value)) {
            violations.push(format!("{relative}: contains raw SQL `{}`", sql.trim()));
        }
    });

    assert!(
        violations.is_empty(),
        "SQL or rusqlite usage leaked outside approved backend modules:\n{}",
        violations.join("\n")
    );
}

fn source_root() -> PathBuf {
    if let Ok(value) = std::env::var("PLATYPUS_MCP_SOURCE_ROOT") {
        if let Some(root) = resolve_source_root(PathBuf::from(value)) {
            return root;
        }
    }

    for candidate in ["_main", "platypus_mcp", "."] {
        if let Some(root) = resolve_source_root(PathBuf::from(candidate)) {
            return root;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn resolve_source_root(path: PathBuf) -> Option<PathBuf> {
    for candidate in runfile_candidates(&path) {
        if candidate.join("src").exists() {
            return Some(candidate);
        }
    }
    None
}

fn runfile_candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if path.is_absolute() || path.exists() {
        candidates.push(path.to_path_buf());
    }

    for key in ["RUNFILES_DIR", "TEST_SRCDIR"] {
        if let Ok(runfiles) = std::env::var(key) {
            let root = PathBuf::from(runfiles);
            candidates.push(root.join(path));
            candidates.push(root.join("_main").join(path));
            candidates.push(root.join("platypus_mcp").join(path));
        }
    }

    candidates
}

#[test]
fn sql_detection_is_case_insensitive_without_matching_prose() {
    assert!(looks_like_sql("insert into tasks(id) values (?)"));
    assert!(looks_like_sql("SELECT id FROM tasks"));
    assert!(looks_like_sql("  update tasks set status = ?"));
    assert!(!looks_like_sql("Update the finding disposition"));
    assert!(!looks_like_sql("Select a worker before dispatching"));
}

#[test]
fn string_literal_scanner_handles_normal_and_raw_strings() {
    let source = r##"
        let normal = "select id from tasks";
        let raw = r#"insert into tasks(id) values (?)"#;
        let prose = "Update the finding disposition";
    "##;
    let values: Vec<_> = string_literals(source).collect();
    assert_eq!(
        values,
        vec![
            "select id from tasks",
            "insert into tasks(id) values (?)",
            "Update the finding disposition",
        ]
    );
}

fn visit_rs_files(root: &Path, directory: &Path, visitor: &mut impl FnMut(&str, &str)) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            visit_rs_files(root, &path, visitor);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
            let relative = path
                .strip_prefix(root)
                .expect("relative path")
                .to_string_lossy()
                .replace('\\', "/");
            visitor(&relative, &source);
        }
    }
}

fn is_allowed_sql_path(relative: &str) -> bool {
    ALLOWED_SQL_PATHS.contains(&relative)
}

fn string_literals(source: &str) -> impl Iterator<Item = String> + '_ {
    let mut literals = Vec::new();
    let mut offset = 0;
    while let Some(start) = source[offset..].find('"') {
        let quote = offset + start;
        if let Some(raw) = raw_string_literal(source, quote) {
            literals.push(raw.value);
            offset = raw.end;
            continue;
        }
        if is_escaped_quote(source, quote) {
            offset = quote + 1;
            continue;
        }
        if let Some(normal) = normal_string_literal(source, quote) {
            literals.push(normal.value);
            offset = normal.end;
        } else {
            offset = quote + 1;
        }
    }
    literals.into_iter()
}

struct Literal {
    value: String,
    end: usize,
}

fn raw_string_literal(source: &str, quote: usize) -> Option<Literal> {
    let prefix = &source[..quote];
    let raw_start = prefix.rfind('r')?;
    if !prefix[raw_start + 1..].chars().all(|ch| ch == '#') {
        return None;
    }
    if raw_start > 0 {
        let previous = prefix[..raw_start].chars().next_back()?;
        if previous.is_ascii_alphanumeric() || previous == '_' {
            return None;
        }
    }
    let hashes = quote - raw_start - 1;
    let terminator = format!("\"{}", "#".repeat(hashes));
    let value_start = quote + 1;
    let value_end = source[value_start..].find(&terminator)? + value_start;
    Some(Literal {
        value: source[value_start..value_end].to_string(),
        end: value_end + terminator.len(),
    })
}

fn normal_string_literal(source: &str, quote: usize) -> Option<Literal> {
    let mut escaped = false;
    for (index, ch) in source[quote + 1..].char_indices() {
        let absolute = quote + 1 + index;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(Literal {
                value: source[quote + 1..absolute].to_string(),
                end: absolute + 1,
            });
        }
    }
    None
}

fn is_escaped_quote(source: &str, quote: usize) -> bool {
    let mut slash_count = 0;
    for ch in source[..quote].chars().rev() {
        if ch == '\\' {
            slash_count += 1;
        } else {
            break;
        }
    }
    slash_count % 2 == 1
}

fn looks_like_sql(value: &str) -> bool {
    let sql = value.trim().to_lowercase();
    (sql.starts_with("select ") && (sql.contains(" from ") || sql == "select 1"))
        || (sql.starts_with("insert ") && sql.contains(" into "))
        || (sql.starts_with("update ") && sql.contains(" set "))
        || (sql.starts_with("delete ") && sql.contains(" from "))
        || sql.starts_with("create table")
        || sql.starts_with("alter table")
        || sql.starts_with("drop table")
        || sql.starts_with("pragma ")
}
