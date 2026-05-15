//! Platform-neutral pull request review data service.
//!
//! This module owns provider detection, token handling, and provider-specific
//! HTTP calls. UI and desktop adapters consume only the common DTOs below.

use crate::infrastructure::try_get_path_manager_arc;
use crate::service::git::{execute_git_command, get_repository_root};
use futures::future::join_all;
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;

const USER_AGENT_VALUE: &str = "BitFun";
const DEFAULT_PR_PAGE: u32 = 1;
const DEFAULT_PR_PAGE_SIZE: u32 = 10;
const MAX_PR_PAGE_SIZE: u32 = 50;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPlatformThread {
    pub id: String,
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
    pub can_reply_to_thread: bool,
    pub can_resolve_thread: bool,
    pub can_merge: bool,
    pub supports_draft_review: bool,
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
        })
    }

    pub async fn pull_request_detail(
        repository_path: &str,
        remote_id: &str,
        pull_request_id: &str,
    ) -> Result<ReviewPlatformPullRequestDetail, ReviewPlatformError> {
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
trait ReviewProvider {
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
}

struct GithubProvider;
struct GitlabProvider;
struct GitcodeProvider;
struct UnsupportedProvider;

fn provider_for(platform: ReviewPlatformKind) -> &'static dyn ReviewProvider {
    match platform {
        ReviewPlatformKind::Github => &GithubProvider,
        ReviewPlatformKind::Gitlab => &GitlabProvider,
        ReviewPlatformKind::Codehub => &GitlabProvider,
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
        let total =
            github_total_count(ctx, &url, &response.headers, pagination, items.len()).await?;
        let has_next = total
            .map(|total| u64::from(pagination.page) * u64::from(pagination.per_page) < total)
            .unwrap_or_else(|| link_header_has_rel(&response.headers, "next"));

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
        let files = send_json(
            github_request(
                client.clone(),
                &format!("{}/files", base),
                ctx.token.as_deref(),
            )
            .query(&[("per_page", "100")]),
        )
        .await?;
        let commits = send_json(
            github_request(
                client.clone(),
                &format!("{}/commits", base),
                ctx.token.as_deref(),
            )
            .query(&[("per_page", "100")]),
        )
        .await?;
        let reviews = send_json(
            github_request(
                client.clone(),
                &format!("{}/reviews", base),
                ctx.token.as_deref(),
            )
            .query(&[("per_page", "100")]),
        )
        .await?;
        let comments = send_json(
            github_request(client, &format!("{}/comments", base), ctx.token.as_deref())
                .query(&[("per_page", "100")]),
        )
        .await?;

        let mut pull_request = github_pull_request_from_value(&detail);
        pull_request.review_decision = github_review_decision(&reviews);

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
            threads: github_threads(&reviews, &comments),
        })
    }
}

#[async_trait::async_trait]
impl ReviewProvider for GitlabProvider {
    async fn list_pull_requests(
        &self,
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

    async fn pull_request_detail(
        &self,
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
        let commits = send_json(gitlab_request(
            client.clone(),
            &format!("{}/commits", base),
            ctx.token.as_deref(),
        ))
        .await?;
        let discussions = send_json(gitlab_request(
            client,
            &format!("{}/discussions", base),
            ctx.token.as_deref(),
        ))
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
            threads: gitlab_threads(&discussions),
        })
    }
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
        let files = send_json(gitcode_request(
            client.clone(),
            &format!("{}/files", base),
            ctx.token.as_deref(),
        ))
        .await
        .unwrap_or(Value::Array(Vec::new()));
        let commits = send_json(gitcode_request(
            client.clone(),
            &format!("{}/commits", base),
            ctx.token.as_deref(),
        ))
        .await
        .unwrap_or(Value::Array(Vec::new()));
        let comments = send_json(gitcode_request(
            client,
            &format!("{}/comments", base),
            ctx.token.as_deref(),
        ))
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

async fn github_total_count(
    ctx: &ProviderContext,
    url: &str,
    headers: &HeaderMap,
    pagination: PullRequestPagination,
    current_count: usize,
) -> Result<Option<u64>, ReviewPlatformError> {
    let Some(last_page) = link_header_last_page(headers) else {
        return Ok(Some(
            u64::from(pagination.page.saturating_sub(1)) * u64::from(pagination.per_page)
                + current_count as u64,
        ));
    };

    if last_page <= pagination.page {
        return Ok(Some(
            u64::from(pagination.page.saturating_sub(1)) * u64::from(pagination.per_page)
                + current_count as u64,
        ));
    }

    let per_page = pagination.per_page.to_string();
    let page = last_page.to_string();
    let response = send_json(
        github_request(http_client()?, url, ctx.token.as_deref()).query(&[
            ("state", "all"),
            ("per_page", &per_page),
            ("page", &page),
        ]),
    )
    .await?;
    let last_count = response.as_array().map(Vec::len).ok_or_else(|| {
        ReviewPlatformError::Parse("GitHub last pull response was not an array".to_string())
    })?;

    Ok(Some(
        u64::from(last_page.saturating_sub(1)) * u64::from(pagination.per_page) + last_count as u64,
    ))
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
    join_all(futures).await
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
    join_all(futures).await
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
    join_all(futures).await
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
            can_reply_to_thread: false,
            can_resolve_thread: false,
            can_merge: false,
            supports_draft_review: false,
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

fn capabilities_for_remote(remote: &ReviewPlatformRemote) -> ReviewPlatformCapabilities {
    let has_token = matches!(
        remote.auth_source,
        ReviewAuthSource::Env | ReviewAuthSource::Stored
    );
    ReviewPlatformCapabilities {
        can_create_review: has_token,
        can_reply_to_thread: has_token,
        can_resolve_thread: has_token
            && matches!(
                remote.platform,
                ReviewPlatformKind::Gitlab | ReviewPlatformKind::Codehub
            ),
        can_merge: false,
        supports_draft_review: remote.platform == ReviewPlatformKind::Github,
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
    let mut decision = ReviewDecision::Pending;
    for review in array_items(reviews) {
        match value_string(review, "state").as_str() {
            "CHANGES_REQUESTED" => return ReviewDecision::ChangesRequested,
            "APPROVED" => decision = ReviewDecision::Approved,
            "COMMENTED" if decision == ReviewDecision::Pending => {
                decision = ReviewDecision::Commented
            }
            _ => {}
        }
    }
    decision
}

fn github_threads(reviews: &Value, comments: &Value) -> Vec<ReviewPlatformThread> {
    let mut threads = Vec::new();
    for review in array_items(reviews) {
        let body = value_string(review, "body");
        if body.trim().is_empty() {
            continue;
        }
        threads.push(ReviewPlatformThread {
            id: format!("review-{}", value_i64(review, "id")),
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
    for comment in array_items(comments) {
        threads.push(ReviewPlatformThread {
            id: format!("comment-{}", value_i64(comment, "id")),
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
        });
    }
    threads
}

fn gitlab_threads(value: &Value) -> Vec<ReviewPlatformThread> {
    let mut threads = Vec::new();
    for discussion in array_items(value) {
        let resolved = value_bool(discussion, "resolved");
        let notes = discussion
            .get("notes")
            .and_then(Value::as_array)
            .map(|notes| notes.as_slice())
            .unwrap_or(&[]);
        for note in notes {
            threads.push(ReviewPlatformThread {
                id: value_string(note, "id"),
                file_path: nested_optional_string(note, &["position", "new_path"])
                    .or_else(|| nested_optional_string(note, &["position", "old_path"])),
                line: note
                    .pointer("/position/new_line")
                    .and_then(Value::as_i64)
                    .or_else(|| note.pointer("/position/old_line").and_then(Value::as_i64)),
                resolved: resolved || value_bool(note, "resolved"),
                author: first_non_empty(&[
                    nested_string(note, &["author", "username"]),
                    nested_string(note, &["author", "name"]),
                ]),
                body: value_string(note, "body"),
                updated_at: first_non_empty(&[
                    value_string(note, "updated_at"),
                    value_string(note, "created_at"),
                ]),
            });
        }
    }
    threads
}

fn gitcode_threads(value: &Value) -> Vec<ReviewPlatformThread> {
    array_items(value)
        .iter()
        .map(|comment| ReviewPlatformThread {
            id: value_string(comment, "id"),
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
