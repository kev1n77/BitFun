use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bitfun_core::agentic::coordination::ConversationCoordinator;
use bitfun_core::runtime_ports::{ThreadGoal, ThreadGoalStatus};

#[derive(Clone, Debug)]
pub struct CliGoalSession {
    pub session_id: String,
    pub workspace_path: PathBuf,
}

pub async fn resolve_goal_session(
    coordinator: &Arc<ConversationCoordinator>,
    workspace_path: &Path,
    session: Option<&str>,
) -> Result<CliGoalSession> {
    let session_id = match session.unwrap_or("last") {
        "last" => coordinator
            .list_sessions(workspace_path)
            .await?
            .first()
            .map(|session| session.session_id.clone())
            .ok_or_else(|| anyhow!("No history sessions for current project"))?,
        id => id.to_string(),
    };

    coordinator
        .restore_session(workspace_path, &session_id)
        .await?;

    Ok(CliGoalSession {
        session_id,
        workspace_path: workspace_path.to_path_buf(),
    })
}

pub fn goal_is_terminal(goal: &ThreadGoal) -> bool {
    matches!(
        goal.status,
        ThreadGoalStatus::Blocked
            | ThreadGoalStatus::UsageLimited
            | ThreadGoalStatus::BudgetLimited
            | ThreadGoalStatus::Complete
    )
}

pub fn parse_goal_from_event_payload(value: &serde_json::Value) -> Option<ThreadGoal> {
    serde_json::from_value(value.clone()).ok()
}

pub fn format_goal_summary(goal: &ThreadGoal) -> String {
    let budget = goal
        .token_budget
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let remaining = goal
        .remaining_tokens()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unbounded".to_string());

    format!(
        "Goal: {}\nStatus: {}\nSession: {}\nTokens: {} used / {} budget / {} remaining\nAuto continuations: {}",
        goal.objective,
        goal.status.as_str(),
        goal.session_id,
        goal.tokens_used,
        budget,
        remaining,
        goal.auto_continuation_count
    )
}

pub fn compact_goal_summary(goal: &ThreadGoal) -> String {
    format!(
        "Goal {}: {} ({})",
        goal.status.as_str(),
        goal.objective,
        goal.session_id
    )
}
