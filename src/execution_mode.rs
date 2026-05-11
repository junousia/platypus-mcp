pub const AUTO: &str = "auto";
pub const PROFILED_WORKER: &str = "profiled_worker";
pub const MANUAL_HANDOFF: &str = "manual_handoff";

pub fn default_assignment_execution_mode() -> String {
    PROFILED_WORKER.to_string()
}

pub fn normalize(value: Option<&str>) -> Result<String, String> {
    let Some(value) = value else {
        return Ok(AUTO.to_string());
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(AUTO.to_string());
    }
    match value {
        AUTO | PROFILED_WORKER | MANUAL_HANDOFF => Ok(value.to_string()),
        _ => Err(format!(
            "invalid execution_mode `{value}`; expected auto, profiled_worker, or manual_handoff"
        )),
    }
}

pub fn normalize_assignment(value: Option<&str>) -> Result<String, String> {
    match normalize(value)?.as_str() {
        AUTO => Ok(PROFILED_WORKER.to_string()),
        PROFILED_WORKER => Ok(PROFILED_WORKER.to_string()),
        MANUAL_HANDOFF => Ok(MANUAL_HANDOFF.to_string()),
        _ => unreachable!("normalize only returns known execution modes"),
    }
}

pub fn is_manual_handoff(value: &str) -> bool {
    value == MANUAL_HANDOFF
}
