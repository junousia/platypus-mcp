use super::types::VALID_TYPES;
use crate::{
    models::{
        ActionResult, ActionStatus, DraftBacklogData, DraftBacklogItem, DraftBacklogItemsParams,
    },
    sampling,
};
use serde::Deserialize;

const ACTION: &str = "draft_backlog_items";

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SampledBacklogDrafts {
    Wrapped { drafts: Vec<DraftBacklogItem> },
    Bare(Vec<DraftBacklogItem>),
}

pub fn draft_backlog_items(params: DraftBacklogItemsParams) -> ActionResult<DraftBacklogData> {
    if let Err(error) = validate_goal(&params.goal) {
        return ActionResult::failed(ACTION, "Could not draft backlog items.", error);
    }
    skipped_without_sampling()
}

pub fn draft_backlog_items_sampling_prompt(
    params: &DraftBacklogItemsParams,
) -> Result<String, String> {
    validate_goal(&params.goal)?;
    let worker = params
        .suggested_worker
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("coder");
    let surfaces = if params.owned_surfaces.is_empty() {
        "none declared".to_string()
    } else {
        params.owned_surfaces.join(", ")
    };
    let verification = if params.verification_command.is_empty() {
        "none declared".to_string()
    } else {
        params.verification_command.join(" && ")
    };
    Ok(format!(
        "Draft concrete Platypus backlog candidates for this goal.\n\n\
         Goal: {goal}\n\
         Suggested worker: {worker}\n\
         Owned surfaces: {surfaces}\n\
         Verification command: {verification}\n\n\
         Return only JSON. The JSON must be either an array of draft objects or \
         an object with a `drafts` array. Each draft object must have exactly \
         these fields: candidate_id, title, objective, type, area, owned_surfaces, \
         suggested_worker, verification_command.\n\n\
         Rules:\n\
         - Decompose the actual goal into concrete independently reviewable work.\n\
         - Do not return generic Shape/Implement/Verify items.\n\
         - Use type values only from: foundation, feature, safety, ux, test, docs.\n\
         - Make titles specific to the domain and changed surface.\n\
         - Keep verification_command as an array of command strings.",
        goal = params.goal.trim()
    ))
}

pub fn draft_backlog_items_from_sample(
    params: &DraftBacklogItemsParams,
    text: &str,
) -> ActionResult<DraftBacklogData> {
    if let Err(error) = validate_goal(&params.goal) {
        return ActionResult::failed(ACTION, "Could not draft backlog items.", error);
    }
    let parsed = match sampling::parse_sampled_json::<SampledBacklogDrafts>(text) {
        Ok(SampledBacklogDrafts::Wrapped { drafts }) | Ok(SampledBacklogDrafts::Bare(drafts)) => {
            drafts
        }
        Err(error) => return ActionResult::failed(ACTION, "Could not draft backlog items.", error),
    };
    let drafts = match validate_sampled_drafts(params.goal.trim(), parsed) {
        Ok(drafts) => drafts,
        Err(error) => return ActionResult::failed(ACTION, "Could not draft backlog items.", error),
    };
    ActionResult::completed(
        ACTION,
        format!(
            "Drafted {} backlog candidate(s) with host sampling.",
            drafts.len()
        ),
        DraftBacklogData { drafts },
    )
}

fn skipped_without_sampling() -> ActionResult<DraftBacklogData> {
    ActionResult {
        action: ACTION.to_string(),
        status: ActionStatus::Skipped,
        summary: "Backlog drafting requires model judgment; this MCP client did not provide sampling."
            .to_string(),
        next_action: Some(
            "Sampling is optional and unavailable in this MCP client. Use the host agent to reason about the goal, then call create_backlog_item with concrete title, goal, type, acceptance, and contract fields; run validate_backlog and commit backlog/items/*.md before dispatch."
                .to_string(),
        ),
        data: Some(DraftBacklogData { drafts: Vec::new() }),
        error: None,
    }
}

fn validate_goal(goal: &str) -> Result<(), String> {
    if goal.trim().is_empty() {
        Err("goal is empty".to_string())
    } else {
        Ok(())
    }
}

fn validate_sampled_drafts(
    goal: &str,
    drafts: Vec<DraftBacklogItem>,
) -> Result<Vec<DraftBacklogItem>, String> {
    if drafts.is_empty() {
        return Err("sampled backlog drafts were empty".to_string());
    }
    let goal_lower = normalize_for_placeholder(goal);
    let mut candidate_ids = std::collections::BTreeSet::new();
    for draft in &drafts {
        if draft.candidate_id.trim().is_empty() {
            return Err("draft candidate_id is required".to_string());
        }
        if !candidate_ids.insert(draft.candidate_id.trim().to_string()) {
            return Err(format!(
                "duplicate draft candidate_id `{}`",
                draft.candidate_id
            ));
        }
        if draft.title.trim().is_empty() {
            return Err(format!("{} title is required", draft.candidate_id));
        }
        if is_placeholder_title(&draft.title, &goal_lower) {
            return Err(format!(
                "{} has generic placeholder title `{}`",
                draft.candidate_id, draft.title
            ));
        }
        if draft.objective.trim().is_empty() {
            return Err(format!("{} objective is required", draft.candidate_id));
        }
        if !VALID_TYPES.contains(&draft.item_type.as_str()) {
            return Err(format!(
                "{} has invalid type `{}`; expected one of: {}",
                draft.candidate_id,
                draft.item_type,
                VALID_TYPES.join(", ")
            ));
        }
        if draft.area.trim().is_empty() {
            return Err(format!("{} area is required", draft.candidate_id));
        }
    }
    Ok(drafts)
}

fn is_placeholder_title(title: &str, goal_lower: &str) -> bool {
    let title = normalize_for_placeholder(title);
    ["shape", "implement", "verify", "plan", "build"]
        .iter()
        .any(|prefix| title == format!("{prefix} {goal_lower}"))
}

fn normalize_for_placeholder(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DraftBacklogItemsParams {
        DraftBacklogItemsParams {
            goal: "Build a FastAPI and React app with auth".to_string(),
            suggested_worker: Some("coder".to_string()),
            owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
            verification_command: vec!["make check".to_string()],
        }
    }

    #[test]
    fn skips_without_sampling_instead_of_generating_templates() {
        let result = draft_backlog_items(params());

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert!(result.data.expect("data").drafts.is_empty());
        assert!(result
            .next_action
            .expect("next action")
            .contains("create_backlog_item"));
    }

    #[test]
    fn parses_valid_sampled_backlog_drafts() {
        let result = draft_backlog_items_from_sample(
            &params(),
            r#"{
              "drafts": [
                {
                  "candidate_id": "draft-1",
                  "title": "Scaffold FastAPI authentication backend",
                  "objective": "Create the backend auth surface.",
                  "type": "foundation",
                  "area": "backend",
                  "owned_surfaces": ["backend/"],
                  "suggested_worker": "coder",
                  "verification_command": ["make check"]
                }
              ]
            }"#,
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(result.data.expect("drafts").drafts.len(), 1);
    }

    #[test]
    fn rejects_placeholder_sampled_titles() {
        let result = draft_backlog_items_from_sample(
            &params(),
            r#"[
              {
                "candidate_id": "draft-1",
                "title": "Implement Build a FastAPI and React app with auth",
                "objective": "Too generic.",
                "type": "feature",
                "area": "implementation",
                "owned_surfaces": [],
                "suggested_worker": "coder",
                "verification_command": []
              }
            ]"#,
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.expect("error").contains("generic placeholder"));
    }

    #[test]
    fn rejects_invalid_sampled_type() {
        let result = draft_backlog_items_from_sample(
            &params(),
            r#"[
              {
                "candidate_id": "draft-1",
                "title": "Scaffold backend",
                "objective": "Create the backend.",
                "type": "backlog",
                "area": "backend",
                "owned_surfaces": ["backend/"],
                "suggested_worker": "coder",
                "verification_command": ["make check"]
              }
            ]"#,
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.expect("error").contains("invalid type"));
    }
}
