//! Platform-neutral pull request review data service.
//!
//! This module owns provider detection, token handling, and provider-specific
//! HTTP calls. UI and desktop adapters consume only the common DTOs below.

use crate::infrastructure::try_get_path_manager_arc;
use crate::service::git::{execute_git_command, get_repository_root};
use futures::{stream, StreamExt};
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;

const USER_AGENT_VALUE: &str = "BitFun";
const DEFAULT_PR_PAGE: u32 = 1;
const DEFAULT_PR_PAGE_SIZE: u32 = 10;
const MAX_PR_PAGE_SIZE: u32 = 50;
const PROVIDER_ENRICH_CONCURRENCY: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum ReviewPlatformError {
    #[error("Invalid repository path: {0}")]
    InvalidRepository(String),
    #[error("Remote not found: {0}")]
    RemoteNotFound(String),
    #[error("Unsupported review platform: {0}")]
    UnsupportedPlatform(String),
    #[error("Provider API failed: {0}")]
    Api(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPlatformKind {
    Github,
    Gitlab,
    Codehub,
    Gitcode,
    Unknown,
}

impl ReviewPlatformKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Codehub => "codehub",
            Self::Gitcode => "gitcode",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAuthState {
    NotConnected,
    NotRequired,
    Connected,
    Expired,
    Error,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAuthSource {
    Env,
    Stored,
    None,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewItemState {
    Open,
    Merged,
    Closed,
    Draft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    Commented,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformAccount {
    pub id: String,
    pub platform: ReviewPlatformKind,
    pub label: String,
    pub username: Option<String>,
    pub host: String,
    pub auth_state: ReviewAuthState,
    pub auth_source: ReviewAuthSource,
    pub scopes: Vec<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformRepositoryRef {
    pub provider_id: String,
    pub platform: ReviewPlatformKind,
    pub host: String,
    pub owner: String,
    pub name: String,
    pub project_path: String,
    pub default_branch: String,
    pub workspace_path: Option<String>,
    pub web_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformRemote {
    pub id: String,
    pub name: String,
    pub url: String,
    pub platform: ReviewPlatformKind,
    pub host: String,
    pub owner: String,
    pub repository_name: String,
    pub project_path: String,
    pub web_url: String,
    pub supported: bool,
    pub auth_state: ReviewAuthState,
    pub auth_source: ReviewAuthSource,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewChecks {
    pub total: i32,
    pub passed: i32,
    pub failed: i32,
    pub pending: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformPullRequest {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub state: ReviewItemState,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    pub updated_at: String,
    pub web_url: String,
    pub additions: i32,
    pub deletions: i32,
    pub changed_files: i32,
    pub comments: i32,
    pub review_decision: ReviewDecision,
    pub checks: ReviewChecks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ReviewFileStatus,
    pub additions: i32,
    pub deletions: i32,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformCommit {
    pub hash: String,
    pub short_hash: String,
    pub title: String,
    pub author: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPlatformThreadKind {
    Review,
    Comment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformThread {
    pub id: String,
    pub provider_thread_id: Option<String>,
    pub provider_comment_id: Option<String>,
    pub kind: ReviewPlatformThreadKind,
    pub reply_to_provider_comment_id: Option<String>,
    pub file_path: Option<String>,
    pub line: Option<i64>,
    pub resolved: bool,
    pub author: String,
    pub body: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformPullRequestDetail {
    #[serde(flatten)]
    pub pull_request: ReviewPlatformPullRequest,
    pub body: String,
    pub files: Vec<ReviewPlatformFile>,
    pub commits: Vec<ReviewPlatformCommit>,
    pub threads: Vec<ReviewPlatformThread>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformCapabilities {
    pub can_create_review: bool,
    pub can_create_pull_request: bool,
    pub can_reply_to_thread: bool,
    pub can_resolve_thread: bool,
    pub can_approve: bool,
    pub can_revoke_approval: bool,
    pub can_request_changes: bool,
    pub can_merge: bool,
    pub supports_draft_review: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSubmitEvent {
    Comment,
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformCreatePullRequestRequest {
    pub repository_path: String,
    pub remote_id: Option<String>,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub body: Option<String>,
    pub draft: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformReplyToThreadRequest {
    pub repository_path: String,
    pub remote_id: String,
    pub pull_request_id: String,
    pub thread_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformSubmitReviewRequest {
    pub repository_path: String,
    pub remote_id: String,
    pub pull_request_id: String,
    pub event: ReviewSubmitEvent,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformResolveThreadRequest {
    pub repository_path: String,
    pub remote_id: String,
    pub pull_request_id: String,
    pub thread_id: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformApprovalRequest {
    pub repository_path: String,
    pub remote_id: String,
    pub pull_request_id: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformRequestChangesRequest {
    pub repository_path: String,
    pub remote_id: String,
    pub pull_request_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformActionResult {
    pub success: bool,
    pub message: String,
    pub web_url: Option<String>,
    pub pull_request: Option<ReviewPlatformPullRequest>,
    pub thread: Option<ReviewPlatformThread>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformWorkspaceSnapshot {
    pub remotes: Vec<ReviewPlatformRemote>,
    pub selected_remote_id: Option<String>,
    pub accounts: Vec<ReviewPlatformAccount>,
    pub repository: Option<ReviewPlatformRepositoryRef>,
    pub pull_requests: Vec<ReviewPlatformPullRequest>,
    pub pagination: ReviewPlatformPagination,
    pub capabilities: ReviewPlatformCapabilities,
    pub message: Option<String>,
}

pub struct ReviewPlatformService;

#[derive(Debug, Clone, Copy)]
struct PullRequestPagination {
    page: u32,
    per_page: u32,
}

impl PullRequestPagination {
    fn new(page: Option<u32>, per_page: Option<u32>) -> Self {
        Self {
            page: page.unwrap_or(DEFAULT_PR_PAGE).max(1),
            per_page: per_page
                .unwrap_or(DEFAULT_PR_PAGE_SIZE)
                .clamp(1, MAX_PR_PAGE_SIZE),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformPagination {
    pub page: u32,
    pub per_page: u32,
    pub total: Option<u64>,
    pub has_next: bool,
}

#[derive(Debug, Clone)]
struct ReviewPlatformPullRequestPage {
    items: Vec<ReviewPlatformPullRequest>,
    pagination: ReviewPlatformPagination,
}

#[derive(Debug, Clone)]
struct ProviderContext {
    remote: ReviewPlatformRemote,
    api_base_url: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ReviewPlatformAuthTokens {
    tokens: HashMap<String, String>,
}

impl ReviewPlatformAuthTokens {
    fn get(&self, platform: ReviewPlatformKind, host: &str) -> Option<&str> {
        token_key(platform, host).and_then(|key| self.tokens.get(&key).map(String::as_str))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredReviewPlatformTokens {
    #[serde(default)]
    tokens: HashMap<String, StoredReviewPlatformToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredReviewPlatformToken {
    token: String,
    updated_at: String,
}

impl ReviewPlatformService {
    pub async fn discover_remotes(
        repository_path: &str,
    ) -> Result<Vec<ReviewPlatformRemote>, ReviewPlatformError> {
        let auth_tokens = load_stored_tokens().await?;
        Self::discover_remotes_with_tokens(repository_path, &auth_tokens).await
    }

    async fn discover_remotes_with_tokens(
        repository_path: &str,
        auth_tokens: &ReviewPlatformAuthTokens,
    ) -> Result<Vec<ReviewPlatformRemote>, ReviewPlatformError> {
        let root = get_repository_root(repository_path)
            .map_err(|error| ReviewPlatformError::InvalidRepository(error.to_string()))?;
        let output = execute_git_command(&root, &["remote", "-v"])
            .await
            .map_err(|error| ReviewPlatformError::InvalidRepository(error.to_string()))?;

        let mut seen = HashSet::new();
        let mut remotes = Vec::new();

        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            if parts.get(2).is_some_and(|kind| *kind != "(fetch)") {
                continue;
            }
            let remote_name = parts[0];
            let remote_url = parts[1];
            let key = format!("{}|{}", remote_name, remote_url);
            if !seen.insert(key) {
                continue;
            }
            if let Some(remote) = parse_remote(remote_name, remote_url, auth_tokens) {
                remotes.push(remote);
            }
        }

        Ok(remotes)
    }

    pub async fn workspace_snapshot(
        repository_path: &str,
        remote_id: Option<&str>,
        page: Option<u32>,
        per_page: Option<u32>,
    ) -> Result<ReviewPlatformWorkspaceSnapshot, ReviewPlatformError> {
        if crate::service::remote_ssh::workspace_state::is_remote_path(repository_path).await {
            return Ok(empty_snapshot(
                Vec::new(),
                None,
                None,
                "Pull request browsing is not available for remote SSH workspaces yet.",
            ));
        }

        let pagination_request = PullRequestPagination::new(page, per_page);
        let auth_tokens = load_stored_tokens().await?;
        let root = get_repository_root(repository_path)
            .map_err(|error| ReviewPlatformError::InvalidRepository(error.to_string()))?;
        let remotes = Self::discover_remotes_with_tokens(&root, &auth_tokens).await?;
        let selected_remote = select_remote(&remotes, remote_id).cloned();

        let Some(remote) = selected_remote else {
            return Ok(empty_snapshot(
                remotes,
                None,
                None,
                "No Git remotes were found",
            ));
        };

        if !remote.supported {
            return Ok(empty_snapshot(
                remotes,
                Some(remote.id.clone()),
                Some(account_for_remote(&remote)),
                remote
                    .message
                    .as_deref()
                    .unwrap_or("Unsupported remote provider"),
            ));
        }

        if remote.platform == ReviewPlatformKind::Gitcode
            && token_for_remote(&remote, &auth_tokens).is_none()
        {
            return Ok(empty_snapshot(
                remotes,
                Some(remote.id.clone()),
                Some(account_for_remote(&remote)),
                "GitCode pull request APIs require a Personal Access Token. Add a token for this remote and refresh.",
            ));
        }

        let ctx = provider_context(remote.clone(), &auth_tokens)?;
        let provider = provider_for(ctx.remote.platform);
        let repository = Some(repository_ref(&ctx.remote, Some(root)));
        let account = account_for_remote(&ctx.remote);
        let page = provider
            .list_pull_requests(&ctx, pagination_request)
            .await?;

        Ok(ReviewPlatformWorkspaceSnapshot {
            remotes,
            selected_remote_id: Some(remote.id.clone()),
            accounts: vec![account],
            repository,
            pull_requests: page.items,
            pagination: page.pagination,
            capabilities: capabilities_for_remote(&remote),
            message: None,
        })
    }

    pub async fn pull_request_detail(
        repository_path: &str,
        remote_id: &str,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        if crate::service::remote_ssh::workspace_state::is_remote_path(repository_path).await {
            return Err(ReviewPlatformError::UnsupportedPlatform(
                "remote SSH workspace".to_string(),
            ));
        }

        let auth_tokens = load_stored_tokens().await?;
        let root = get_repository_root(repository_path)
            .map_err(|error| ReviewPlatformError::InvalidRepository(error.to_string()))?;
        let remotes = Self::discover_remotes_with_tokens(&root, &auth_tokens).await?;
        let remote = remotes
            .into_iter()
            .find(|remote| remote.id == remote_id)
            .ok_or_else(|| ReviewPlatformError::RemoteNotFound(remote_id.to_string()))?;
        if !remote.supported {
            return Err(ReviewPlatformError::UnsupportedPlatform(remote.host));
        }
        let ctx = provider_context(remote, &auth_tokens)?;
        provider_for(ctx.remote.platform)
            .pull_request_detail(&ctx, pull_request_id)
            .await
    }

    pub async fn create_pull_request(
        request: ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            request.remote_id.as_deref(),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .create_pull_request(&ctx, &request)
            .await
    }

    pub async fn reply_to_thread(
        request: ReviewPlatformReplyToThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .reply_to_thread(&ctx, &request)
            .await
    }

    pub async fn submit_review(
        request: ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .submit_review(&ctx, &request)
            .await
    }

    pub async fn resolve_thread(
        request: ReviewPlatformResolveThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .resolve_thread(&ctx, &request)
            .await
    }

    pub async fn approve_pull_request(
        request: ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .approve_pull_request(&ctx, &request)
            .await
    }

    pub async fn revoke_approval(
        request: ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .revoke_approval(&ctx, &request)
            .await
    }

    pub async fn request_changes(
        request: ReviewPlatformRequestChangesRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let ctx = Self::provider_context_for_repository(
            &request.repository_path,
            Some(request.remote_id.as_str()),
        )
        .await?;
        provider_for(ctx.remote.platform)
            .request_changes(&ctx, &request)
            .await
    }

    async fn provider_context_for_repository(
        repository_path: &str,
        remote_id: Option<&str>,
    ) -> Result<ProviderContext, ReviewPlatformError> {
        if crate::service::remote_ssh::workspace_state::is_remote_path(repository_path).await {
            return Err(ReviewPlatformError::UnsupportedPlatform(
                "remote SSH workspace".to_string(),
            ));
        }

        let auth_tokens = load_stored_tokens().await?;
        let root = get_repository_root(repository_path)
            .map_err(|error| ReviewPlatformError::InvalidRepository(error.to_string()))?;
        let remotes = Self::discover_remotes_with_tokens(&root, &auth_tokens).await?;
        let remote = select_remote_for_action(&remotes, remote_id)?.clone();
        if !remote.supported {
            return Err(ReviewPlatformError::UnsupportedPlatform(remote.host));
        }
        provider_context(remote, &auth_tokens)
    }

    pub async fn update_auth_token(
        platform: ReviewPlatformKind,
        host: &str,
        token: &str,
    ) -> Result<(), ReviewPlatformError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(ReviewPlatformError::Api(
                "Token cannot be empty".to_string(),
            ));
        }
        let key = token_key(platform, host)
            .ok_or_else(|| ReviewPlatformError::UnsupportedPlatform(host.to_string()))?;
        let mut stored = load_stored_token_file().await?;
        stored.tokens.insert(
            key,
            StoredReviewPlatformToken {
                token: token.to_string(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        save_stored_token_file(&stored).await
    }

    pub async fn clear_auth_token(
        platform: ReviewPlatformKind,
        host: &str,
    ) -> Result<(), ReviewPlatformError> {
        let key = token_key(platform, host)
            .ok_or_else(|| ReviewPlatformError::UnsupportedPlatform(host.to_string()))?;
        let mut stored = load_stored_token_file().await?;
        stored.tokens.remove(&key);
        save_stored_token_file(&stored).await
    }
}

#[async_trait::async_trait]
trait ReviewProvider: Sync {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError>;

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError>;

    async fn create_pull_request(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} pull request creation",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn reply_to_thread(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformReplyToThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} thread replies",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn submit_review(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} review submission",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn resolve_thread(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformResolveThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} thread resolution",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn approve_pull_request(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} pull request approval",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn revoke_approval(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} approval revocation",
            platform_label(ctx.remote.platform)
        )))
    }

    async fn request_changes(
        &self,
        ctx: &ProviderContext,
        _request: &ReviewPlatformRequestChangesRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(format!(
            "{} native change requests",
            platform_label(ctx.remote.platform)
        )))
    }
}

struct GithubProvider;
struct GitlabProvider;
struct CodehubProvider;
struct GitcodeProvider;
struct UnsupportedProvider;

fn provider_for(platform: ReviewPlatformKind) -> &'static dyn ReviewProvider {
    match platform {
        ReviewPlatformKind::Github => &GithubProvider,
        ReviewPlatformKind::Gitlab => &GitlabProvider,
        ReviewPlatformKind::Codehub => &CodehubProvider,
        ReviewPlatformKind::Gitcode => &GitcodeProvider,
        ReviewPlatformKind::Unknown => &UnsupportedProvider,
    }
}

#[async_trait::async_trait]
impl ReviewProvider for GithubProvider {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
        let url = format!(
            "{}/repos/{}/{}/pulls",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name
        );
        let per_page = pagination.per_page.to_string();
        let page = pagination.page.to_string();
        let response = send_json_response(
            github_request(http_client()?, &url, ctx.token.as_deref()).query(&[
                ("state", "all"),
                ("per_page", &per_page),
                ("page", &page),
            ]),
        )
        .await?;
        let items = response.value.as_array().ok_or_else(|| {
            ReviewPlatformError::Parse("GitHub pull response was not an array".to_string())
        })?;
        let total = pagination_total_from_links(&response.headers, pagination, items.len());
        let has_next = link_header_has_rel(&response.headers, "next");

        let pull_requests = items
            .iter()
            .map(github_pull_request_from_value)
            .collect::<Vec<_>>();
        let pull_requests = enrich_github_pull_request_counts(ctx, pull_requests).await;

        Ok(ReviewPlatformPullRequestPage {
            items: pull_requests,
            pagination: ReviewPlatformPagination {
                page: pagination.page,
                per_page: pagination.per_page,
                total,
                has_next,
            },
        })
    }

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        let base = format!(
            "{}/repos/{}/{}/pulls/{}",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request_id
        );
        let client = http_client()?;
        let detail = send_json(github_request(client.clone(), &base, ctx.token.as_deref())).await?;
        let token = ctx.token.clone();
        let files_url = format!("{}/files", base);
        let files = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                github_request(client.clone(), &files_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await?;
        let token = ctx.token.clone();
        let commits_url = format!("{}/commits", base);
        let commits = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                github_request(client.clone(), &commits_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await?;
        let token = ctx.token.clone();
        let reviews_url = format!("{}/reviews", base);
        let reviews = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                github_request(client.clone(), &reviews_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await?;
        let token = ctx.token.clone();
        let review_comments_url = format!("{}/comments", base);
        let review_comments = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                github_request(client.clone(), &review_comments_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await?;
        let token = ctx.token.clone();
        let issue_comments_url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request_id
        );
        let issue_comments = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                github_request(client.clone(), &issue_comments_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await?;

        let mut pull_request = github_pull_request_from_value(&detail);
        pull_request.review_decision = github_review_decision(&reviews);
        pull_request.checks = github_checks(ctx, &client, &detail).await;

        Ok(ReviewPlatformPullRequestDetail {
            body: value_string(&detail, "body"),
            pull_request,
            files: array_items(&files)
                .iter()
                .map(github_file_from_value)
                .collect(),
            commits: array_items(&commits)
                .iter()
                .map(github_commit_from_value)
                .collect(),
            threads: github_threads(&reviews, &review_comments, &issue_comments),
        })
    }

    async fn create_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let token = require_write_token(ctx, "Creating a pull request")?;
        let url = format!(
            "{}/repos/{}/{}/pulls",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name
        );
        let payload = json!({
            "title": request.title,
            "head": request.source_branch,
            "base": request.target_branch,
            "body": request.body.clone().unwrap_or_default(),
            "draft": request.draft.unwrap_or(false),
        });
        let value =
            send_json(github_post_request(http_client()?, &url, Some(token)).json(&payload))
                .await?;
        let pull_request = github_pull_request_from_value(&value);
        let web_url = Some(pull_request.web_url.clone());
        Ok(ReviewPlatformActionResult {
            success: true,
            message: format!("Created pull request #{}", pull_request.number),
            web_url,
            pull_request: Some(pull_request),
            thread: None,
        })
    }

    async fn reply_to_thread(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformReplyToThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let token = require_write_token(ctx, "Replying to a pull request thread")?;
        let comment_id = parse_provider_comment_id(&request.thread_id).ok_or_else(|| {
            ReviewPlatformError::Api(
                "GitHub replies require a review comment thread id such as comment-123".to_string(),
            )
        })?;
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/comments/{}/replies",
            ctx.api_base_url,
            ctx.remote.owner,
            ctx.remote.repository_name,
            request.pull_request_id,
            comment_id
        );
        let value = send_json(
            github_post_request(http_client()?, &url, Some(token))
                .json(&json!({ "body": request.body })),
        )
        .await?;
        let thread = github_thread_from_review_comment(&value);
        Ok(ReviewPlatformActionResult {
            success: true,
            message: "Replied to pull request thread".to_string(),
            web_url: value
                .get("html_url")
                .and_then(Value::as_str)
                .map(str::to_string),
            pull_request: None,
            thread: Some(thread),
        })
    }

    async fn submit_review(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let event = match request.event {
            ReviewSubmitEvent::Comment => "COMMENT",
            ReviewSubmitEvent::Approve => "APPROVE",
            ReviewSubmitEvent::RequestChanges => "REQUEST_CHANGES",
        };
        github_submit_review(ctx, &request.pull_request_id, event, &request.body).await
    }

    async fn approve_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        github_submit_review(
            ctx,
            &request.pull_request_id,
            "APPROVE",
            request.body.as_deref().unwrap_or(""),
        )
        .await
    }

    async fn request_changes(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformRequestChangesRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        github_submit_review(
            ctx,
            &request.pull_request_id,
            "REQUEST_CHANGES",
            &request.body,
        )
        .await
    }
}

async fn github_submit_review(
    ctx: &ProviderContext,
    pull_request_id: &str,
    event: &str,
    body: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, "Submitting a pull request review")?;
    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request_id
    );
    let value = send_json(
        github_post_request(http_client()?, &url, Some(token)).json(&json!({
            "body": body,
            "event": event,
        })),
    )
    .await?;
    Ok(ReviewPlatformActionResult {
        success: true,
        message: format!("Submitted GitHub review with event {}", event),
        web_url: value
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        pull_request: None,
        thread: None,
    })
}

#[async_trait::async_trait]
impl ReviewProvider for GitlabProvider {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
        gitlab_list_pull_requests(ctx, pagination).await
    }

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        gitlab_pull_request_detail(ctx, pull_request_id).await
    }

    async fn create_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_create_pull_request(ctx, request, "merge request").await
    }

    async fn reply_to_thread(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformReplyToThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_reply_to_thread(ctx, request, "merge request").await
    }

    async fn submit_review(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        if request.event != ReviewSubmitEvent::Comment {
            return Err(ReviewPlatformError::UnsupportedPlatform(
                "GitLab submit_review supports comments only; use approve_pull_request for approvals"
                    .to_string(),
            ));
        }
        gitlab_add_merge_request_note(
            ctx,
            &request.pull_request_id,
            &request.body,
            "Added merge request comment",
        )
        .await
    }

    async fn resolve_thread(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformResolveThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_resolve_thread(ctx, request, "merge request").await
    }

    async fn approve_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_approve_pull_request(ctx, request, "merge request").await
    }

    async fn revoke_approval(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_revoke_approval(ctx, request, "merge request").await
    }
}

#[async_trait::async_trait]
impl ReviewProvider for CodehubProvider {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
        gitlab_list_pull_requests(ctx, pagination).await
    }

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        gitlab_pull_request_detail(ctx, pull_request_id).await
    }

    async fn create_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_create_pull_request(ctx, request, "CodeHub merge request").await
    }

    async fn reply_to_thread(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformReplyToThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_reply_to_thread(ctx, request, "CodeHub merge request").await
    }

    async fn submit_review(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        if request.event != ReviewSubmitEvent::Comment {
            return Err(ReviewPlatformError::UnsupportedPlatform(
                "CodeHub submit_review supports comments only; use approve_pull_request if the host supports approvals"
                    .to_string(),
            ));
        }
        gitlab_add_merge_request_note(
            ctx,
            &request.pull_request_id,
            &request.body,
            "Added CodeHub merge request comment",
        )
        .await
    }

    async fn resolve_thread(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformResolveThreadRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_resolve_thread(ctx, request, "CodeHub merge request").await
    }

    async fn approve_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_approve_pull_request(ctx, request, "CodeHub merge request").await
    }

    async fn revoke_approval(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        gitlab_revoke_approval(ctx, request, "CodeHub merge request").await
    }
}

async fn gitlab_list_pull_requests(
    ctx: &ProviderContext,
    pagination: PullRequestPagination,
) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!("{}/projects/{}/merge_requests", ctx.api_base_url, project);
    let per_page = pagination.per_page.to_string();
    let page = pagination.page.to_string();
    let response = send_json_response(
        gitlab_request(http_client()?, &url, ctx.token.as_deref()).query(&[
            ("state", "all"),
            ("per_page", &per_page),
            ("page", &page),
        ]),
    )
    .await?;
    let items = response.value.as_array().ok_or_else(|| {
        ReviewPlatformError::Parse("GitLab merge request response was not an array".to_string())
    })?;
    let total = header_u64(&response.headers, "x-total");
    let has_next = header_string(&response.headers, "x-next-page")
        .is_some_and(|value| !value.trim().is_empty())
        || total
            .map(|total| u64::from(pagination.page) * u64::from(pagination.per_page) < total)
            .unwrap_or(false);

    let pull_requests = items
        .iter()
        .map(gitlab_pull_request_from_value)
        .collect::<Vec<_>>();
    let pull_requests = enrich_gitlab_pull_request_counts(ctx, pull_requests).await;

    Ok(ReviewPlatformPullRequestPage {
        items: pull_requests,
        pagination: ReviewPlatformPagination {
            page: pagination.page,
            per_page: pagination.per_page,
            total,
            has_next,
        },
    })
}

async fn gitlab_pull_request_detail(
    ctx: &ProviderContext,
    pull_request_id: &str,
) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
    let client = http_client()?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let base = format!(
        "{}/projects/{}/merge_requests/{}",
        ctx.api_base_url, project, pull_request_id
    );
    let detail = send_json(gitlab_request(client.clone(), &base, ctx.token.as_deref())).await?;
    let changes = send_json(gitlab_request(
        client.clone(),
        &format!("{}/changes", base),
        ctx.token.as_deref(),
    ))
    .await?;
    let token = ctx.token.clone();
    let commits_url = format!("{}/commits", base);
    let commits = fetch_paginated_array(
        |page| {
            let page = page.to_string();
            gitlab_request(client.clone(), &commits_url, token.as_deref())
                .query(&[("per_page", "100"), ("page", &page)])
        },
        gitlab_next_page,
    )
    .await?;
    let token = ctx.token.clone();
    let discussions_url = format!("{}/discussions", base);
    let discussions = fetch_paginated_array(
        |page| {
            let page = page.to_string();
            gitlab_request(client.clone(), &discussions_url, token.as_deref())
                .query(&[("per_page", "100"), ("page", &page)])
        },
        gitlab_next_page,
    )
    .await?;
    let token = ctx.token.clone();
    let notes_url = format!("{}/notes", base);
    let notes = fetch_paginated_array(
        |page| {
            let page = page.to_string();
            gitlab_request(client.clone(), &notes_url, token.as_deref())
                .query(&[("per_page", "100"), ("page", &page)])
        },
        gitlab_next_page,
    )
    .await?;

    let mut pull_request = gitlab_pull_request_from_value(&detail);
    let files = gitlab_files(&changes);
    pull_request.changed_files = files.len() as i32;
    let (additions, deletions) = files.iter().fold((0, 0), |acc, file| {
        (acc.0 + file.additions, acc.1 + file.deletions)
    });
    pull_request.additions = additions;
    pull_request.deletions = deletions;

    Ok(ReviewPlatformPullRequestDetail {
        body: value_string(&detail, "description"),
        pull_request,
        files,
        commits: array_items(&commits)
            .iter()
            .map(gitlab_commit_from_value)
            .collect(),
        threads: gitlab_threads(&discussions, &notes),
    })
}

async fn gitlab_create_pull_request(
    ctx: &ProviderContext,
    request: &ReviewPlatformCreatePullRequestRequest,
    label: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, &format!("Creating a {}", label))?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!("{}/projects/{}/merge_requests", ctx.api_base_url, project);
    let value = send_json(
        gitlab_post_request(http_client()?, &url, Some(token)).json(&json!({
            "title": request.title,
            "source_branch": request.source_branch,
            "target_branch": request.target_branch,
            "description": request.body.clone().unwrap_or_default(),
        })),
    )
    .await?;
    let pull_request = gitlab_pull_request_from_value(&value);
    let web_url = Some(pull_request.web_url.clone());
    Ok(ReviewPlatformActionResult {
        success: true,
        message: format!("Created {} !{}", label, pull_request.number),
        web_url,
        pull_request: Some(pull_request),
        thread: None,
    })
}

async fn gitlab_reply_to_thread(
    ctx: &ProviderContext,
    request: &ReviewPlatformReplyToThreadRequest,
    label: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, &format!("Replying to a {} thread", label))?;
    let discussion_id = parse_provider_thread_id(&request.thread_id).ok_or_else(|| {
        ReviewPlatformError::Api(
            "Replies require a discussion thread id from pull request detail".to_string(),
        )
    })?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!(
        "{}/projects/{}/merge_requests/{}/discussions/{}/notes",
        ctx.api_base_url, project, request.pull_request_id, discussion_id
    );
    let value = send_json(
        gitlab_post_request(http_client()?, &url, Some(token))
            .json(&json!({ "body": request.body })),
    )
    .await?;
    let thread = gitlab_thread_from_note(
        &value,
        Some(discussion_id.to_string()),
        false,
        ReviewPlatformThreadKind::Comment,
        None,
    );
    Ok(ReviewPlatformActionResult {
        success: true,
        message: format!("Replied to {} discussion", label),
        web_url: None,
        pull_request: None,
        thread: Some(thread),
    })
}

async fn gitlab_add_merge_request_note(
    ctx: &ProviderContext,
    pull_request_id: &str,
    body: &str,
    message: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, "Adding a merge request comment")?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!(
        "{}/projects/{}/merge_requests/{}/notes",
        ctx.api_base_url, project, pull_request_id
    );
    let value = send_json(
        gitlab_post_request(http_client()?, &url, Some(token)).json(&json!({ "body": body })),
    )
    .await?;
    let thread = gitlab_thread_from_note(
        &value,
        None,
        false,
        ReviewPlatformThreadKind::Comment,
        None,
    );
    Ok(ReviewPlatformActionResult {
        success: true,
        message: message.to_string(),
        web_url: None,
        pull_request: None,
        thread: Some(thread),
    })
}

async fn gitlab_resolve_thread(
    ctx: &ProviderContext,
    request: &ReviewPlatformResolveThreadRequest,
    label: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, &format!("Resolving a {} thread", label))?;
    let discussion_id = parse_provider_thread_id(&request.thread_id).ok_or_else(|| {
        ReviewPlatformError::Api(
            "Thread resolution requires a discussion thread id from pull request detail"
                .to_string(),
        )
    })?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!(
        "{}/projects/{}/merge_requests/{}/discussions/{}",
        ctx.api_base_url, project, request.pull_request_id, discussion_id
    );
    send_json(
        gitlab_put_request(http_client()?, &url, Some(token))
            .json(&json!({ "resolved": request.resolved })),
    )
    .await?;
    Ok(ReviewPlatformActionResult {
        success: true,
        message: if request.resolved {
            format!("Resolved {} discussion", label)
        } else {
            format!("Reopened {} discussion", label)
        },
        web_url: None,
        pull_request: None,
        thread: None,
    })
}

async fn gitlab_approve_pull_request(
    ctx: &ProviderContext,
    request: &ReviewPlatformApprovalRequest,
    label: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, &format!("Approving a {}", label))?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!(
        "{}/projects/{}/merge_requests/{}/approve",
        ctx.api_base_url, project, request.pull_request_id
    );
    send_json(gitlab_post_request(http_client()?, &url, Some(token))).await?;
    if let Some(body) = request
        .body
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let _ = gitlab_add_merge_request_note(
            ctx,
            &request.pull_request_id,
            body,
            "Added approval note",
        )
        .await;
    }
    Ok(ReviewPlatformActionResult {
        success: true,
        message: format!("Approved {}", label),
        web_url: None,
        pull_request: None,
        thread: None,
    })
}

async fn gitlab_revoke_approval(
    ctx: &ProviderContext,
    request: &ReviewPlatformApprovalRequest,
    label: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, &format!("Revoking approval for a {}", label))?;
    let project = urlencoding::encode(&ctx.remote.project_path);
    let url = format!(
        "{}/projects/{}/merge_requests/{}/unapprove",
        ctx.api_base_url, project, request.pull_request_id
    );
    send_json(gitlab_post_request(http_client()?, &url, Some(token))).await?;
    Ok(ReviewPlatformActionResult {
        success: true,
        message: format!("Revoked approval for {}", label),
        web_url: None,
        pull_request: None,
        thread: None,
    })
}

async fn gitcode_add_pull_request_comment(
    ctx: &ProviderContext,
    pull_request_id: &str,
    body: &str,
) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
    let token = require_write_token(ctx, "Adding a GitCode pull request comment")?;
    let url = format!(
        "{}/repos/{}/{}/pulls/{}/comments",
        ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request_id
    );
    let value = send_json(
        gitcode_post_request(http_client()?, &url, Some(token)).json(&json!({ "body": body })),
    )
    .await?;
    let thread = gitcode_threads(&Value::Array(vec![value]))
        .into_iter()
        .next();
    Ok(ReviewPlatformActionResult {
        success: true,
        message: "Added GitCode pull request comment".to_string(),
        web_url: None,
        pull_request: None,
        thread,
    })
}

#[async_trait::async_trait]
impl ReviewProvider for GitcodeProvider {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
        let url = format!(
            "{}/repos/{}/{}/pulls",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name
        );
        let per_page = pagination.per_page.to_string();
        let page = pagination.page.to_string();
        let response = send_json_response(
            gitcode_request(http_client()?, &url, ctx.token.as_deref()).query(&[
                ("state", "all"),
                ("per_page", &per_page),
                ("page", &page),
            ]),
        )
        .await?;
        let items = response.value.as_array().ok_or_else(|| {
            ReviewPlatformError::Parse("GitCode pull response was not an array".to_string())
        })?;
        let total = header_u64(&response.headers, "x-total").or_else(|| {
            link_header_last_page(&response.headers).map(|last_page| {
                if last_page == pagination.page {
                    (u64::from(last_page.saturating_sub(1)) * u64::from(pagination.per_page))
                        + items.len() as u64
                } else {
                    u64::from(last_page) * u64::from(pagination.per_page)
                }
            })
        });
        let has_next = link_header_has_rel(&response.headers, "next")
            || total
                .map(|total| u64::from(pagination.page) * u64::from(pagination.per_page) < total)
                .unwrap_or(items.len() == pagination.per_page as usize);

        let pull_requests = items
            .iter()
            .map(gitcode_pull_request_from_value)
            .collect::<Vec<_>>();
        let pull_requests = enrich_gitcode_pull_request_counts(ctx, pull_requests).await;

        Ok(ReviewPlatformPullRequestPage {
            items: pull_requests,
            pagination: ReviewPlatformPagination {
                page: pagination.page,
                per_page: pagination.per_page,
                total,
                has_next,
            },
        })
    }

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        let client = http_client()?;
        let base = format!(
            "{}/repos/{}/{}/pulls/{}",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request_id
        );
        let detail =
            send_json(gitcode_request(client.clone(), &base, ctx.token.as_deref())).await?;
        let token = ctx.token.clone();
        let files_url = format!("{}/files", base);
        let files = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                gitcode_request(client.clone(), &files_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await
        .unwrap_or(Value::Array(Vec::new()));
        let token = ctx.token.clone();
        let commits_url = format!("{}/commits", base);
        let commits = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                gitcode_request(client.clone(), &commits_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await
        .unwrap_or(Value::Array(Vec::new()));
        let token = ctx.token.clone();
        let comments_url = format!("{}/comments", base);
        let comments = fetch_paginated_array(
            |page| {
                let page = page.to_string();
                gitcode_request(client.clone(), &comments_url, token.as_deref())
                    .query(&[("per_page", "100"), ("page", &page)])
            },
            github_next_page,
        )
        .await
        .unwrap_or(Value::Array(Vec::new()));

        Ok(ReviewPlatformPullRequestDetail {
            body: first_non_empty(&[
                value_string(&detail, "body"),
                value_string(&detail, "description"),
            ]),
            pull_request: gitcode_pull_request_from_value(&detail),
            files: array_items(&files)
                .iter()
                .map(gitcode_file_from_value)
                .collect(),
            commits: array_items(&commits)
                .iter()
                .map(gitcode_commit_from_value)
                .collect(),
            threads: gitcode_threads(&comments),
        })
    }

    async fn create_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformCreatePullRequestRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let token = require_write_token(ctx, "Creating a GitCode pull request")?;
        let url = format!(
            "{}/repos/{}/{}/pulls",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name
        );
        let value = send_json(
            gitcode_post_request(http_client()?, &url, Some(token)).json(&json!({
                "title": request.title,
                "head": request.source_branch,
                "base": request.target_branch,
                "body": request.body.clone().unwrap_or_default(),
                "draft": request.draft.unwrap_or(false),
            })),
        )
        .await?;
        let pull_request = gitcode_pull_request_from_value(&value);
        let web_url = Some(pull_request.web_url.clone());
        Ok(ReviewPlatformActionResult {
            success: true,
            message: format!("Created GitCode pull request #{}", pull_request.number),
            web_url,
            pull_request: Some(pull_request),
            thread: None,
        })
    }

    async fn submit_review(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformSubmitReviewRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        if request.event != ReviewSubmitEvent::Comment {
            return Err(ReviewPlatformError::UnsupportedPlatform(
                "GitCode submit_review supports comments only; use approve_pull_request for review processing"
                    .to_string(),
            ));
        }
        gitcode_add_pull_request_comment(ctx, &request.pull_request_id, &request.body).await
    }

    async fn approve_pull_request(
        &self,
        ctx: &ProviderContext,
        request: &ReviewPlatformApprovalRequest,
    ) -> Result<ReviewPlatformActionResult, ReviewPlatformError> {
        let token = require_write_token(ctx, "Approving a GitCode pull request")?;
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/review",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, request.pull_request_id
        );
        send_json(
            gitcode_post_request(http_client()?, &url, Some(token))
                .json(&json!({ "force": false })),
        )
        .await?;
        if let Some(body) = request
            .body
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let _ = gitcode_add_pull_request_comment(ctx, &request.pull_request_id, body).await;
        }
        Ok(ReviewPlatformActionResult {
            success: true,
            message: "Approved GitCode pull request".to_string(),
            web_url: None,
            pull_request: None,
            thread: None,
        })
    }
}

#[async_trait::async_trait]
impl ReviewProvider for UnsupportedProvider {
    async fn list_pull_requests(
        &self,
        ctx: &ProviderContext,
        _pagination: PullRequestPagination,
    ) -> Result<ReviewPlatformPullRequestPage, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(
            ctx.remote.host.clone(),
        ))
    }

    async fn pull_request_detail(
        &self,
        ctx: &ProviderContext,
        _pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
        Err(ReviewPlatformError::UnsupportedPlatform(
            ctx.remote.host.clone(),
        ))
    }
}

fn http_client() -> Result<reqwest::Client, ReviewPlatformError> {
    reqwest::Client::builder()
        .use_native_tls()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|error| ReviewPlatformError::Network(error.to_string()))
}

struct JsonResponse {
    value: Value,
    headers: HeaderMap,
}

async fn send_json(request: reqwest::RequestBuilder) -> Result<Value, ReviewPlatformError> {
    send_json_response(request)
        .await
        .map(|response| response.value)
}

async fn send_json_response(
    request: reqwest::RequestBuilder,
) -> Result<JsonResponse, ReviewPlatformError> {
    let response = request
        .send()
        .await
        .map_err(|error| ReviewPlatformError::Network(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let preview = body.chars().take(280).collect::<String>();
        return Err(ReviewPlatformError::Api(format!(
            "HTTP {}{}",
            status,
            if preview.is_empty() {
                String::new()
            } else {
                format!(": {}", preview)
            }
        )));
    }
    let headers = response.headers().clone();
    let value = response
        .json::<Value>()
        .await
        .map_err(|error| ReviewPlatformError::Parse(error.to_string()))?;
    Ok(JsonResponse { value, headers })
}

async fn fetch_paginated_array<F>(
    mut build_request: F,
    next_page: fn(&HeaderMap, u32) -> Option<u32>,
) -> Result<Value, ReviewPlatformError>
where
    F: FnMut(u32) -> reqwest::RequestBuilder,
{
    let mut page = 1;
    let mut values = Vec::new();

    loop {
        let response = send_json_response(build_request(page)).await?;
        let items = response.value.as_array().ok_or_else(|| {
            ReviewPlatformError::Parse("Provider paginated response was not an array".to_string())
        })?;
        values.extend(items.iter().cloned());

        let Some(next) = next_page(&response.headers, page).filter(|next| *next > page) else {
            break;
        };
        page = next;
    }

    Ok(Value::Array(values))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    header_string(headers, name).and_then(|value| value.parse::<u64>().ok())
}

fn link_header_has_rel(headers: &HeaderMap, rel: &str) -> bool {
    header_string(headers, "link")
        .as_deref()
        .is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.contains(&format!("rel=\"{}\"", rel)))
        })
}

fn link_header_last_page(headers: &HeaderMap) -> Option<u32> {
    let link = header_string(headers, "link")?;
    for part in link.split(',') {
        if !part.contains("rel=\"last\"") {
            continue;
        }
        let url = part
            .split(';')
            .next()?
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        return query_param_u32(url, "page");
    }
    None
}

fn pagination_total_from_links(
    headers: &HeaderMap,
    pagination: PullRequestPagination,
    item_count: usize,
) -> Option<u64> {
    if let Some(last_page) = link_header_last_page(headers) {
        if pagination.per_page == 1 {
            return Some(u64::from(last_page));
        }
        if last_page == pagination.page {
            return Some(
                u64::from(pagination.page.saturating_sub(1)) * u64::from(pagination.per_page)
                    + item_count as u64,
            );
        }
        return None;
    }

    Some(
        u64::from(pagination.page.saturating_sub(1)) * u64::from(pagination.per_page)
            + item_count as u64,
    )
}

fn github_next_page(headers: &HeaderMap, current_page: u32) -> Option<u32> {
    if link_header_has_rel(headers, "next") {
        Some(current_page.saturating_add(1))
    } else {
        None
    }
}

fn gitlab_next_page(headers: &HeaderMap, _current_page: u32) -> Option<u32> {
    header_string(headers, "x-next-page").and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<u32>().ok()
        }
    })
}

fn query_param_u32(url: &str, name: &str) -> Option<u32> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == name {
                return value.parse::<u32>().ok();
            }
        }
    }
    None
}

async fn enrich_github_pull_request_counts(
    ctx: &ProviderContext,
    pull_requests: Vec<ReviewPlatformPullRequest>,
) -> Vec<ReviewPlatformPullRequest> {
    let Ok(client) = http_client() else {
        return pull_requests;
    };
    let futures = pull_requests.into_iter().map(|mut pull_request| {
        let client = client.clone();
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request.id
        );
        let token = ctx.token.clone();
        async move {
            if let Ok(value) = send_json(github_request(client, &url, token.as_deref())).await {
                pull_request.additions = value_i64(&value, "additions") as i32;
                pull_request.deletions = value_i64(&value, "deletions") as i32;
                pull_request.changed_files = value_i64(&value, "changed_files") as i32;
                pull_request.comments =
                    (value_i64(&value, "comments") + value_i64(&value, "review_comments")) as i32;
            }
            pull_request
        }
    });
    stream::iter(futures)
        .buffered(PROVIDER_ENRICH_CONCURRENCY)
        .collect()
        .await
}

async fn enrich_gitlab_pull_request_counts(
    ctx: &ProviderContext,
    pull_requests: Vec<ReviewPlatformPullRequest>,
) -> Vec<ReviewPlatformPullRequest> {
    let Ok(client) = http_client() else {
        return pull_requests;
    };
    let project = urlencoding::encode(&ctx.remote.project_path).to_string();
    let futures = pull_requests.into_iter().map(|mut pull_request| {
        let client = client.clone();
        let url = format!(
            "{}/projects/{}/merge_requests/{}/changes",
            ctx.api_base_url, project, pull_request.id
        );
        let token = ctx.token.clone();
        async move {
            if let Ok(value) = send_json(gitlab_request(client, &url, token.as_deref())).await {
                let files = gitlab_files(&value);
                pull_request.changed_files = files.len() as i32;
                let (additions, deletions) = files.iter().fold((0, 0), |acc, file| {
                    (acc.0 + file.additions, acc.1 + file.deletions)
                });
                pull_request.additions = additions;
                pull_request.deletions = deletions;
            }
            pull_request
        }
    });
    stream::iter(futures)
        .buffered(PROVIDER_ENRICH_CONCURRENCY)
        .collect()
        .await
}

async fn enrich_gitcode_pull_request_counts(
    ctx: &ProviderContext,
    pull_requests: Vec<ReviewPlatformPullRequest>,
) -> Vec<ReviewPlatformPullRequest> {
    let Ok(client) = http_client() else {
        return pull_requests;
    };
    let futures = pull_requests.into_iter().map(|mut pull_request| {
        let client = client.clone();
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, pull_request.id
        );
        let token = ctx.token.clone();
        async move {
            if let Ok(value) = send_json(gitcode_request(client, &url, token.as_deref())).await {
                let detail = gitcode_pull_request_from_value(&value);
                pull_request.additions = detail.additions;
                pull_request.deletions = detail.deletions;
                pull_request.changed_files = detail.changed_files;
                pull_request.comments = detail.comments;
            }
            pull_request
        }
    });
    stream::iter(futures)
        .buffered(PROVIDER_ENRICH_CONCURRENCY)
        .collect()
        .await
}

fn github_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {}", token));
    }
    request
}

fn github_post_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {}", token));
    }
    request
}

fn gitlab_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    if let Some(token) = token {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn gitlab_post_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    if let Some(token) = token {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn gitlab_put_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .put(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    if let Some(token) = token {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn gitcode_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    if let Some(token) = token {
        request = request
            .header("PRIVATE-TOKEN", token)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .query(&[("access_token", token)]);
    }
    request
}

fn gitcode_post_request(
    client: reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    if let Some(token) = token {
        request = request
            .header("PRIVATE-TOKEN", token)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .query(&[("access_token", token)]);
    }
    request
}

fn require_write_token<'a>(
    ctx: &'a ProviderContext,
    action: &str,
) -> Result<&'a str, ReviewPlatformError> {
    ctx.token.as_deref().ok_or_else(|| {
        ReviewPlatformError::Api(format!(
            "{} requires a {} token for {}",
            action,
            platform_label(ctx.remote.platform),
            ctx.remote.host
        ))
    })
}

fn provider_context(
    remote: ReviewPlatformRemote,
    auth_tokens: &ReviewPlatformAuthTokens,
) -> Result<ProviderContext, ReviewPlatformError> {
    let api_base_url = match remote.platform {
        ReviewPlatformKind::Github => "https://api.github.com".to_string(),
        ReviewPlatformKind::Gitlab => format!("https://{}/api/v4", remote.host),
        ReviewPlatformKind::Codehub => "https://codehub-y.huawei.com/api/v4".to_string(),
        ReviewPlatformKind::Gitcode => "https://api.gitcode.com/api/v5".to_string(),
        ReviewPlatformKind::Unknown => {
            return Err(ReviewPlatformError::UnsupportedPlatform(remote.host));
        }
    };
    let token = token_for_remote(&remote, auth_tokens);
    Ok(ProviderContext {
        remote,
        api_base_url,
        token,
    })
}

fn token_for_remote(
    remote: &ReviewPlatformRemote,
    auth_tokens: &ReviewPlatformAuthTokens,
) -> Option<String> {
    auth_tokens
        .get(remote.platform, &remote.host)
        .map(str::to_string)
        .or_else(|| env_token_for_platform(remote.platform))
}

fn env_token_for_platform(platform: ReviewPlatformKind) -> Option<String> {
    let names: &[&str] = match platform {
        ReviewPlatformKind::Github => &["GITHUB_TOKEN", "GH_TOKEN"],
        ReviewPlatformKind::Gitlab => &["GITLAB_TOKEN", "GITLAB_PRIVATE_TOKEN"],
        ReviewPlatformKind::Codehub => &["CODEHUB_TOKEN"],
        ReviewPlatformKind::Gitcode => &["GITCODE_TOKEN"],
        ReviewPlatformKind::Unknown => &[],
    };
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn auth_for_platform_host(
    platform: ReviewPlatformKind,
    host: &str,
    auth_tokens: &ReviewPlatformAuthTokens,
) -> (ReviewAuthState, ReviewAuthSource) {
    if platform == ReviewPlatformKind::Unknown {
        return (ReviewAuthState::Unsupported, ReviewAuthSource::Unsupported);
    }
    if auth_tokens.get(platform, host).is_some() {
        return (ReviewAuthState::Connected, ReviewAuthSource::Stored);
    }
    if env_token_for_platform(platform).is_some() {
        return (ReviewAuthState::Connected, ReviewAuthSource::Env);
    }
    if platform == ReviewPlatformKind::Gitcode {
        (ReviewAuthState::NotConnected, ReviewAuthSource::None)
    } else {
        (ReviewAuthState::NotRequired, ReviewAuthSource::None)
    }
}

fn token_key(platform: ReviewPlatformKind, host: &str) -> Option<String> {
    if platform == ReviewPlatformKind::Unknown {
        return None;
    }
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(format!("{}:{}", platform.as_str(), host))
}

fn stored_token_file_path() -> Result<PathBuf, ReviewPlatformError> {
    let path_manager =
        try_get_path_manager_arc().map_err(|error| ReviewPlatformError::Api(error.to_string()))?;
    Ok(path_manager
        .user_data_dir()
        .join("review-platform-tokens.json"))
}

async fn load_stored_tokens() -> Result<ReviewPlatformAuthTokens, ReviewPlatformError> {
    let stored = load_stored_token_file().await?;
    Ok(ReviewPlatformAuthTokens {
        tokens: stored
            .tokens
            .into_iter()
            .filter_map(|(key, entry)| {
                let token = entry.token.trim().to_string();
                if token.is_empty() {
                    None
                } else {
                    Some((key, token))
                }
            })
            .collect(),
    })
}

async fn load_stored_token_file() -> Result<StoredReviewPlatformTokens, ReviewPlatformError> {
    let path = stored_token_file_path()?;
    match fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str::<StoredReviewPlatformTokens>(&content)
            .map_err(|error| ReviewPlatformError::Parse(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(StoredReviewPlatformTokens::default())
        }
        Err(error) => Err(ReviewPlatformError::Api(format!(
            "Failed to read review platform token store: {}",
            error
        ))),
    }
}

async fn save_stored_token_file(
    stored: &StoredReviewPlatformTokens,
) -> Result<(), ReviewPlatformError> {
    let path = stored_token_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|error| {
            ReviewPlatformError::Api(format!(
                "Failed to create review platform token store directory: {}",
                error
            ))
        })?;
    }
    let content = serde_json::to_string_pretty(stored)
        .map_err(|error| ReviewPlatformError::Parse(error.to_string()))?;
    fs::write(&path, content).await.map_err(|error| {
        ReviewPlatformError::Api(format!(
            "Failed to write review platform token store: {}",
            error
        ))
    })
}

fn select_remote<'a>(
    remotes: &'a [ReviewPlatformRemote],
    remote_id: Option<&str>,
) -> Option<&'a ReviewPlatformRemote> {
    if let Some(remote_id) = remote_id {
        if let Some(remote) = remotes.iter().find(|remote| remote.id == remote_id) {
            return Some(remote);
        }
    }
    remotes
        .iter()
        .find(|remote| remote.supported)
        .or_else(|| remotes.first())
}

fn select_remote_for_action<'a>(
    remotes: &'a [ReviewPlatformRemote],
    remote_id: Option<&str>,
) -> Result<&'a ReviewPlatformRemote, ReviewPlatformError> {
    if let Some(remote_id) = remote_id {
        return remotes
            .iter()
            .find(|remote| remote.id == remote_id)
            .ok_or_else(|| ReviewPlatformError::RemoteNotFound(remote_id.to_string()));
    }

    let supported = remotes
        .iter()
        .filter(|remote| remote.supported)
        .collect::<Vec<_>>();
    match supported.as_slice() {
        [] => remotes
            .first()
            .ok_or_else(|| ReviewPlatformError::RemoteNotFound("default".to_string())),
        [remote] => Ok(remote),
        _ => Err(ReviewPlatformError::Api(format!(
            "Multiple supported review platform remotes were found. Provide remote_id explicitly. Candidate remotes:\n{}",
            supported
                .iter()
                .map(|remote| format!(
                    "- remote_id: {} | name: {} | platform: {:?} | project: {} | url: {}",
                    remote.id, remote.name, remote.platform, remote.project_path, remote.web_url
                ))
                .collect::<Vec<_>>()
                .join("\n")
        ))),
    }
}

fn empty_snapshot(
    remotes: Vec<ReviewPlatformRemote>,
    selected_remote_id: Option<String>,
    account: Option<ReviewPlatformAccount>,
    message: &str,
) -> ReviewPlatformWorkspaceSnapshot {
    let mut accounts = account.into_iter().collect::<Vec<_>>();
    if let Some(account) = accounts.first_mut() {
        if account.message.is_none() && !message.trim().is_empty() {
            account.message = Some(message.to_string());
        }
    }

    ReviewPlatformWorkspaceSnapshot {
        remotes,
        selected_remote_id,
        accounts,
        repository: None,
        pull_requests: Vec::new(),
        pagination: ReviewPlatformPagination {
            page: DEFAULT_PR_PAGE,
            per_page: DEFAULT_PR_PAGE_SIZE,
            total: Some(0),
            has_next: false,
        },
        capabilities: ReviewPlatformCapabilities {
            can_create_review: false,
            can_create_pull_request: false,
            can_reply_to_thread: false,
            can_resolve_thread: false,
            can_approve: false,
            can_revoke_approval: false,
            can_request_changes: false,
            can_merge: false,
            supports_draft_review: false,
        },
        message: if message.trim().is_empty() {
            None
        } else {
            Some(message.to_string())
        },
    }
}

fn repository_ref(
    remote: &ReviewPlatformRemote,
    workspace_path: Option<String>,
) -> ReviewPlatformRepositoryRef {
    ReviewPlatformRepositoryRef {
        provider_id: remote.id.clone(),
        platform: remote.platform,
        host: remote.host.clone(),
        owner: remote.owner.clone(),
        name: remote.repository_name.clone(),
        project_path: remote.project_path.clone(),
        default_branch: "main".to_string(),
        workspace_path,
        web_url: remote.web_url.clone(),
    }
}

fn account_for_remote(remote: &ReviewPlatformRemote) -> ReviewPlatformAccount {
    ReviewPlatformAccount {
        id: remote.id.clone(),
        platform: remote.platform,
        label: format!("{} ({})", platform_label(remote.platform), remote.host),
        username: None,
        host: remote.host.clone(),
        auth_state: remote.auth_state,
        auth_source: remote.auth_source,
        scopes: if matches!(
            remote.auth_source,
            ReviewAuthSource::Env | ReviewAuthSource::Stored
        ) {
            vec!["pull_request:read".to_string()]
        } else {
            Vec::new()
        },
        message: remote.message.clone(),
    }
}

fn capabilities_for_remote(_remote: &ReviewPlatformRemote) -> ReviewPlatformCapabilities {
    let platform = _remote.platform;
    ReviewPlatformCapabilities {
        can_create_review: matches!(
            platform,
            ReviewPlatformKind::Github
                | ReviewPlatformKind::Gitlab
                | ReviewPlatformKind::Codehub
                | ReviewPlatformKind::Gitcode
        ),
        can_create_pull_request: matches!(
            platform,
            ReviewPlatformKind::Github
                | ReviewPlatformKind::Gitlab
                | ReviewPlatformKind::Codehub
                | ReviewPlatformKind::Gitcode
        ),
        can_reply_to_thread: matches!(
            platform,
            ReviewPlatformKind::Github | ReviewPlatformKind::Gitlab | ReviewPlatformKind::Codehub
        ),
        can_resolve_thread: matches!(
            platform,
            ReviewPlatformKind::Gitlab | ReviewPlatformKind::Codehub
        ),
        can_approve: matches!(
            platform,
            ReviewPlatformKind::Github
                | ReviewPlatformKind::Gitlab
                | ReviewPlatformKind::Codehub
                | ReviewPlatformKind::Gitcode
        ),
        can_revoke_approval: matches!(
            platform,
            ReviewPlatformKind::Gitlab | ReviewPlatformKind::Codehub
        ),
        can_request_changes: matches!(platform, ReviewPlatformKind::Github),
        can_merge: false,
        supports_draft_review: matches!(platform, ReviewPlatformKind::Github),
    }
}

fn platform_label(platform: ReviewPlatformKind) -> &'static str {
    match platform {
        ReviewPlatformKind::Github => "GitHub",
        ReviewPlatformKind::Gitlab => "GitLab",
        ReviewPlatformKind::Codehub => "CodeHub",
        ReviewPlatformKind::Gitcode => "GitCode",
        ReviewPlatformKind::Unknown => "Git",
    }
}

async fn github_checks(
    ctx: &ProviderContext,
    client: &reqwest::Client,
    pull_detail: &Value,
) -> ReviewChecks {
    let sha = nested_string(pull_detail, &["head", "sha"]);
    if sha.trim().is_empty() {
        return empty_checks();
    }

    let mut checks = empty_checks();
    let status_url = format!(
        "{}/repos/{}/{}/commits/{}/status",
        ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, sha
    );
    if let Ok(status) = send_json(github_request(
        client.clone(),
        &status_url,
        ctx.token.as_deref(),
    ))
    .await
    {
        let statuses = status
            .get("statuses")
            .and_then(Value::as_array)
            .map(|items| items.as_slice())
            .unwrap_or(&[]);
        for item in statuses {
            match value_string(item, "state").as_str() {
                "success" => checks.passed += 1,
                "failure" | "error" => checks.failed += 1,
                _ => checks.pending += 1,
            }
        }
    }

    let check_runs_url = format!(
        "{}/repos/{}/{}/commits/{}/check-runs",
        ctx.api_base_url, ctx.remote.owner, ctx.remote.repository_name, sha
    );
    if let Ok(check_runs) = send_json(
        github_request(client.clone(), &check_runs_url, ctx.token.as_deref())
            .query(&[("per_page", "100")]),
    )
    .await
    {
        for item in check_runs
            .get("check_runs")
            .and_then(Value::as_array)
            .map(|items| items.as_slice())
            .unwrap_or(&[])
        {
            if value_string(item, "status") != "completed" {
                checks.pending += 1;
                continue;
            }
            match value_string(item, "conclusion").as_str() {
                "success" | "neutral" | "skipped" => checks.passed += 1,
                "failure" | "timed_out" | "cancelled" | "action_required" => checks.failed += 1,
                _ => checks.pending += 1,
            }
        }
    }

    checks.total = checks.passed + checks.failed + checks.pending;
    checks
}

fn parse_remote(
    remote_name: &str,
    remote_url: &str,
    auth_tokens: &ReviewPlatformAuthTokens,
) -> Option<ReviewPlatformRemote> {
    let parsed = parse_remote_url(remote_url)?;
    let host_lower = parsed.host.to_ascii_lowercase();
    let platform = if host_lower.contains("github.com") {
        ReviewPlatformKind::Github
    } else if host_lower.contains("-y") && host_lower.contains("codehub") {
        ReviewPlatformKind::Codehub
    } else if host_lower.contains("gitlab") {
        ReviewPlatformKind::Gitlab
    } else if host_lower.contains("gitcode") {
        ReviewPlatformKind::Gitcode
    } else {
        ReviewPlatformKind::Unknown
    };

    let segments: Vec<&str> = parsed
        .path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() < 2 {
        return None;
    }
    let owner = segments.first()?.to_string();
    let repository_name = segments.last()?.trim_end_matches(".git").to_string();
    let project_path = segments
        .iter()
        .map(|segment| segment.trim_end_matches(".git"))
        .collect::<Vec<_>>()
        .join("/");

    let supported = platform != ReviewPlatformKind::Unknown;
    let (auth_state, auth_source) = auth_for_platform_host(platform, &parsed.host, auth_tokens);
    let web_url = format!("{}://{}/{}", parsed.scheme, parsed.host, project_path);

    Some(ReviewPlatformRemote {
        id: format!(
            "{}:{}:{}",
            remote_name,
            platform.as_str(),
            project_path.replace('/', "__")
        ),
        name: remote_name.to_string(),
        url: sanitize_remote_url(remote_url),
        platform,
        host: parsed.host,
        owner,
        repository_name,
        project_path,
        web_url,
        supported,
        auth_state,
        auth_source,
        message: if !supported {
            Some("This remote is detected, but no provider adapter is available yet.".to_string())
        } else if platform == ReviewPlatformKind::Gitcode
            && auth_state == ReviewAuthState::NotConnected
        {
            Some("Add a GitCode token to load pull requests.".to_string())
        } else {
            None
        },
    })
}

#[derive(Debug)]
struct ParsedRemoteUrl {
    scheme: String,
    host: String,
    path: String,
}

fn parse_remote_url(remote_url: &str) -> Option<ParsedRemoteUrl> {
    if let Some(scheme_end) = remote_url.find("://") {
        let scheme = &remote_url[..scheme_end];
        let rest = &remote_url[scheme_end + 3..];
        let slash = rest.find('/')?;
        let authority = &rest[..slash];
        let host_part = authority.rsplit('@').next().unwrap_or(authority);
        let host = host_part.split(':').next().unwrap_or(host_part);
        let path = rest[slash + 1..].trim_end_matches(".git").to_string();
        return Some(ParsedRemoteUrl {
            scheme: if scheme == "ssh" { "https" } else { scheme }.to_string(),
            host: host.to_string(),
            path,
        });
    }

    if let Some((user_host, path)) = remote_url.split_once(':') {
        if user_host.contains('@') && !path.contains('\\') {
            let host = user_host.rsplit('@').next()?.to_string();
            return Some(ParsedRemoteUrl {
                scheme: "https".to_string(),
                host,
                path: path.trim_end_matches(".git").to_string(),
            });
        }
    }

    None
}

fn sanitize_remote_url(remote_url: &str) -> String {
    if let Some(scheme_end) = remote_url.find("://") {
        let scheme = &remote_url[..scheme_end];
        let rest = &remote_url[scheme_end + 3..];
        if let Some(slash) = rest.find('/') {
            let authority = &rest[..slash];
            if authority.contains('@') {
                let host = authority.rsplit('@').next().unwrap_or(authority);
                return format!("{}://{}/{}", scheme, host, &rest[slash + 1..]);
            }
        }
    }
    remote_url.to_string()
}

fn github_pull_request_from_value(value: &Value) -> ReviewPlatformPullRequest {
    let number = value_i64(value, "number");
    let state = if value_bool(value, "draft") {
        ReviewItemState::Draft
    } else if !value_string(value, "merged_at").is_empty() {
        ReviewItemState::Merged
    } else {
        match value_string(value, "state").as_str() {
            "closed" => ReviewItemState::Closed,
            _ => ReviewItemState::Open,
        }
    };

    ReviewPlatformPullRequest {
        id: number.to_string(),
        number,
        title: value_string(value, "title"),
        state,
        author: nested_string(value, &["user", "login"]),
        source_branch: nested_string(value, &["head", "ref"]),
        target_branch: nested_string(value, &["base", "ref"]),
        updated_at: value_string(value, "updated_at"),
        web_url: value_string(value, "html_url"),
        additions: value_i64(value, "additions") as i32,
        deletions: value_i64(value, "deletions") as i32,
        changed_files: value_i64(value, "changed_files") as i32,
        comments: (value_i64(value, "comments") + value_i64(value, "review_comments")) as i32,
        review_decision: ReviewDecision::Pending,
        checks: empty_checks(),
    }
}

fn gitlab_pull_request_from_value(value: &Value) -> ReviewPlatformPullRequest {
    let number = value_i64(value, "iid");
    let state = if value_bool(value, "draft") || value_bool(value, "work_in_progress") {
        ReviewItemState::Draft
    } else {
        match value_string(value, "state").as_str() {
            "merged" => ReviewItemState::Merged,
            "closed" => ReviewItemState::Closed,
            _ => ReviewItemState::Open,
        }
    };
    let changed_files = value_string(value, "changes_count")
        .parse::<i32>()
        .unwrap_or(0);

    ReviewPlatformPullRequest {
        id: number.to_string(),
        number,
        title: value_string(value, "title"),
        state,
        author: first_non_empty(&[
            nested_string(value, &["author", "username"]),
            nested_string(value, &["author", "name"]),
        ]),
        source_branch: value_string(value, "source_branch"),
        target_branch: value_string(value, "target_branch"),
        updated_at: value_string(value, "updated_at"),
        web_url: value_string(value, "web_url"),
        additions: 0,
        deletions: 0,
        changed_files,
        comments: value_i64(value, "user_notes_count") as i32,
        review_decision: ReviewDecision::Pending,
        checks: empty_checks(),
    }
}

fn gitcode_pull_request_from_value(value: &Value) -> ReviewPlatformPullRequest {
    let number = first_non_zero(&[value_i64(value, "number"), value_i64(value, "id")]);
    let state = match value_string(value, "state").as_str() {
        "merged" => ReviewItemState::Merged,
        "closed" => ReviewItemState::Closed,
        _ => ReviewItemState::Open,
    };
    ReviewPlatformPullRequest {
        id: number.to_string(),
        number,
        title: value_string(value, "title"),
        state,
        author: first_non_empty(&[
            nested_string(value, &["user", "login"]),
            nested_string(value, &["user", "name"]),
            nested_string(value, &["author", "login"]),
        ]),
        source_branch: first_non_empty(&[
            nested_string(value, &["head", "ref"]),
            value_string(value, "head_branch"),
        ]),
        target_branch: first_non_empty(&[
            nested_string(value, &["base", "ref"]),
            value_string(value, "base_branch"),
        ]),
        updated_at: value_string(value, "updated_at"),
        web_url: first_non_empty(&[
            value_string(value, "html_url"),
            value_string(value, "web_url"),
        ]),
        additions: value_i64(value, "additions") as i32,
        deletions: value_i64(value, "deletions") as i32,
        changed_files: value_i64(value, "changed_files") as i32,
        comments: value_i64(value, "comments") as i32,
        review_decision: ReviewDecision::Pending,
        checks: empty_checks(),
    }
}

fn github_file_from_value(value: &Value) -> ReviewPlatformFile {
    ReviewPlatformFile {
        path: value_string(value, "filename"),
        old_path: value
            .get("previous_filename")
            .and_then(Value::as_str)
            .map(str::to_string),
        status: file_status(&value_string(value, "status")),
        additions: value_i64(value, "additions") as i32,
        deletions: value_i64(value, "deletions") as i32,
        patch: optional_string(value, "patch"),
    }
}

fn gitcode_file_from_value(value: &Value) -> ReviewPlatformFile {
    ReviewPlatformFile {
        path: first_non_empty(&[
            value_string(value, "filename"),
            value_string(value, "new_path"),
        ]),
        old_path: value
            .get("previous_filename")
            .and_then(Value::as_str)
            .map(str::to_string),
        status: file_status(&value_string(value, "status")),
        additions: value_i64(value, "additions") as i32,
        deletions: value_i64(value, "deletions") as i32,
        patch: optional_string(value, "patch").or_else(|| optional_string(value, "diff")),
    }
}

fn gitlab_files(value: &Value) -> Vec<ReviewPlatformFile> {
    value
        .get("changes")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .map(|change| {
            let diff = value_string(change, "diff");
            let (additions, deletions) = count_diff_lines(&diff);
            let status = if value_bool(change, "new_file") {
                ReviewFileStatus::Added
            } else if value_bool(change, "deleted_file") {
                ReviewFileStatus::Deleted
            } else if value_bool(change, "renamed_file") {
                ReviewFileStatus::Renamed
            } else {
                ReviewFileStatus::Modified
            };
            ReviewPlatformFile {
                path: value_string(change, "new_path"),
                old_path: change
                    .get("old_path")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status,
                additions,
                deletions,
                patch: Some(diff),
            }
        })
        .collect()
}

fn github_commit_from_value(value: &Value) -> ReviewPlatformCommit {
    let hash = value_string(value, "sha");
    ReviewPlatformCommit {
        short_hash: short_hash(&hash),
        hash,
        title: first_line(&nested_string(value, &["commit", "message"])),
        author: first_non_empty(&[
            nested_string(value, &["author", "login"]),
            nested_string(value, &["commit", "author", "name"]),
        ]),
        committed_at: nested_string(value, &["commit", "author", "date"]),
    }
}

fn gitlab_commit_from_value(value: &Value) -> ReviewPlatformCommit {
    let hash = value_string(value, "id");
    ReviewPlatformCommit {
        short_hash: first_non_empty(&[value_string(value, "short_id"), short_hash(&hash)]),
        hash,
        title: first_non_empty(&[
            value_string(value, "title"),
            first_line(&value_string(value, "message")),
        ]),
        author: value_string(value, "author_name"),
        committed_at: first_non_empty(&[
            value_string(value, "committed_date"),
            value_string(value, "created_at"),
        ]),
    }
}

fn gitcode_commit_from_value(value: &Value) -> ReviewPlatformCommit {
    let hash = first_non_empty(&[value_string(value, "sha"), value_string(value, "id")]);
    ReviewPlatformCommit {
        short_hash: short_hash(&hash),
        hash,
        title: first_non_empty(&[
            nested_string(value, &["commit", "message"])
                .lines()
                .next()
                .unwrap_or_default()
                .to_string(),
            value_string(value, "message"),
        ]),
        author: first_non_empty(&[
            nested_string(value, &["author", "login"]),
            nested_string(value, &["commit", "author", "name"]),
        ]),
        committed_at: first_non_empty(&[
            nested_string(value, &["commit", "author", "date"]),
            value_string(value, "created_at"),
        ]),
    }
}

fn github_review_decision(reviews: &Value) -> ReviewDecision {
    let mut latest_by_author: HashMap<String, String> = HashMap::new();
    let mut anonymous_states = Vec::new();
    for review in array_items(reviews) {
        let state = value_string(review, "state");
        if state == "DISMISSED" || state.trim().is_empty() {
            continue;
        }
        let author = nested_string(review, &["user", "login"]);
        if author.trim().is_empty() {
            anonymous_states.push(state);
        } else {
            latest_by_author.insert(author, state);
        }
    }

    let states = latest_by_author
        .values()
        .chain(anonymous_states.iter())
        .map(String::as_str)
        .collect::<Vec<_>>();

    if states.iter().any(|state| *state == "CHANGES_REQUESTED") {
        return ReviewDecision::ChangesRequested;
    }
    if states.iter().any(|state| *state == "APPROVED") {
        return ReviewDecision::Approved;
    }
    if states.iter().any(|state| *state == "COMMENTED") {
        return ReviewDecision::Commented;
    }
    ReviewDecision::Pending
}

fn github_threads(
    reviews: &Value,
    review_comments: &Value,
    issue_comments: &Value,
) -> Vec<ReviewPlatformThread> {
    let mut threads = Vec::new();
    for review in array_items(reviews) {
        let body = github_review_body(review);
        threads.push(ReviewPlatformThread {
            id: format!("review-{}", value_i64(review, "id")),
            provider_thread_id: None,
            provider_comment_id: value_i64(review, "id")
                .checked_abs()
                .map(|id| id.to_string()),
            kind: ReviewPlatformThreadKind::Review,
            reply_to_provider_comment_id: None,
            file_path: None,
            line: None,
            resolved: false,
            author: nested_string(review, &["user", "login"]),
            body,
            updated_at: first_non_empty(&[
                value_string(review, "submitted_at"),
                value_string(review, "updated_at"),
            ]),
        });
    }
    for comment in array_items(review_comments) {
        threads.push(github_thread_from_review_comment(comment));
    }
    for comment in array_items(issue_comments) {
        threads.push(github_thread_from_issue_comment(comment));
    }
    threads
}

fn github_review_body(review: &Value) -> String {
    let body = value_string(review, "body");
    if !body.trim().is_empty() {
        return body;
    }
    match value_string(review, "state").as_str() {
        "APPROVED" => "Approved this pull request.".to_string(),
        "CHANGES_REQUESTED" => "Requested changes.".to_string(),
        "COMMENTED" => "Submitted a pull request review.".to_string(),
        state if !state.trim().is_empty() => format!("Submitted a {} review.", state),
        _ => "Submitted a pull request review.".to_string(),
    }
}

fn github_thread_from_review_comment(comment: &Value) -> ReviewPlatformThread {
    let comment_id = first_non_empty(&[
        value_string(comment, "id"),
        value_i64(comment, "id").to_string(),
    ]);
    ReviewPlatformThread {
        id: format!("comment-{}", comment_id),
        provider_thread_id: None,
        provider_comment_id: Some(comment_id),
        kind: ReviewPlatformThreadKind::Comment,
        reply_to_provider_comment_id: value_i64(comment, "in_reply_to_id")
            .checked_abs()
            .map(|id| id.to_string())
            .or_else(|| {
                comment
                    .get("in_reply_to_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        file_path: comment
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string),
        line: comment
            .get("line")
            .and_then(Value::as_i64)
            .or_else(|| comment.get("original_line").and_then(Value::as_i64)),
        resolved: false,
        author: nested_string(comment, &["user", "login"]),
        body: value_string(comment, "body"),
        updated_at: value_string(comment, "updated_at"),
    }
}

fn github_thread_from_issue_comment(comment: &Value) -> ReviewPlatformThread {
    let comment_id = first_non_empty(&[
        value_string(comment, "id"),
        value_i64(comment, "id").to_string(),
    ]);
    ReviewPlatformThread {
        id: format!("issue-comment-{}", comment_id),
        provider_thread_id: None,
        provider_comment_id: Some(comment_id),
        kind: ReviewPlatformThreadKind::Comment,
        reply_to_provider_comment_id: None,
        file_path: None,
        line: None,
        resolved: false,
        author: nested_string(comment, &["user", "login"]),
        body: value_string(comment, "body"),
        updated_at: value_string(comment, "updated_at"),
    }
}

fn gitlab_threads(discussions: &Value, notes: &Value) -> Vec<ReviewPlatformThread> {
    let mut threads = Vec::new();
    let mut seen_comment_ids = HashSet::new();
    for discussion in array_items(discussions) {
        let discussion_id = value_string(discussion, "id");
        let resolved = value_bool(discussion, "resolved");
        let discussion_notes = discussion
            .get("notes")
            .and_then(Value::as_array)
            .map(|notes| notes.as_slice())
            .unwrap_or(&[]);
        let mut root_comment_id: Option<String> = None;
        for (index, note) in discussion_notes.iter().enumerate() {
            let kind = if index == 0 {
                ReviewPlatformThreadKind::Review
            } else {
                ReviewPlatformThreadKind::Comment
            };
            let reply_to = if index == 0 {
                None
            } else {
                root_comment_id.clone()
            };
            let thread = gitlab_thread_from_note(
                note,
                Some(discussion_id.clone()),
                resolved,
                kind,
                reply_to,
            );
            if root_comment_id.is_none() {
                root_comment_id = thread.provider_comment_id.clone();
            }
            if let Some(comment_id) = thread.provider_comment_id.clone() {
                seen_comment_ids.insert(comment_id);
            }
            threads.push(thread);
        }
    }
    for note in array_items(notes) {
        let thread = gitlab_thread_from_note(
            note,
            None,
            false,
            ReviewPlatformThreadKind::Comment,
            None,
        );
        if let Some(comment_id) = thread.provider_comment_id.as_ref() {
            if seen_comment_ids.contains(comment_id) {
                continue;
            }
            seen_comment_ids.insert(comment_id.clone());
        }
        threads.push(thread);
    }
    threads
}

fn gitlab_thread_from_note(
    note: &Value,
    discussion_id: Option<String>,
    discussion_resolved: bool,
    kind: ReviewPlatformThreadKind,
    reply_to_provider_comment_id: Option<String>,
) -> ReviewPlatformThread {
    let note_id = value_string(note, "id");
    let id = match discussion_id.as_deref() {
        Some(discussion_id) if !discussion_id.trim().is_empty() => {
            format!("discussion-{}:note-{}", discussion_id, note_id)
        }
        _ => format!("note-{}", note_id),
    };

    ReviewPlatformThread {
        id,
        provider_thread_id: discussion_id,
        provider_comment_id: Some(note_id),
        kind,
        reply_to_provider_comment_id,
        file_path: nested_optional_string(note, &["position", "new_path"])
            .or_else(|| nested_optional_string(note, &["position", "old_path"])),
        line: note
            .pointer("/position/new_line")
            .and_then(Value::as_i64)
            .or_else(|| note.pointer("/position/old_line").and_then(Value::as_i64)),
        resolved: discussion_resolved || value_bool(note, "resolved"),
        author: first_non_empty(&[
            nested_string(note, &["author", "username"]),
            nested_string(note, &["author", "name"]),
        ]),
        body: value_string(note, "body"),
        updated_at: first_non_empty(&[
            value_string(note, "updated_at"),
            value_string(note, "created_at"),
        ]),
    }
}

fn parse_provider_comment_id(thread_id: &str) -> Option<&str> {
    let trimmed = thread_id.trim();
    trimmed
        .strip_prefix("comment-")
        .or_else(|| trimmed.strip_prefix("note-"))
        .or_else(|| trimmed.split_once(":note-").map(|(_, note_id)| note_id))
        .or_else(|| {
            if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
                Some(trimmed)
            } else {
                None
            }
        })
        .filter(|value| !value.trim().is_empty())
}

fn parse_provider_thread_id(thread_id: &str) -> Option<&str> {
    let trimmed = thread_id.trim();
    trimmed
        .strip_prefix("discussion-")
        .map(|value| {
            value
                .split_once(":note-")
                .map(|(id, _)| id)
                .unwrap_or(value)
        })
        .or_else(|| {
            if trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                Some(trimmed)
            } else {
                None
            }
        })
        .filter(|value| !value.trim().is_empty())
}

fn gitcode_threads(value: &Value) -> Vec<ReviewPlatformThread> {
    array_items(value)
        .iter()
        .map(|comment| ReviewPlatformThread {
            id: value_string(comment, "id"),
            provider_thread_id: None,
            provider_comment_id: Some(value_string(comment, "id")),
            kind: ReviewPlatformThreadKind::Comment,
            reply_to_provider_comment_id: comment
                .get("in_reply_to_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    comment
                        .get("in_reply_to_id")
                        .and_then(Value::as_i64)
                        .map(|id| id.to_string())
                }),
            file_path: comment
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string),
            line: comment.get("line").and_then(Value::as_i64),
            resolved: false,
            author: first_non_empty(&[
                nested_string(comment, &["user", "login"]),
                nested_string(comment, &["user", "name"]),
            ]),
            body: value_string(comment, "body"),
            updated_at: first_non_empty(&[
                value_string(comment, "updated_at"),
                value_string(comment, "created_at"),
            ]),
        })
        .collect()
}

fn empty_checks() -> ReviewChecks {
    ReviewChecks {
        total: 0,
        passed: 0,
        failed: 0,
        pending: 0,
    }
}

fn file_status(status: &str) -> ReviewFileStatus {
    match status {
        "added" | "new" => ReviewFileStatus::Added,
        "removed" | "deleted" => ReviewFileStatus::Deleted,
        "renamed" => ReviewFileStatus::Renamed,
        _ => ReviewFileStatus::Modified,
    }
}

fn count_diff_lines(diff: &str) -> (i32, i32) {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    (additions, deletions)
}

fn array_items<'a>(value: &'a Value) -> &'a [Value] {
    value
        .as_array()
        .map(|items| items.as_slice())
        .unwrap_or(&[])
}

fn value_string(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn nested_string(value: &Value, path: &[&str]) -> String {
    nested_optional_string(value, path).unwrap_or_default()
}

fn nested_optional_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    match current {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn value_i64(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn value_bool(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_str().map(|text| text.eq_ignore_ascii_case("true")))
        })
        .unwrap_or(false)
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn first_non_zero(values: &[i64]) -> i64 {
    values
        .iter()
        .copied()
        .find(|value| *value != 0)
        .unwrap_or(0)
}

fn first_line(value: &str) -> String {
    value.lines().next().unwrap_or_default().to_string()
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn github_review_decision_uses_latest_review_per_author() {
        let reviews = json!([
            {
                "id": 1,
                "state": "CHANGES_REQUESTED",
                "user": { "login": "alice" }
            },
            {
                "id": 2,
                "state": "APPROVED",
                "user": { "login": "alice" }
            }
        ]);

        assert_eq!(github_review_decision(&reviews), ReviewDecision::Approved);
    }

    #[test]
    fn github_review_decision_keeps_active_change_request_from_any_reviewer() {
        let reviews = json!([
            {
                "id": 1,
                "state": "APPROVED",
                "user": { "login": "alice" }
            },
            {
                "id": 2,
                "state": "CHANGES_REQUESTED",
                "user": { "login": "bob" }
            }
        ]);

        assert_eq!(
            github_review_decision(&reviews),
            ReviewDecision::ChangesRequested
        );
    }

    #[test]
    fn github_threads_include_issue_comments_and_review_comments() {
        let reviews = json!([]);
        let review_comments = json!([
            {
                "id": 10,
                "path": "src/lib.rs",
                "line": 8,
                "user": { "login": "alice" },
                "body": "Inline comment",
                "updated_at": "2026-05-18T01:00:00Z"
            }
        ]);
        let issue_comments = json!([
            {
                "id": 20,
                "user": { "login": "bob" },
                "body": "Conversation comment",
                "updated_at": "2026-05-18T02:00:00Z"
            }
        ]);

        let threads = github_threads(&reviews, &review_comments, &issue_comments);

        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "comment-10");
        assert_eq!(threads[0].file_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(threads[1].id, "issue-comment-20");
        assert_eq!(threads[1].file_path, None);
        assert_eq!(threads[1].body, "Conversation comment");
    }

    #[test]
    fn github_threads_keep_empty_body_reviews_visible() {
        let reviews = json!([
            {
                "id": 30,
                "state": "APPROVED",
                "user": { "login": "alice" },
                "body": "",
                "submitted_at": "2026-05-18T03:00:00Z"
            }
        ]);

        let threads = github_threads(&reviews, &json!([]), &json!([]));

        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "review-30");
        assert_eq!(threads[0].body, "Approved this pull request.");
    }

    #[test]
    fn github_review_comment_replies_track_parent_comment() {
        let threads = github_threads(
            &json!([]),
            &json!([
                {
                    "id": 40,
                    "in_reply_to_id": 10,
                    "user": { "login": "alice" },
                    "body": "Reply",
                    "updated_at": "2026-05-18T04:30:00Z"
                }
            ]),
            &json!([]),
        );

        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].kind, ReviewPlatformThreadKind::Comment);
        assert_eq!(
            threads[0].reply_to_provider_comment_id.as_deref(),
            Some("10")
        );
    }

    #[test]
    fn gitlab_threads_include_top_level_notes_without_duplication() {
        let discussions = json!([
            {
                "id": "discussion-1",
                "resolved": false,
                "notes": [
                    {
                        "id": "100",
                        "author": { "username": "alice" },
                        "body": "Inline note",
                        "updated_at": "2026-05-18T04:00:00Z",
                        "position": { "new_path": "src/lib.rs", "new_line": 12 }
                    }
                ]
            }
        ]);
        let notes = json!([
            {
                "id": "100",
                "author": { "username": "alice" },
                "body": "Inline note",
                "updated_at": "2026-05-18T04:00:00Z",
                "position": { "new_path": "src/lib.rs", "new_line": 12 }
            },
            {
                "id": "200",
                "author": { "username": "bob" },
                "body": "Top-level note",
                "updated_at": "2026-05-18T05:00:00Z"
            }
        ]);

        let threads = gitlab_threads(&discussions, &notes);

        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "discussion-discussion-1:note-100");
        assert_eq!(threads[1].id, "note-200");
        assert_eq!(threads[1].file_path, None);
        assert_eq!(threads[1].body, "Top-level note");
    }

    #[test]
    fn gitlab_discussion_threads_mark_root_as_review_and_replies_as_comments() {
        let discussions = json!([
            {
                "id": "discussion-2",
                "resolved": false,
                "notes": [
                    {
                        "id": "300",
                        "author": { "username": "alice" },
                        "body": "Root note",
                        "updated_at": "2026-05-18T06:00:00Z"
                    },
                    {
                        "id": "301",
                        "author": { "username": "bob" },
                        "body": "Reply note",
                        "updated_at": "2026-05-18T06:05:00Z"
                    }
                ]
            }
        ]);

        let threads = gitlab_threads(&discussions, &json!([]));

        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].kind, ReviewPlatformThreadKind::Review);
        assert_eq!(threads[0].reply_to_provider_comment_id, None);
        assert_eq!(threads[1].kind, ReviewPlatformThreadKind::Comment);
        assert_eq!(
            threads[1].reply_to_provider_comment_id.as_deref(),
            Some("300")
        );
    }
}
