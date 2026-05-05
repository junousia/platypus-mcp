use crate::models::{ActionResult, DraftBacklogData, DraftBacklogItem, DraftBacklogItemsParams};

pub fn draft_backlog_items(params: DraftBacklogItemsParams) -> ActionResult<DraftBacklogData> {
    let action = "draft_backlog_items";
    let goal = params.goal.trim();
    if goal.is_empty() {
        return ActionResult::failed(action, "Could not draft backlog items.", "goal is empty");
    }
    let worker = params
        .suggested_worker
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some("coder".to_string()));
    let verification = if params.verification_command.is_empty() {
        vec!["cargo".to_string(), "test".to_string()]
    } else {
        params.verification_command
    };
    let drafts = vec![
        DraftBacklogItem {
            candidate_id: "draft-1".to_string(),
            title: format!("Shape {}", goal),
            objective: format!("Clarify scope, constraints, and acceptance for {}.", goal),
            owned_surfaces: params.owned_surfaces.clone(),
            suggested_worker: worker.clone(),
            required_capability_tags: params.required_capability_tags.clone(),
            complexity_tier: params.complexity_tier.clone(),
            verification_command: verification.clone(),
        },
        DraftBacklogItem {
            candidate_id: "draft-2".to_string(),
            title: format!("Implement {}", goal),
            objective: format!(
                "Deliver the first working implementation slice for {}.",
                goal
            ),
            owned_surfaces: params.owned_surfaces.clone(),
            suggested_worker: worker.clone(),
            required_capability_tags: params.required_capability_tags.clone(),
            complexity_tier: params.complexity_tier.clone(),
            verification_command: verification.clone(),
        },
        DraftBacklogItem {
            candidate_id: "draft-3".to_string(),
            title: format!("Verify {}", goal),
            objective: format!(
                "Add deterministic verification and recovery coverage for {}.",
                goal
            ),
            owned_surfaces: params.owned_surfaces,
            suggested_worker: worker,
            required_capability_tags: params.required_capability_tags,
            complexity_tier: params.complexity_tier,
            verification_command: verification,
        },
    ];
    ActionResult::completed(
        action,
        format!("Drafted {} backlog candidate(s).", drafts.len()),
        DraftBacklogData { drafts },
    )
}
