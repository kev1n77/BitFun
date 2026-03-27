//! AI client implementation.
//!
//! The client module acts as a small facade:
//! - `client/*` holds shared transport and aggregation utilities
//! - `providers/*` owns provider-specific request/response adaptation

pub(crate) mod format;
pub(crate) mod healthcheck;
pub(crate) mod http;
pub(crate) mod quirks;
pub(crate) mod response_aggregator;
pub(crate) mod sse;
pub(crate) mod utils;

use crate::providers::{anthropic, gemini, openai};
use crate::types::ProxyConfig;
use crate::types::*;
use anyhow::Result;
use format::ApiFormat;
use reqwest::Client;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;

/// Streamed response result with the parsed stream and optional raw SSE receiver.
pub struct StreamResponse {
    pub stream: Pin<Box<dyn futures::Stream<Item = Result<crate::stream::UnifiedResponse>> + Send>>,
    pub raw_sse_rx: Option<mpsc::UnboundedReceiver<String>>,
}

/// Runtime stream behavior shared across provider implementations.
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    /// Maximum idle time between streamed chunks. `None` means wait indefinitely.
    pub idle_timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct AIClient {
    pub(crate) client: Client,
    pub config: AIConfig,
    pub(crate) stream_options: StreamOptions,
}

impl AIClient {
    pub(crate) const TEST_IMAGE_EXPECTED_CODE: &'static str = "BYGR";
    pub(crate) const TEST_IMAGE_PNG_BASE64: &'static str =
        "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAIAAADTED8xAAACBklEQVR42u3ZsREAIAwDMYf9dw4txwJupI7Wua+YZEPBfO91h4ZjAgQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABIAAQAAgABAACAAEAAIAAYAAQAAgABAACAAEAAIAAYAAQAAgABAAAAAAAEDRZI3QGf7jDvEPAAIAAYAAQAAgABAACAAEAAIAAYAAQAAgABAACAAEAAIAAYAAQAAgABAACAABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAACAAEAAIAAQAAgABgABAAAjABAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQAAgABAACAAGAAEAAIAAQALwuLkoG8OSfau4AAAAASUVORK5CYII=";
    pub(crate) const STREAM_CONNECT_TIMEOUT_SECS: u64 = 10;
    pub(crate) const HTTP_POOL_IDLE_TIMEOUT_SECS: u64 = 30;
    pub(crate) const HTTP_TCP_KEEPALIVE_SECS: u64 = 60;

    /// Create an AIClient without proxy.
    pub fn new(config: AIConfig) -> Self {
        Self::new_with_runtime_options(config, None, StreamOptions::default())
    }

    /// Create an AIClient with proxy configuration.
    pub fn new_with_proxy(config: AIConfig, proxy_config: Option<ProxyConfig>) -> Self {
        Self::new_with_runtime_options(config, proxy_config, StreamOptions::default())
    }

    /// Create an AIClient with proxy and runtime stream options.
    pub fn new_with_runtime_options(
        config: AIConfig,
        proxy_config: Option<ProxyConfig>,
        stream_options: StreamOptions,
    ) -> Self {
        let client = http::create_http_client(proxy_config, config.skip_ssl_verify);
        Self {
            client,
            config,
            stream_options,
        }
    }

    /// Returns the configured idle timeout between streamed chunks, if any.
    pub fn stream_idle_timeout(&self) -> Option<Duration> {
        self.stream_options.idle_timeout
    }

    pub async fn send_message_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<StreamResponse> {
        let custom_body = self.config.custom_request_body.clone();
        self.send_message_stream_with_extra_body(messages, tools, custom_body)
            .await
    }

    pub async fn send_message_stream_with_extra_body(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
    ) -> Result<StreamResponse> {
        let max_tries = 3;
        match ApiFormat::parse(&self.config.format)? {
            ApiFormat::OpenAIChat => {
                openai::chat::send_stream(self, messages, tools, extra_body, max_tries).await
            }
            ApiFormat::OpenAIResponses => {
                openai::responses::send_stream(self, messages, tools, extra_body, max_tries).await
            }
            ApiFormat::Anthropic => {
                anthropic::request::send_stream(self, messages, tools, extra_body, max_tries).await
            }
            ApiFormat::Gemini => {
                gemini::request::send_stream(self, messages, tools, extra_body, max_tries).await
            }
            ApiFormat::GeminiCodeAssist => {
                gemini::code_assist::send_stream(self, messages, tools, extra_body, max_tries)
                    .await
            }
        }
    }

    pub async fn send_message(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<GeminiResponse> {
        let custom_body = self.config.custom_request_body.clone();
        self.send_message_with_extra_body(messages, tools, custom_body)
            .await
    }

    pub async fn send_message_with_extra_body(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
    ) -> Result<GeminiResponse> {
        let stream_response = self
            .send_message_stream_with_extra_body(messages, tools, extra_body)
            .await?;
        response_aggregator::aggregate_stream_response(stream_response).await
    }

    pub async fn test_connection(&self) -> Result<ConnectionTestResult> {
        healthcheck::test_connection(self).await
    }

    pub async fn test_image_input_connection(&self) -> Result<ConnectionTestResult> {
        healthcheck::test_image_input_connection(self).await
    }

    pub async fn list_models(&self) -> Result<Vec<RemoteModelInfo>> {
        match ApiFormat::parse(&self.config.format)? {
            ApiFormat::OpenAIChat | ApiFormat::OpenAIResponses => {
                openai::common::list_models(self).await
            }
            ApiFormat::Anthropic => anthropic::discovery::list_models(self).await,
            ApiFormat::Gemini => gemini::discovery::list_models(self).await,
            ApiFormat::GeminiCodeAssist => gemini::code_assist::list_models(self).await,
        }
    }
}
