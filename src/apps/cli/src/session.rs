/// Session management module
///
/// Responsible for creating, saving, loading and managing chat sessions
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::CliConfig;

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session ID
    pub id: String,
    /// Session title
    pub title: String,
    /// Created time
    pub created_at: DateTime<Utc>,
    /// Updated time
    pub updated_at: DateTime<Utc>,
    /// Workspace path
    pub workspace: Option<String>,
    /// Agent used
    pub agent: String,
    /// Message list
    pub messages: Vec<Message>,
    /// Metadata
    pub metadata: SessionMetadata,
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    /// Message count
    pub message_count: usize,
    /// Tool call count
    pub tool_calls: usize,
    /// Files modified count
    pub files_modified: usize,
    /// Tags
    pub tags: Vec<String>,
}

/// Message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message ID
    pub id: String,
    /// Role (user, assistant, system)
    pub role: String,
    /// Content (for simple text messages)
    pub content: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Flow items (mixed text and tools in order)
    #[serde(default)]
    pub flow_items: Vec<FlowItem>,
}

/// Flow item (inspired by flowchat architecture)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FlowItem {
    /// Text block
    #[serde(rename = "text")]
    Text {
        /// Content
        content: String,
        /// Whether currently streaming
        #[serde(default)]
        is_streaming: bool,
    },
    /// Tool call
    #[serde(rename = "tool")]
    Tool {
        /// Tool call details
        tool_call: ToolCall,
    },
}

/// Tool call record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool ID
    pub tool_id: Option<String>,
    /// Tool name
    pub tool_name: String,
    /// Tool parameters
    pub parameters: serde_json::Value,
    /// Execution result
    pub result: Option<String>,
    /// Execution status
    pub status: ToolCallStatus,
    /// Progress percentage (0.0 - 1.0)
    pub progress: Option<f32>,
    /// Progress message
    pub progress_message: Option<String>,
    /// Execution duration (milliseconds)
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCallStatus {
    /// Early detected
    EarlyDetected,
    /// Parameters partially parsed
    ParamsPartial,
    /// Queued
    Queued,
    /// Waiting for dependencies
    Waiting,
    /// Confirmation needed
    ConfirmationNeeded,
    /// Confirmed
    Confirmed,
    /// Rejected
    Rejected,
    /// Pending execution
    Pending,
    /// Running
    Running,
    /// Streaming output
    Streaming,
    /// Execution successful
    Success,
    /// Execution failed
    Failed,
    /// Cancelled
    Cancelled,
}

impl Session {
    /// Create new session
    pub fn new(agent: String, workspace: Option<String>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        Self {
            id: id.clone(),
            title: format!("Session {}", now.format("%m-%d %H:%M")),
            created_at: now,
            updated_at: now,
            workspace,
            agent,
            messages: Vec::new(),
            metadata: SessionMetadata::default(),
        }
    }

    /// Add message
    pub fn add_message(&mut self, role: String, content: String) {
        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            content,
            timestamp: Utc::now(),
            flow_items: Vec::new(),
        };

        self.messages.push(message);
        self.metadata.message_count = self.messages.len();
        self.updated_at = Utc::now();
    }

    /// Add or update text flow of the last message
    pub fn update_last_message_text_flow(&mut self, content: String, is_streaming: bool) {
        if let Some(last_message) = self.messages.last_mut() {
            if let Some(FlowItem::Text {
                content: ref mut c,
                is_streaming: ref mut s,
            }) = last_message.flow_items.last_mut()
            {
                *c = content.clone();
                *s = is_streaming;
            } else {
                last_message.flow_items.push(FlowItem::Text {
                    content: content.clone(),
                    is_streaming,
                });
            }
            last_message.content = content;
            self.updated_at = Utc::now();
        }
    }

    /// Add tool call to the last message
    pub fn add_tool_to_last_message(&mut self, tool_call: ToolCall) {
        if let Some(last_message) = self.messages.last_mut() {
            last_message.flow_items.push(FlowItem::Tool { tool_call });
            self.metadata.tool_calls += 1;
            self.updated_at = Utc::now();
        }
    }

    /// Update tool call status in the last message
    pub fn update_tool_in_last_message(
        &mut self,
        tool_id: &str,
        update_fn: impl FnOnce(&mut ToolCall),
    ) {
        if let Some(last_message) = self.messages.last_mut() {
            for item in last_message.flow_items.iter_mut() {
                if let FlowItem::Tool { tool_call } = item {
                    if tool_call.tool_id.as_deref() == Some(tool_id) {
                        update_fn(tool_call);
                        break;
                    }
                }
            }
            self.updated_at = Utc::now();
        }
    }

    /// Save session
    pub fn save(&self) -> Result<()> {
        let sessions_dir = CliConfig::sessions_dir()?;
        let session_file = sessions_dir.join(format!("{}.json", self.id));
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&session_file, content)?;
        tracing::debug!("Saved session: {}", self.id);
        Ok(())
    }

    /// Load session
    pub fn load(id: &str) -> Result<Self> {
        let sessions_dir = CliConfig::sessions_dir()?;
        let session_file = sessions_dir.join(format!("{}.json", id));

        if !session_file.exists() {
            anyhow::bail!("Session not found: {}", id);
        }

        let content = fs::read_to_string(&session_file)?;
        let session: Self = serde_json::from_str(&content)?;
        tracing::info!(
            "Loaded session: {} ({} messages)",
            session.title,
            session.messages.len()
        );
        Ok(session)
    }

    /// List all sessions
    pub fn list_all() -> Result<Vec<SessionInfo>> {
        let sessions_dir = CliConfig::sessions_dir()?;

        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in fs::read_dir(sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            match Self::load_info(&path) {
                Ok(info) => sessions.push(info),
                Err(e) => {
                    tracing::warn!("Failed to load session info {:?}: {}", path, e);
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    fn load_info(path: &PathBuf) -> Result<SessionInfo> {
        let content = fs::read_to_string(path)?;
        let session: Self = serde_json::from_str(&content)?;

        Ok(SessionInfo {
            id: session.id,
            title: session.title,
            created_at: session.created_at,
            updated_at: session.updated_at,
            agent: session.agent,
            message_count: session.metadata.message_count,
            workspace: session.workspace,
        })
    }

    /// Delete session
    pub fn delete(id: &str) -> Result<()> {
        let sessions_dir = CliConfig::sessions_dir()?;
        let session_file = sessions_dir.join(format!("{}.json", id));

        if session_file.exists() {
            fs::remove_file(session_file)?;
            tracing::info!("Deleted session: {}", id);
        }

        Ok(())
    }

    /// Get most recent session
    pub fn get_last() -> Result<Option<Self>> {
        let sessions = Self::list_all()?;

        if let Some(info) = sessions.first() {
            Ok(Some(Self::load(&info.id)?))
        } else {
            Ok(None)
        }
    }
}

/// Session info (lightweight)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub agent: String,
    pub message_count: usize,
    pub workspace: Option<String>,
}
