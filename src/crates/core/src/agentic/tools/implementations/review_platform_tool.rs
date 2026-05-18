//! Pull request / review platform tool.
//!
//! This tool exposes hosted review-platform operations to the agent while
//! keeping provider-specific HTTP behavior inside `ReviewPlatformService`.

use crate::agentic::tools::framework::{
    Tool, ToolExposure, ToolRenderOptions, ToolResult, ToolUseContext, ValidationResult,
};
use crate::service::review_platform::{
    ReviewPlatformApprovalRequest, ReviewPlatformCreatePullRequestRequest, ReviewPlatformRemote,
    ReviewPlatformReplyToThreadRequest, ReviewPlatformRequestChangesRequest,
    ReviewPlatformResolveThreadRequest, ReviewPlatformService, ReviewPlatformSubmitReviewRequest,
    ReviewSubmitEvent,
};
use crate::util::errors::{BitFunError, BitFunResult};
use async_trait::async_trait;
use serde_json::{json, Value};

const ACTION_LIST: &str = "list_pull_requests";
const ACTION_COUNT: &str = "count_pull_requests";
const ACTION_GET: &str = "get_pull_request";
const ACTION_CREATE: &str = "create_pull_request";
const ACTION_REPLY: &str = "reply_to_thread";
const ACTION_SUBMIT_REVIEW: &str = "submit_review";
const ACTION_APPROVE: &str = "approve_pull_request";
const ACTION_REVOKE_APPROVAL: &str = "revoke_approval";
const ACTION_REQUEST_CHANGES: &str = "request_changes";
const ACTION_RESOLVE: &str = "resolve_thread";

const WRITE_ACTIONS: &[&str] = &[
    ACTION_CREATE,
    ACTION_REPLY,
    ACTION_SUBMIT_REVIEW,
    ACTION_APPROVE,
    ACTION_REVOKE_APPROVAL,
    ACTION_REQUEST_CHANGES,
    ACTION_RESOLVE,
];

pub struct ReviewPlatformTool;

impl ReviewPlatformTool {
    pub fn new() -> Self {
        Self
    }

    fn repository_path(input: &Value, context: &ToolUseContext) -> BitFunResult<String> {
        let requested = input
            .get("repository_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(path) = requested {
            return context.resolve_workspace_tool_path(path);
        }

        context
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path_string())
            .ok_or_else(|| BitFunError::tool("repository_path is required".to_string()))
    }

    fn string_field(input: &Value, key: &str) -> BitFunResult<String> {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| BitFunError::tool(format!("{} is required", key)))
    }

    fn optional_string_field(input: &Value, key: &str) -> Option<String> {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    fn submit_event(input: &Value) -> BitFunResult<ReviewSubmitEvent> {
        match input
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("comment")
        {
            "comment" => Ok(ReviewSubmitEvent::Comment),
            "approve" => Ok(ReviewSubmitEvent::Approve),
            "request_changes" => Ok(ReviewSubmitEvent::RequestChanges),
            other => Err(BitFunError::tool(format!(
                "Unsupported review event: {}",
                other
            ))),
        }
    }

    async fn resolve_remote_id(repository_path: &str, input: &Value) -> BitFunResult<String> {
        if let Some(remote_id) = Self::optional_string_field(input, "remote_id") {
            return Ok(remote_id);
        }

        let remotes = ReviewPlatformService::discover_remotes(repository_path)
            .await
            .map_err(|error| BitFunError::tool(error.to_string()))?;
        let supported = supported_remotes(&remotes);
        match supported.as_slice() {
            [] => Err(BitFunError::tool(
                "No supported review platform remote found".to_string(),
            )),
            [remote] => Ok(remote.id.clone()),
            _ => Err(BitFunError::tool(remote_ambiguity_message(&supported))),
        }
    }

    async fn resolve_remote_id_for_list(
        repository_path: &str,
        input: &Value,
    ) -> BitFunResult<Result<String, Value>> {
        if let Some(remote_id) = Self::optional_string_field(input, "remote_id") {
            return Ok(Ok(remote_id));
        }

        let remotes = ReviewPlatformService::discover_remotes(repository_path)
            .await
            .map_err(|error| BitFunError::tool(error.to_string()))?;
        let supported = supported_remotes(&remotes);
        match supported.as_slice() {
            [] => Err(BitFunError::tool(
                "No supported review platform remote found".to_string(),
            )),
            [remote] => Ok(Ok(remote.id.clone())),
            _ => Ok(Err(json!({
                "action": ACTION_LIST,
                "repositoryPath": repository_path,
                "status": "needs_remote_selection",
                "message": "Multiple supported review platform remotes were found. Provide remote_id explicitly.",
                "candidateRemotes": supported,
            }))),
        }
    }

    fn action(input: &Value) -> Option<&str> {
        input.get("action").and_then(Value::as_str)
    }

    fn render_action_result(output: &Value) -> Option<String> {
        let result = output.get("result")?;
        let message = result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Review platform action completed");
        let web_url = result.get("webUrl").and_then(Value::as_str);
        let pr = result.get("pullRequest");

        let mut lines = vec![message.to_string()];
        if let Some(pr) = pr {
            let title = pr
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Pull request");
            let number = pr.get("number").and_then(Value::as_i64).unwrap_or_default();
            let url = pr.get("webUrl").and_then(Value::as_str).or(web_url);
            if let Some(url) = url {
                lines.push(format!("[#{} {}]({})", number, title, url));
            }
        } else if let Some(url) = web_url {
            lines.push(url.to_string());
        }
        Some(lines.join("\n"))
    }
}

#[async_trait]
impl Tool for ReviewPlatformTool {
    fn name(&self) -> &str {
        "ReviewPlatform"
    }

    async fn description(&self) -> BitFunResult<String> {
        Ok(r#"Read and operate on hosted pull requests / merge requests.

Use this for remote review-platform operations such as counting pull requests, listing pull requests, opening pull request detail, creating a pull request, replying to review threads, submitting a comment review, approving, revoking approval, requesting changes, or resolving a review thread. Use the Git tool for local repository state and branch/commit/push operations.

When returning pull request results to the user, include the provider web URL so the chat UI can open the pull request detail panel naturally."#.to_string())
    }

    fn short_description(&self) -> String {
        "Inspect and operate on hosted pull requests / merge requests.".to_string()
    }

    fn default_exposure(&self) -> ToolExposure {
        ToolExposure::Collapsed
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        ACTION_LIST,
                        ACTION_COUNT,
                        ACTION_GET,
                        ACTION_CREATE,
                        ACTION_REPLY,
                        ACTION_SUBMIT_REVIEW,
                        ACTION_APPROVE,
                        ACTION_REVOKE_APPROVAL,
                        ACTION_REQUEST_CHANGES,
                        ACTION_RESOLVE
                    ],
                    "description": "Review platform action to perform."
                },
                "repository_path": {
                    "type": "string",
                    "description": "Repository path. Omit to use the current workspace."
                },
                "remote_id": {
                    "type": "string",
                    "description": "Review platform remote id. Omit to use the only supported remote; provide it explicitly when the repository has multiple supported review-platform remotes."
                },
                "pull_request_id": {
                    "type": "string",
                    "description": "Pull request or merge request number/id."
                },
                "page": {
                    "type": "integer",
                    "description": "Page number for list_pull_requests."
                },
                "per_page": {
                    "type": "integer",
                    "description": "Page size for list_pull_requests."
                },
                "title": {
                    "type": "string",
                    "description": "Pull request title for create_pull_request."
                },
                "source_branch": {
                    "type": "string",
                    "description": "Source/head branch for create_pull_request."
                },
                "target_branch": {
                    "type": "string",
                    "description": "Target/base branch for create_pull_request."
                },
                "body": {
                    "type": "string",
                    "description": "Pull request body, review body, or comment body depending on action."
                },
                "draft": {
                    "type": "boolean",
                    "description": "Create a draft pull request when the provider supports it."
                },
                "thread_id": {
                    "type": "string",
                    "description": "Thread id returned by get_pull_request for reply_to_thread or resolve_thread."
                },
                "event": {
                    "type": "string",
                    "enum": ["comment", "approve", "request_changes"],
                    "description": "Review event for submit_review."
                },
                "resolved": {
                    "type": "boolean",
                    "description": "Whether resolve_thread should mark the thread resolved or reopened."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn is_readonly(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self, input: Option<&Value>) -> bool {
        input
            .and_then(Self::action)
            .is_some_and(|action| !WRITE_ACTIONS.contains(&action))
    }

    fn needs_permissions(&self, input: Option<&Value>) -> bool {
        input
            .and_then(Self::action)
            .map(|action| WRITE_ACTIONS.contains(&action))
            .unwrap_or(true)
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: Option<&ToolUseContext>,
    ) -> ValidationResult {
        let Some(action) = Self::action(input) else {
            return ValidationResult {
                result: false,
                message: Some("action is required".to_string()),
                error_code: Some(400),
                meta: None,
            };
        };
        let valid = [
            ACTION_LIST,
            ACTION_COUNT,
            ACTION_GET,
            ACTION_CREATE,
            ACTION_REPLY,
            ACTION_SUBMIT_REVIEW,
            ACTION_APPROVE,
            ACTION_REVOKE_APPROVAL,
            ACTION_REQUEST_CHANGES,
            ACTION_RESOLVE,
        ];
        if !valid.contains(&action) {
            return ValidationResult {
                result: false,
                message: Some(format!("Unsupported ReviewPlatform action: {}", action)),
                error_code: Some(400),
                meta: None,
            };
        }
        ValidationResult {
            result: true,
            message: None,
            error_code: None,
            meta: None,
        }
    }

    fn render_tool_use_message(&self, input: &Value, _options: &ToolRenderOptions) -> String {
        let action = Self::action(input).unwrap_or("unknown");
        match action {
            ACTION_LIST => "List pull requests".to_string(),
            ACTION_COUNT => "Count pull requests".to_string(),
            ACTION_GET => format!(
                "Open pull request {}",
                input
                    .get("pull_request_id")
                    .and_then(Value::as_str)
                    .unwrap_or("detail")
            ),
            ACTION_CREATE => "Create pull request".to_string(),
            ACTION_REPLY => "Reply to pull request thread".to_string(),
            ACTION_SUBMIT_REVIEW => "Submit pull request review".to_string(),
            ACTION_APPROVE => "Approve pull request".to_string(),
            ACTION_REVOKE_APPROVAL => "Revoke pull request approval".to_string(),
            ACTION_REQUEST_CHANGES => "Request pull request changes".to_string(),
            ACTION_RESOLVE => "Resolve pull request thread".to_string(),
            _ => format!("Review platform action: {}", action),
        }
    }

    fn render_result_for_assistant(&self, output: &Value) -> String {
        let action = output.get("action").and_then(Value::as_str).unwrap_or("");
        if let Some(action_result) = Self::render_action_result(output) {
            return action_result;
        }

        match action {
            ACTION_COUNT => {
                if output
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "needs_remote_selection")
                {
                    let remotes = output
                        .get("candidateRemotes")
                        .and_then(Value::as_array)
                        .map(|items| items.as_slice())
                        .unwrap_or(&[]);
                    let mut lines = vec![
                        "Multiple review platform remotes were found. Ask the user which remote to use, then retry with remote_id.".to_string(),
                        "Candidate remotes:".to_string(),
                    ];
                    lines.extend(remotes.iter().map(|remote| {
                        let id = remote.get("id").and_then(Value::as_str).unwrap_or("");
                        let name = remote.get("name").and_then(Value::as_str).unwrap_or("");
                        let platform = remote.get("platform").and_then(Value::as_str).unwrap_or("");
                        let project = remote
                            .get("projectPath")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let url = remote.get("webUrl").and_then(Value::as_str).unwrap_or("");
                        format!(
                            "- remote_id: {} | name: {} | platform: {} | project: {} | url: {}",
                            id, name, platform, project, url
                        )
                    }));
                    return lines.join("\n");
                }

                let remote_id = output.get("remoteId").and_then(Value::as_str).unwrap_or("");
                let total = output.get("total").and_then(Value::as_u64);
                match total {
                    Some(total) => format!("Remote {} has {} pull requests.", remote_id, total),
                    None => format!(
                        "Remote {} did not return an exact pull request count.",
                        remote_id
                    ),
                }
            }
            ACTION_LIST => {
                if output
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "needs_remote_selection")
                {
                    let remotes = output
                        .get("candidateRemotes")
                        .and_then(Value::as_array)
                        .map(|items| items.as_slice())
                        .unwrap_or(&[]);
                    let mut lines = vec![
                        "Multiple review platform remotes were found. Ask the user which remote to use, then retry with remote_id.".to_string(),
                        "Candidate remotes:".to_string(),
                    ];
                    lines.extend(remotes.iter().map(|remote| {
                        let id = remote.get("id").and_then(Value::as_str).unwrap_or("");
                        let name = remote.get("name").and_then(Value::as_str).unwrap_or("");
                        let platform = remote.get("platform").and_then(Value::as_str).unwrap_or("");
                        let project = remote
                            .get("projectPath")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let url = remote.get("webUrl").and_then(Value::as_str).unwrap_or("");
                        format!(
                            "- remote_id: {} | name: {} | platform: {} | project: {} | url: {}",
                            id, name, platform, project, url
                        )
                    }));
                    return lines.join("\n");
                }

                let prs = output
                    .pointer("/snapshot/pullRequests")
                    .and_then(Value::as_array)
                    .map(|items| items.as_slice())
                    .unwrap_or(&[]);
                let pagination = output
                    .get("snapshot")
                    .and_then(|snapshot| snapshot.get("pagination"));
                let page = pagination
                    .and_then(|value| value.get("page"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                let per_page = pagination
                    .and_then(|value| value.get("perPage"))
                    .and_then(Value::as_u64)
                    .unwrap_or(prs.len() as u64);
                let total = pagination
                    .and_then(|value| value.get("total"))
                    .and_then(Value::as_u64);
                let has_next = pagination
                    .and_then(|value| value.get("hasNext"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let remote_id = output.get("remoteId").and_then(Value::as_str).unwrap_or("");

                let mut lines = vec![match total {
                    Some(total) => format!(
                        "Remote {} has {} pull requests. Showing {} from page {} (page size {}).",
                        remote_id,
                        total,
                        prs.len(),
                        page,
                        per_page
                    ),
                    None => format!(
                        "Remote {} returned {} pull requests on page {} (page size {}).{}",
                        remote_id,
                        prs.len(),
                        page,
                        per_page,
                        if has_next {
                            " More pages are available; this is not the total count."
                        } else {
                            ""
                        }
                    ),
                }];
                if prs.is_empty() {
                    return lines.join("\n");
                }
                lines.extend(prs.iter().take(10).map(|pr| {
                    let number = pr.get("number").and_then(Value::as_i64).unwrap_or_default();
                    let title = pr
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Untitled");
                    let state = pr.get("state").and_then(Value::as_str).unwrap_or("unknown");
                    let url = pr.get("webUrl").and_then(Value::as_str).unwrap_or("");
                    if url.is_empty() {
                        format!("#{} {} ({})", number, title, state)
                    } else {
                        format!("[#{} {}]({}) ({})", number, title, url, state)
                    }
                }));
                lines.join("\n")
            }
            ACTION_GET => {
                let pr = output.get("pullRequest");
                let Some(pr) = pr else {
                    return "Pull request detail loaded.".to_string();
                };
                let number = pr.get("number").and_then(Value::as_i64).unwrap_or_default();
                let title = pr
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled");
                let url = pr.get("webUrl").and_then(Value::as_str).unwrap_or("");
                let files = output
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0);
                let threads = output
                    .get("threads")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0);
                if url.is_empty() {
                    format!(
                        "Loaded PR #{} {} ({} files, {} threads)",
                        number, title, files, threads
                    )
                } else {
                    format!(
                        "Loaded [#{} {}]({}) ({} files, {} threads)",
                        number, title, url, files, threads
                    )
                }
            }
            _ => "Review platform action completed.".to_string(),
        }
    }

    async fn call_impl(
        &self,
        input: &Value,
        context: &ToolUseContext,
    ) -> BitFunResult<Vec<ToolResult>> {
        let action = Self::string_field(input, "action")?;
        let repository_path = Self::repository_path(input, context)?;

        let data = match action.as_str() {
            ACTION_COUNT => {
                let remote_id =
                    match Self::resolve_remote_id_for_list(&repository_path, input).await? {
                        Ok(remote_id) => remote_id,
                        Err(mut selection_result) => {
                            if let Some(obj) = selection_result.as_object_mut() {
                                obj.insert("action".to_string(), json!(ACTION_COUNT));
                            }
                            let result_for_assistant =
                                self.render_result_for_assistant(&selection_result);
                            return Ok(vec![ToolResult::Result {
                                data: selection_result,
                                result_for_assistant: Some(result_for_assistant),
                                image_attachments: None,
                            }]);
                        }
                    };
                let snapshot = ReviewPlatformService::workspace_snapshot(
                    &repository_path,
                    Some(remote_id.as_str()),
                    Some(1),
                    Some(1),
                )
                .await
                .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({
                    "action": action,
                    "repositoryPath": repository_path,
                    "remoteId": remote_id,
                    "total": snapshot.pagination.total,
                    "hasNext": snapshot.pagination.has_next,
                })
            }
            ACTION_LIST => {
                let page = input
                    .get("page")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
                let remote_id =
                    match Self::resolve_remote_id_for_list(&repository_path, input).await? {
                        Ok(remote_id) => remote_id,
                        Err(selection_result) => {
                            let result_for_assistant =
                                self.render_result_for_assistant(&selection_result);
                            return Ok(vec![ToolResult::Result {
                                data: selection_result,
                                result_for_assistant: Some(result_for_assistant),
                                image_attachments: None,
                            }]);
                        }
                    };
                let snapshot = ReviewPlatformService::workspace_snapshot(
                    &repository_path,
                    Some(remote_id.as_str()),
                    page,
                    per_page,
                )
                .await
                .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({
                    "action": action,
                    "repositoryPath": repository_path,
                    "remoteId": remote_id,
                    "snapshot": snapshot,
                })
            }
            ACTION_GET => {
                let pull_request_id = Self::string_field(input, "pull_request_id")?;
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let detail = ReviewPlatformService::pull_request_detail(
                    &repository_path,
                    &remote_id,
                    &pull_request_id,
                )
                .await
                .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({
                    "action": action,
                    "repositoryPath": repository_path,
                    "remoteId": remote_id,
                    "pullRequest": detail.pull_request,
                    "body": detail.body,
                    "files": detail.files,
                    "commits": detail.commits,
                    "threads": detail.threads,
                })
            }
            ACTION_CREATE => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformCreatePullRequestRequest {
                    repository_path,
                    remote_id: Some(remote_id),
                    title: Self::string_field(input, "title")?,
                    source_branch: Self::string_field(input, "source_branch")?,
                    target_branch: Self::string_field(input, "target_branch")?,
                    body: Self::optional_string_field(input, "body"),
                    draft: input.get("draft").and_then(Value::as_bool),
                };
                let result = ReviewPlatformService::create_pull_request(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_REPLY => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformReplyToThreadRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    thread_id: Self::string_field(input, "thread_id")?,
                    body: Self::string_field(input, "body")?,
                };
                let result = ReviewPlatformService::reply_to_thread(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_SUBMIT_REVIEW => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformSubmitReviewRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    event: Self::submit_event(input)?,
                    body: Self::string_field(input, "body")?,
                };
                let result = ReviewPlatformService::submit_review(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_APPROVE => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformApprovalRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    body: Self::optional_string_field(input, "body"),
                };
                let result = ReviewPlatformService::approve_pull_request(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_REVOKE_APPROVAL => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformApprovalRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    body: None,
                };
                let result = ReviewPlatformService::revoke_approval(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_REQUEST_CHANGES => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformRequestChangesRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    body: Self::string_field(input, "body")?,
                };
                let result = ReviewPlatformService::request_changes(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            ACTION_RESOLVE => {
                let remote_id = Self::resolve_remote_id(&repository_path, input).await?;
                let request = ReviewPlatformResolveThreadRequest {
                    repository_path,
                    remote_id,
                    pull_request_id: Self::string_field(input, "pull_request_id")?,
                    thread_id: Self::string_field(input, "thread_id")?,
                    resolved: input
                        .get("resolved")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                };
                let result = ReviewPlatformService::resolve_thread(request)
                    .await
                    .map_err(|error| BitFunError::tool(error.to_string()))?;
                json!({ "action": action, "result": result })
            }
            _ => return Err(BitFunError::tool(format!("Unsupported action: {}", action))),
        };

        let result_for_assistant = self.render_result_for_assistant(&data);
        Ok(vec![ToolResult::Result {
            data,
            result_for_assistant: Some(result_for_assistant),
            image_attachments: None,
        }])
    }
}

impl Default for ReviewPlatformTool {
    fn default() -> Self {
        Self::new()
    }
}

fn supported_remotes(remotes: &[ReviewPlatformRemote]) -> Vec<&ReviewPlatformRemote> {
    remotes.iter().filter(|remote| remote.supported).collect()
}

fn remote_ambiguity_message(remotes: &[&ReviewPlatformRemote]) -> String {
    let mut lines = vec![
        "Multiple supported review platform remotes were found. Provide remote_id explicitly."
            .to_string(),
        "Candidate remotes:".to_string(),
    ];
    lines.extend(remotes.iter().map(|remote| {
        format!(
            "- remote_id: {} | name: {} | platform: {:?} | project: {} | url: {}",
            remote.id, remote.name, remote.platform, remote.project_path, remote.web_url
        )
    }));
    lines.join("\n")
}
