//! AI client implementation.
//!
//! The client module now acts as a small facade:
//! - `client/*` holds shared transport and aggregation utilities
//! - `providers/*` owns provider-specific request/response adaptation

pub(crate) mod format;
pub(crate) mod healthcheck;
pub(crate) mod http;
pub(crate) mod quirks;
pub(crate) mod response_aggregator;
pub(crate) mod sse;
pub(crate) mod utils;

use crate::infrastructure::ai::providers::anthropic::AnthropicMessageConverter;
use crate::infrastructure::ai::providers::gemini::GeminiMessageConverter;
use crate::infrastructure::ai::providers::openai::OpenAIMessageConverter;
use crate::infrastructure::ai::providers::{anthropic, gemini, openai};
use crate::infrastructure::telemetry::{get_global_telemetry, TelemetryRequestSpan};
use crate::service::config::ProxyConfig;
use crate::util::types::*;
use ai_stream_handlers::{
    handle_anthropic_stream, handle_gemini_stream, handle_openai_stream, handle_responses_stream,
    UnifiedResponse,
};
use anyhow::{anyhow, Result};
use format::ApiFormat;
use log::{debug, error, warn};
use opentelemetry::KeyValue;
use reqwest::Client;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// Streamed response result with the parsed stream and optional raw SSE receiver.
pub struct StreamResponse {
    pub stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<ai_stream_handlers::UnifiedResponse>> + Send>,
    >,
    pub raw_sse_rx: Option<mpsc::UnboundedReceiver<String>>,
}

#[derive(Debug, Clone, Default)]
struct ModelRequestTelemetryMeta {
    retry_count: usize,
    status_code: Option<u16>,
    error_type: Option<String>,
}

#[derive(Debug)]
struct ModelRequestFailure {
    error: anyhow::Error,
    telemetry: ModelRequestTelemetryMeta,
}

struct TelemetryStream {
    inner: Pin<Box<dyn futures::Stream<Item = Result<UnifiedResponse>> + Send>>,
    span: Option<TelemetryRequestSpan>,
}

impl TelemetryStream {
    fn new(
        inner: Pin<Box<dyn futures::Stream<Item = Result<UnifiedResponse>> + Send>>,
        span: TelemetryRequestSpan,
    ) -> Self {
        Self {
            inner,
            span: Some(span),
        }
    }
}

impl futures::Stream for TelemetryStream {
    type Item = Result<UnifiedResponse>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if let Some(span) = self.span.as_mut() {
                    if let Some(reason) = chunk.finish_reason.clone() {
                        span.add_attribute(KeyValue::new("finish_reason", reason));
                    }

                    if let Some(usage) = chunk.usage.as_ref() {
                        span.add_attribute(KeyValue::new(
                            "total_tokens",
                            usage.total_token_count as i64,
                        ));
                        span.add_attribute(KeyValue::new(
                            "input_tokens",
                            usage.prompt_token_count as i64,
                        ));
                        span.add_attribute(KeyValue::new(
                            "output_tokens",
                            usage.candidates_token_count as i64,
                        ));
                    }
                }

                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                if let Some(mut span) = self.span.take() {
                    span.mark_error(error.to_string());
                }
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                if let Some(mut span) = self.span.take() {
                    span.mark_success();
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn attach_model_request_telemetry(
    span: &mut TelemetryRequestSpan,
    telemetry: &ModelRequestTelemetryMeta,
) {
    span.add_attribute(KeyValue::new(
        "retry_count",
        telemetry.retry_count as i64,
    ));

    if let Some(status_code) = telemetry.status_code {
        span.add_attribute(KeyValue::new("status_code", status_code as i64));
    }

    if let Some(error_type) = telemetry.error_type.as_ref() {
        span.add_attribute(KeyValue::new("error_type", error_type.clone()));
    }
}

fn classify_status_error(status: reqwest::StatusCode) -> &'static str {
    match status.as_u16() {
        429 => "rate_limit",
        500..=599 => "server_error",
        400..=499 => "client_error",
        _ => "http_error",
    }
}

#[derive(Debug, Clone)]
pub struct AIClient {
    pub(crate) client: Client,
    pub config: AIConfig,
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
        let client = http::create_http_client(None, config.skip_ssl_verify);
        Self { client, config }
    }

    /// Create an AIClient with proxy configuration.
    pub fn new_with_proxy(config: AIConfig, proxy_config: Option<ProxyConfig>) -> Self {
        let client = http::create_http_client(proxy_config, config.skip_ssl_verify);
        Self { client, config }
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
        let tool_count = tools.as_ref().map(|entries| entries.len()).unwrap_or(0);
        let api_format = self.get_api_format().to_string();
        let span = get_global_telemetry().and_then(|telemetry| {
            telemetry.start_request_span(
                "model_request",
                vec![
                    KeyValue::new("provider", self.config.name.clone()),
                    KeyValue::new("model", self.config.model.clone()),
                    KeyValue::new("api_format", api_format),
                    KeyValue::new("stream", true),
                    KeyValue::new("message_count", messages.len() as i64),
                    KeyValue::new("tool_count", tool_count as i64),
                ],
            )
        });

        let max_tries = 3;
        let result = match ApiFormat::parse(&self.config.format) {
            Ok(ApiFormat::OpenAIChat) => {
                self.send_openai_stream(messages, tools, extra_body, max_tries)
                    .await
            }
            Ok(ApiFormat::OpenAIResponses) => {
                self.send_responses_stream(messages, tools, extra_body, max_tries)
                    .await
            }
            Ok(ApiFormat::Anthropic) => {
                self.send_anthropic_stream(messages, tools, extra_body, max_tries)
                    .await
            }
            Ok(ApiFormat::Gemini) => {
                self.send_gemini_stream(messages, tools, extra_body, max_tries)
                    .await
            }
            Err(error) => Err(ModelRequestFailure {
                error: anyhow!(error),
                telemetry: ModelRequestTelemetryMeta {
                    retry_count: 0,
                    status_code: None,
                    error_type: Some("unknown_api_format".to_string()),
                },
            }),
        };

        match result {
            Ok((mut response, telemetry)) => {
                if let Some(mut span) = span {
                    attach_model_request_telemetry(&mut span, &telemetry);
                    response.stream = Box::pin(TelemetryStream::new(response.stream, span));
                }
                Ok(response)
            }
            Err(failure) => {
                if let Some(mut span) = span {
                    attach_model_request_telemetry(&mut span, &failure.telemetry);
                    span.mark_error(failure.error.to_string());
                }
                Err(failure.error)
            }
        }
    }

    /// Send an OpenAI streaming request with retries
    ///
    /// # Parameters
    /// - `messages`: message list
    /// - `tools`: tool definitions
    /// - `extra_body`: extra request body parameters
    /// - `max_tries`: max attempts (including the first)
    async fn send_openai_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
        max_tries: usize,
    ) -> std::result::Result<(StreamResponse, ModelRequestTelemetryMeta), ModelRequestFailure> {
        let url = self.config.request_url.clone();
        debug!(
            "OpenAI config: model={}, request_url={}, max_tries={}",
            self.config.model, self.config.request_url, max_tries
        );

        // Use OpenAI message converter
        let openai_messages = OpenAIMessageConverter::convert_messages(messages);
        let openai_tools = OpenAIMessageConverter::convert_tools(tools);

        // Build request body
        let request_body =
            self.build_openai_request_body(&url, openai_messages, openai_tools, extra_body);

        let mut last_error = None;
        let mut last_telemetry = ModelRequestTelemetryMeta::default();
        let base_wait_time_ms = 500;

        for attempt in 0..max_tries {
            let request_start_time = std::time::Instant::now();

            // Send request - apply request headers
            let request_builder = self.apply_openai_headers(self.client.post(&url));
            let response_result = request_builder.json(&request_body).send().await;

            let response = match response_result {
                Ok(resp) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let status = resp.status();

                    if status.is_client_error() {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        error!(
                            "OpenAI Streaming API client error {}: {}",
                            status, error_text
                        );
                        return Err(ModelRequestFailure {
                            error: anyhow!(
                                "OpenAI Streaming API client error {}: {}",
                                status,
                                error_text
                            ),
                            telemetry: ModelRequestTelemetryMeta {
                                retry_count: attempt,
                                status_code: Some(status.as_u16()),
                                error_type: Some(classify_status_error(status).to_string()),
                            },
                        });
                    }

                    if status.is_success() {
                        debug!(
                            "Stream request connected: {}ms, status: {}, attempt: {}/{}",
                            connect_time,
                            status,
                            attempt + 1,
                            max_tries
                        );
                        resp
                    } else {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        let error =
                            anyhow!("OpenAI Streaming API error {}: {}", status, error_text);
                        warn!(
                            "Stream request failed (attempt {}/{}): {}",
                            attempt + 1,
                            max_tries,
                            error
                        );
                        last_error = Some(error);
                        last_telemetry = ModelRequestTelemetryMeta {
                            retry_count: attempt,
                            status_code: Some(status.as_u16()),
                            error_type: Some(classify_status_error(status).to_string()),
                        };

                        if attempt < max_tries - 1 {
                            let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                            debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        continue;
                    }
                }
                Err(e) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let error = anyhow!("Stream request connection failed: {}", e);
                    warn!(
                        "Stream request connection failed: {}ms, attempt {}/{}, error: {}",
                        connect_time,
                        attempt + 1,
                        max_tries,
                        e
                    );
                    last_error = Some(error);
                    last_telemetry = ModelRequestTelemetryMeta {
                        retry_count: attempt,
                        status_code: None,
                        error_type: Some("connection_error".to_string()),
                    };

                    if attempt < max_tries - 1 {
                        let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                        debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
            };

            // Success: create channels and return
            let status_code = response.status().as_u16();
            let (tx, rx) = mpsc::unbounded_channel();
            let (tx_raw, rx_raw) = mpsc::unbounded_channel();

            tokio::spawn(handle_openai_stream(
                response,
                tx,
                Some(tx_raw),
                self.config.inline_think_in_text,
            ));

            return Ok((
                StreamResponse {
                    stream: Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
                    raw_sse_rx: Some(rx_raw),
                },
                ModelRequestTelemetryMeta {
                    retry_count: attempt,
                    status_code: Some(status_code),
                    error_type: None,
                },
            ));
        }

        let error_msg = format!(
            "Stream request failed after {} attempts: {}",
            max_tries,
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        );
        error!("{}", error_msg);
        Err(ModelRequestFailure {
            error: anyhow!(error_msg),
            telemetry: last_telemetry,
        })
    }

    /// Send a Gemini streaming request with retries.
    async fn send_gemini_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
        max_tries: usize,
    ) -> std::result::Result<(StreamResponse, ModelRequestTelemetryMeta), ModelRequestFailure> {
        let url = Self::resolve_gemini_request_url(&self.config.request_url, &self.config.model);
        debug!(
            "Gemini config: model={}, request_url={}, max_tries={}",
            self.config.model, url, max_tries
        );

        let (system_instruction, contents) =
            GeminiMessageConverter::convert_messages(messages, &self.config.model);
        let gemini_tools = GeminiMessageConverter::convert_tools(tools);
        let request_body =
            self.build_gemini_request_body(system_instruction, contents, gemini_tools, extra_body);

        let mut last_error = None;
        let mut last_telemetry = ModelRequestTelemetryMeta::default();
        let base_wait_time_ms = 500;

        for attempt in 0..max_tries {
            let request_start_time = std::time::Instant::now();
            let request_builder = self.apply_gemini_headers(self.client.post(&url));
            let response_result = request_builder.json(&request_body).send().await;

            let response = match response_result {
                Ok(resp) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let status = resp.status();

                    if status.is_client_error() {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        error!(
                            "Gemini Streaming API client error {}: {}",
                            status, error_text
                        );
                        return Err(ModelRequestFailure {
                            error: anyhow!(
                                "Gemini Streaming API client error {}: {}",
                                status,
                                error_text
                            ),
                            telemetry: ModelRequestTelemetryMeta {
                                retry_count: attempt,
                                status_code: Some(status.as_u16()),
                                error_type: Some(classify_status_error(status).to_string()),
                            },
                        });
                    }

                    if status.is_success() {
                        debug!(
                            "Gemini stream request connected: {}ms, status: {}, attempt: {}/{}",
                            connect_time,
                            status,
                            attempt + 1,
                            max_tries
                        );
                        resp
                    } else {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        let error =
                            anyhow!("Gemini Streaming API error {}: {}", status, error_text);
                        warn!(
                            "Gemini stream request failed: {}ms, attempt {}/{}, error: {}",
                            connect_time,
                            attempt + 1,
                            max_tries,
                            error
                        );
                        last_error = Some(error);
                        last_telemetry = ModelRequestTelemetryMeta {
                            retry_count: attempt,
                            status_code: Some(status.as_u16()),
                            error_type: Some(classify_status_error(status).to_string()),
                        };

                        if attempt < max_tries - 1 {
                            let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                            debug!(
                                "Retrying Gemini after {}ms (attempt {})",
                                delay_ms,
                                attempt + 2
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        continue;
                    }
                }
                Err(e) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let error = anyhow!("Gemini stream request connection failed: {}", e);
                    warn!(
                        "Gemini stream request connection failed: {}ms, attempt {}/{}, error: {}",
                        connect_time,
                        attempt + 1,
                        max_tries,
                        e
                    );
                    last_error = Some(error);
                    last_telemetry = ModelRequestTelemetryMeta {
                        retry_count: attempt,
                        status_code: None,
                        error_type: Some("connection_error".to_string()),
                    };

                    if attempt < max_tries - 1 {
                        let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                        debug!(
                            "Retrying Gemini after {}ms (attempt {})",
                            delay_ms,
                            attempt + 2
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
            };

            let status_code = response.status().as_u16();
            let (tx, rx) = mpsc::unbounded_channel();
            let (tx_raw, rx_raw) = mpsc::unbounded_channel();

            tokio::spawn(handle_gemini_stream(response, tx, Some(tx_raw)));

            return Ok((
                StreamResponse {
                    stream: Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
                    raw_sse_rx: Some(rx_raw),
                },
                ModelRequestTelemetryMeta {
                    retry_count: attempt,
                    status_code: Some(status_code),
                    error_type: None,
                },
            ));
        }

        let error_msg = format!(
            "Gemini stream request failed after {} attempts: {}",
            max_tries,
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        );
        error!("{}", error_msg);
        Err(ModelRequestFailure {
            error: anyhow!(error_msg),
            telemetry: last_telemetry,
        })
    }

    /// Send a Responses API streaming request with retries.
    async fn send_responses_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
        max_tries: usize,
    ) -> std::result::Result<(StreamResponse, ModelRequestTelemetryMeta), ModelRequestFailure> {
        let url = self.config.request_url.clone();
        debug!(
            "Responses config: model={}, request_url={}, max_tries={}",
            self.config.model, self.config.request_url, max_tries
        );

        let (instructions, response_input) =
            OpenAIMessageConverter::convert_messages_to_responses_input(messages);
        let openai_tools = OpenAIMessageConverter::convert_tools(tools);
        let request_body = self.build_responses_request_body(
            instructions,
            response_input,
            openai_tools,
            extra_body,
        );

        let mut last_error = None;
        let mut last_telemetry = ModelRequestTelemetryMeta::default();
        let base_wait_time_ms = 500;

        for attempt in 0..max_tries {
            let request_start_time = std::time::Instant::now();
            let request_builder = self.apply_openai_headers(self.client.post(&url));
            let response_result = request_builder.json(&request_body).send().await;

            let response = match response_result {
                Ok(resp) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let status = resp.status();

                    if status.is_client_error() {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        error!("Responses API client error {}: {}", status, error_text);
                        return Err(ModelRequestFailure {
                            error: anyhow!(
                                "Responses API client error {}: {}",
                                status,
                                error_text
                            ),
                            telemetry: ModelRequestTelemetryMeta {
                                retry_count: attempt,
                                status_code: Some(status.as_u16()),
                                error_type: Some(classify_status_error(status).to_string()),
                            },
                        });
                    }

                    if status.is_success() {
                        debug!(
                            "Responses request connected: {}ms, status: {}, attempt: {}/{}",
                            connect_time,
                            status,
                            attempt + 1,
                            max_tries
                        );
                        resp
                    } else {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        let error = anyhow!("Responses API error {}: {}", status, error_text);
                        warn!(
                            "Responses request failed (attempt {}/{}): {}",
                            attempt + 1,
                            max_tries,
                            error
                        );
                        last_error = Some(error);
                        last_telemetry = ModelRequestTelemetryMeta {
                            retry_count: attempt,
                            status_code: Some(status.as_u16()),
                            error_type: Some(classify_status_error(status).to_string()),
                        };

                        if attempt < max_tries - 1 {
                            let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                            debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        continue;
                    }
                }
                Err(e) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let error = anyhow!("Responses request connection failed: {}", e);
                    warn!(
                        "Responses request connection failed: {}ms, attempt {}/{}, error: {}",
                        connect_time,
                        attempt + 1,
                        max_tries,
                        e
                    );
                    last_error = Some(error);
                    last_telemetry = ModelRequestTelemetryMeta {
                        retry_count: attempt,
                        status_code: None,
                        error_type: Some("connection_error".to_string()),
                    };

                    if attempt < max_tries - 1 {
                        let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                        debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
            };

            let status_code = response.status().as_u16();
            let (tx, rx) = mpsc::unbounded_channel();
            let (tx_raw, rx_raw) = mpsc::unbounded_channel();

            tokio::spawn(handle_responses_stream(response, tx, Some(tx_raw)));

            return Ok((
                StreamResponse {
                    stream: Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
                    raw_sse_rx: Some(rx_raw),
                },
                ModelRequestTelemetryMeta {
                    retry_count: attempt,
                    status_code: Some(status_code),
                    error_type: None,
                },
            ));
        }

        let error_msg = format!(
            "Responses request failed after {} attempts: {}",
            max_tries,
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        );
        error!("{}", error_msg);
        Err(ModelRequestFailure {
            error: anyhow!(error_msg),
            telemetry: last_telemetry,
        })
    }

    /// Send an Anthropic streaming request with retries
    ///
    /// # Parameters
    /// - `messages`: message list
    /// - `tools`: tool definitions
    /// - `extra_body`: extra request body parameters
    /// - `max_tries`: max attempts (including the first)
    async fn send_anthropic_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        extra_body: Option<serde_json::Value>,
        max_tries: usize,
    ) -> std::result::Result<(StreamResponse, ModelRequestTelemetryMeta), ModelRequestFailure> {
        let url = self.config.request_url.clone();
        debug!(
            "Anthropic config: model={}, request_url={}, max_tries={}",
            self.config.model, self.config.request_url, max_tries
        );

        // Use Anthropic message converter
        let (system_message, anthropic_messages) =
            AnthropicMessageConverter::convert_messages(messages);
        let anthropic_tools = AnthropicMessageConverter::convert_tools(tools);

        // Build request body
        let request_body = self.build_anthropic_request_body(
            &url,
            system_message,
            anthropic_messages,
            anthropic_tools,
            extra_body,
        );

        let mut last_error = None;
        let mut last_telemetry = ModelRequestTelemetryMeta::default();
        let base_wait_time_ms = 500;

        for attempt in 0..max_tries {
            let request_start_time = std::time::Instant::now();

            // Send request - apply Anthropic-style request headers
            let request_builder = self.apply_anthropic_headers(self.client.post(&url), &url);
            let response_result = request_builder.json(&request_body).send().await;

            let response = match response_result {
                Ok(resp) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let status = resp.status();

                    if status.is_client_error() {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        error!(
                            "Anthropic Streaming API client error {}: {}",
                            status, error_text
                        );
                        return Err(ModelRequestFailure {
                            error: anyhow!(
                                "Anthropic Streaming API client error {}: {}",
                                status,
                                error_text
                            ),
                            telemetry: ModelRequestTelemetryMeta {
                                retry_count: attempt,
                                status_code: Some(status.as_u16()),
                                error_type: Some(classify_status_error(status).to_string()),
                            },
                        });
                    }

                    if status.is_success() {
                        debug!(
                            "Stream request connected: {}ms, status: {}, attempt: {}/{}",
                            connect_time,
                            status,
                            attempt + 1,
                            max_tries
                        );
                        resp
                    } else {
                        let error_text = resp
                            .text()
                            .await
                            .unwrap_or_else(|e| format!("Failed to read error response: {}", e));
                        let error =
                            anyhow!("Anthropic Streaming API error {}: {}", status, error_text);
                        warn!(
                            "Stream request failed (attempt {}/{}): {}",
                            attempt + 1,
                            max_tries,
                            error
                        );
                        last_error = Some(error);
                        last_telemetry = ModelRequestTelemetryMeta {
                            retry_count: attempt,
                            status_code: Some(status.as_u16()),
                            error_type: Some(classify_status_error(status).to_string()),
                        };

                        if attempt < max_tries - 1 {
                            let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                            debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        continue;
                    }
                }
                Err(e) => {
                    let connect_time = request_start_time.elapsed().as_millis();
                    let error = anyhow!("Stream request connection failed: {}", e);
                    warn!(
                        "Stream request connection failed: {}ms, attempt {}/{}, error: {}",
                        connect_time,
                        attempt + 1,
                        max_tries,
                        e
                    );
                    last_error = Some(error);
                    last_telemetry = ModelRequestTelemetryMeta {
                        retry_count: attempt,
                        status_code: None,
                        error_type: Some("connection_error".to_string()),
                    };

                    if attempt < max_tries - 1 {
                        let delay_ms = base_wait_time_ms * (1 << attempt.min(3));
                        debug!("Retrying after {}ms (attempt {})", delay_ms, attempt + 2);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
            };

            // Success: create channels and return
            let status_code = response.status().as_u16();
            let (tx, rx) = mpsc::unbounded_channel();
            let (tx_raw, rx_raw) = mpsc::unbounded_channel();

            tokio::spawn(handle_anthropic_stream(response, tx, Some(tx_raw)));

            return Ok((
                StreamResponse {
                    stream: Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
                    raw_sse_rx: Some(rx_raw),
                },
                ModelRequestTelemetryMeta {
                    retry_count: attempt,
                    status_code: Some(status_code),
                    error_type: None,
                },
            ));
        }

        let error_msg = format!(
            "Stream request failed after {} attempts: {}",
            max_tries,
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        );
        error!("{}", error_msg);
        Err(ModelRequestFailure {
            error: anyhow!(error_msg),
            telemetry: last_telemetry,
        })
    }

    /// Send a message and wait for the full response (non-streaming)
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AIClient;
    use crate::infrastructure::ai::providers::{
        anthropic, gemini, gemini::GeminiMessageConverter, openai,
    };
    use crate::service::config::types::ReasoningMode;
    use crate::util::types::{AIConfig, ToolDefinition};
    use serde_json::{json, Value};

    fn make_test_client(format: &str, custom_request_body: Option<Value>) -> AIClient {
        AIClient::new(AIConfig {
            name: format!("{}-test", format),
            base_url: "https://example.com/v1".to_string(),
            request_url: "https://example.com/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            format: format.to_string(),
            context_window: 128000,
            max_tokens: Some(8192),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Default,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body,
            custom_request_body_mode: None,
        })
    }

    fn make_trim_test_client(format: &str) -> AIClient {
        let mut client = make_test_client(format, None);
        client.config.custom_request_body_mode = Some("trim".to_string());
        client
    }

    #[test]
    fn resolves_openai_models_url_from_completion_endpoint() {
        let client = AIClient::new(AIConfig {
            name: "test".to_string(),
            base_url: "https://api.openai.com/v1/chat/completions".to_string(),
            request_url: "https://api.openai.com/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-4.1".to_string(),
            format: "openai".to_string(),
            context_window: 128000,
            max_tokens: Some(8192),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Default,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        assert_eq!(
            openai::common::resolve_models_url(&client),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn resolves_anthropic_models_url_from_messages_endpoint() {
        let client = AIClient::new(AIConfig {
            name: "test".to_string(),
            base_url: "https://api.anthropic.com/v1/messages".to_string(),
            request_url: "https://api.anthropic.com/v1/messages".to_string(),
            api_key: "test-key".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            format: "anthropic".to_string(),
            context_window: 200000,
            max_tokens: Some(8192),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Default,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        assert_eq!(
            anthropic::discovery::resolve_models_url(&client),
            "https://api.anthropic.com/v1/models"
        );
    }

    #[test]
    fn build_gemini_request_body_translates_response_format_and_merges_generation_config() {
        let client = AIClient::new(AIConfig {
            name: "gemini".to_string(),
            base_url: "https://example.com".to_string(),
            request_url: "https://example.com/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
                .to_string(),
            api_key: "test-key".to_string(),
            model: "gemini-2.5-pro".to_string(),
            format: "gemini".to_string(),
            context_window: 128000,
            max_tokens: Some(4096),
            temperature: Some(0.2),
            top_p: Some(0.8),
            reasoning_mode: ReasoningMode::Enabled,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let request_body = gemini::request::build_request_body(
            &client,
            None,
            vec![json!({
                "role": "user",
                "parts": [{ "text": "hello" }]
            })],
            None,
            Some(json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": {
                            "type": "object",
                            "properties": {
                                "answer": { "type": "string" }
                            },
                            "required": ["answer"],
                            "additionalProperties": false
                        }
                    }
                },
                "stop": ["END"],
                "generationConfig": {
                    "candidateCount": 1
                }
            })),
        );

        assert_eq!(request_body["generationConfig"]["maxOutputTokens"], 4096);
        assert_eq!(request_body["generationConfig"]["temperature"], 0.2);
        assert_eq!(request_body["generationConfig"]["topP"], 0.8);
        assert_eq!(
            request_body["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );
        assert_eq!(
            request_body["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(request_body["generationConfig"]["candidateCount"], 1);
        assert_eq!(
            request_body["generationConfig"]["stopSequences"],
            json!(["END"])
        );
        assert_eq!(
            request_body["generationConfig"]["responseJsonSchema"]["required"],
            json!(["answer"])
        );
        assert!(request_body["generationConfig"]["responseJsonSchema"]
            .get("additionalProperties")
            .is_none());
        assert!(request_body.get("response_format").is_none());
        assert!(request_body.get("stop").is_none());
    }

    #[test]
    fn build_gemini_request_body_omits_function_calling_config_for_native_only_tools() {
        let client = AIClient::new(AIConfig {
            name: "gemini".to_string(),
            base_url: "https://example.com".to_string(),
            request_url: "https://example.com/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
                .to_string(),
            api_key: "test-key".to_string(),
            model: "gemini-2.5-pro".to_string(),
            format: "gemini".to_string(),
            context_window: 128000,
            max_tokens: Some(4096),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Default,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let gemini_tools = GeminiMessageConverter::convert_tools(Some(vec![ToolDefinition {
            name: "googleSearchRetrieval".to_string(),
            description: "Search the web".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        }]));

        let request_body = gemini::request::build_request_body(
            &client,
            None,
            vec![json!({
                "role": "user",
                "parts": [{ "text": "hello" }]
            })],
            gemini_tools,
            None,
        );

        assert_eq!(request_body["tools"][0]["googleSearchRetrieval"], json!({}));
        assert!(request_body.get("toolConfig").is_none());
    }

    #[test]
    fn build_openai_request_body_uses_generic_thinking_object_when_enabled() {
        let client = AIClient::new(AIConfig {
            name: "openai-compatible".to_string(),
            base_url: "https://example.com/v1".to_string(),
            request_url: "https://example.com/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            format: "openai".to_string(),
            context_window: 128000,
            max_tokens: Some(4096),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Enabled,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let request_body = openai::chat::build_request_body(
            &client,
            &client.config.request_url,
            vec![json!({ "role": "user", "content": "hello" })],
            None,
            None,
        );

        assert_eq!(request_body["thinking"]["type"], "enabled");
        assert!(request_body.get("enable_thinking").is_none());
        assert!(request_body.get("reasoning_split").is_none());
    }

    #[test]
    fn build_openai_request_body_uses_enable_thinking_for_siliconflow() {
        let client = AIClient::new(AIConfig {
            name: "siliconflow".to_string(),
            base_url: "https://api.siliconflow.cn/v1".to_string(),
            request_url: "https://api.siliconflow.cn/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string(),
            format: "openai".to_string(),
            context_window: 128000,
            max_tokens: Some(4096),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Enabled,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let request_body = openai::chat::build_request_body(
            &client,
            &client.config.request_url,
            vec![json!({ "role": "user", "content": "hello" })],
            None,
            None,
        );

        assert_eq!(request_body["enable_thinking"], true);
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn build_responses_request_body_maps_disabled_mode_to_none_effort() {
        let client = AIClient::new(AIConfig {
            name: "responses".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            request_url: "https://api.openai.com/v1/responses".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-5".to_string(),
            format: "responses".to_string(),
            context_window: 128000,
            max_tokens: Some(4096),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Disabled,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: None,
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let request_body = openai::responses::build_request_body(
            &client,
            Some("Be concise".to_string()),
            vec![json!({
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            })],
            None,
            None,
        );

        assert_eq!(request_body["reasoning"]["effort"], "none");
    }

    #[test]
    fn build_anthropic_request_body_uses_adaptive_reasoning_and_effort() {
        let client = AIClient::new(AIConfig {
            name: "anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            request_url: "https://api.anthropic.com/v1/messages".to_string(),
            api_key: "test-key".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            format: "anthropic".to_string(),
            context_window: 200000,
            max_tokens: Some(8192),
            temperature: None,
            top_p: None,
            reasoning_mode: ReasoningMode::Adaptive,
            inline_think_in_text: false,
            custom_headers: None,
            custom_headers_mode: None,
            skip_ssl_verify: false,
            reasoning_effort: Some("high".to_string()),
            thinking_budget_tokens: None,
            custom_request_body: None,
            custom_request_body_mode: None,
        });

        let request_body = anthropic::request::build_request_body(
            &client,
            &client.config.request_url,
            None,
            vec![json!({ "role": "user", "content": [{ "type": "text", "text": "hello" }] })],
            None,
            None,
        );

        assert_eq!(request_body["thinking"]["type"], "adaptive");
        assert_eq!(request_body["output_config"]["effort"], "high");
    }

    #[test]
    fn build_openai_request_body_trim_mode_preserves_essential_fields() {
        let mut client = make_trim_test_client("openai");
        client.config.max_tokens = Some(8192);
        let messages = vec![json!({ "role": "user", "content": "hello" })];

        let request_body = openai::chat::build_request_body(
            &client,
            &client.config.request_url,
            messages.clone(),
            None,
            Some(json!({
                "model": "override-model",
                "messages": [{ "role": "user", "content": "override" }],
                "stream": false,
                "max_tokens": 1,
                "temperature": 0.7,
                "response_format": { "type": "json_object" }
            })),
        );

        assert_eq!(request_body["model"], "test-model");
        assert_eq!(request_body["messages"], json!(messages));
        assert_eq!(request_body["stream"], true);
        assert_eq!(request_body["max_tokens"], 8192);
        assert_eq!(request_body["temperature"], 0.7);
        assert_eq!(request_body["response_format"]["type"], "json_object");
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn build_responses_request_body_trim_mode_preserves_essential_fields() {
        let mut client = make_trim_test_client("responses");
        client.config.max_tokens = Some(4096);
        let input = vec![json!({
            "role": "user",
            "content": [{ "type": "input_text", "text": "hello" }]
        })];

        let request_body = openai::responses::build_request_body(
            &client,
            Some("Be concise".to_string()),
            input.clone(),
            None,
            Some(json!({
                "instructions": "override me",
                "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "override" }] }],
                "stream": false,
                "max_output_tokens": 1,
                "temperature": 0.1
            })),
        );

        assert_eq!(request_body["model"], "test-model");
        assert_eq!(request_body["input"], json!(input));
        assert_eq!(request_body["instructions"], "Be concise");
        assert_eq!(request_body["stream"], true);
        assert_eq!(request_body["max_output_tokens"], 4096);
        assert_eq!(request_body["temperature"], 0.1);
        assert!(request_body.get("reasoning").is_none());
    }

    #[test]
    fn build_anthropic_request_body_trim_mode_preserves_essential_fields() {
        let mut client = make_trim_test_client("anthropic");
        client.config.max_tokens = Some(8192);
        let messages = vec![json!({
            "role": "user",
            "content": [{ "type": "text", "text": "hello" }]
        })];

        let request_body = anthropic::request::build_request_body(
            &client,
            &client.config.request_url,
            Some("Use the system prompt".to_string()),
            messages.clone(),
            None,
            Some(json!({
                "system": "override me",
                "messages": [{ "role": "user", "content": [{ "type": "text", "text": "override" }] }],
                "max_tokens": 1,
                "stream": false,
                "metadata": { "tag": "kept" }
            })),
        );

        assert_eq!(request_body["model"], "test-model");
        assert_eq!(request_body["messages"], json!(messages));
        assert_eq!(request_body["system"], "Use the system prompt");
        assert_eq!(request_body["stream"], true);
        assert_eq!(request_body["max_tokens"], 8192);
        assert_eq!(request_body["metadata"]["tag"], "kept");
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn build_gemini_request_body_trim_mode_preserves_essential_fields() {
        let mut client = make_trim_test_client("gemini");
        client.config.model = "gemini-2.5-pro".to_string();
        client.config.max_tokens = Some(4096);

        let contents = vec![json!({
            "role": "user",
            "parts": [{ "text": "hello" }]
        })];
        let system_instruction = json!({
            "parts": [{ "text": "system" }]
        });
        let gemini_tools = GeminiMessageConverter::convert_tools(Some(vec![ToolDefinition {
            name: "lookup".to_string(),
            description: "Look up data".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        }]));

        let request_body = gemini::request::build_request_body(
            &client,
            Some(system_instruction.clone()),
            contents.clone(),
            gemini_tools,
            Some(json!({
                "contents": [{ "role": "user", "parts": [{ "text": "override" }] }],
                "systemInstruction": { "parts": [{ "text": "override system" }] },
                "generationConfig": {
                    "maxOutputTokens": 1,
                    "candidateCount": 2
                },
                "tools": [],
                "toolConfig": {
                    "functionCallingConfig": {
                        "mode": "NONE"
                    }
                },
                "temperature": 0.3
            })),
        );

        assert_eq!(request_body["contents"], json!(contents));
        assert_eq!(request_body["systemInstruction"], system_instruction);
        assert_eq!(request_body["generationConfig"]["maxOutputTokens"], 4096);
        assert_eq!(request_body["generationConfig"]["candidateCount"], 2);
        assert_eq!(request_body["generationConfig"]["temperature"], 0.3);
        assert_eq!(
            request_body["toolConfig"]["functionCallingConfig"]["mode"],
            "AUTO"
        );
        assert_eq!(
            request_body["tools"][0]["functionDeclarations"][0]["name"],
            "lookup"
        );
    }

    #[test]
    fn streaming_http_client_does_not_apply_global_request_timeout() {
        let client = make_test_client("openai", None);
        let request = client
            .client
            .get("https://example.com/stream")
            .build()
            .expect("request should build");

        assert_eq!(request.timeout(), None);
    }
}
