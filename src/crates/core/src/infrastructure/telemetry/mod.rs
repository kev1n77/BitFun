//! OpenTelemetry foundation for BitFun.
//!
//! This module intentionally hides OTel specifics behind a small internal API so
//! business code can emit telemetry without depending on exporter details.

mod exporter;

use crate::agentic::events::{AgenticEvent, EventSubscriber, ToolEventData};
use crate::infrastructure::{try_get_path_manager_arc, PathManager};
use crate::service::system;
use crate::util::errors::{BitFunError, BitFunResult};
use chrono::{SecondsFormat, Utc};
use exporter::build_tracer_provider;
use log::{debug, info, warn};
use opentelemetry::global;
use opentelemetry::trace::{Span, Status, Tracer};
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static GLOBAL_TELEMETRY: OnceLock<Arc<TelemetryService>> = OnceLock::new();

tokio::task_local! {
    static ACTIVE_REQUEST_CONTEXT: TelemetryRequestContext;
}

#[derive(Debug, Clone)]
pub struct TelemetryInitConfig {
    pub service_name: String,
    pub app_name: String,
    pub app_version: String,
    pub app_kind: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ConfiguredTelemetry {
    service: Arc<TelemetryService>,
}

#[derive(Debug, Clone)]
pub struct TelemetryIdentity {
    pub uid: String,
    pub process_session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TelemetryRequestContext {
    pub session_id: String,
    pub turn_id: String,
    pub round_id: String,
    pub is_subagent: bool,
}

impl ConfiguredTelemetry {
    pub fn service(&self) -> Arc<TelemetryService> {
        self.service.clone()
    }

    pub fn event_subscriber(&self) -> Arc<TelemetryEventSubscriber> {
        Arc::new(TelemetryEventSubscriber::new(self.service.clone()))
    }
}

#[derive(Debug, Clone)]
struct TelemetryCommonContext {
    uid: String,
    username: Option<String>,
    process_session_id: String,
    ide_version: String,
    os: String,
    os_version: Option<String>,
    arch: String,
    app_name: String,
    app_kind: String,
}

#[derive(Debug)]
pub struct TelemetryService {
    enabled: AtomicBool,
    tracer_name: String,
    common_context: TelemetryCommonContext,
    provider: Mutex<Option<SdkTracerProvider>>,
}

impl TelemetryService {
    fn new(
        enabled: bool,
        tracer_name: String,
        common_context: TelemetryCommonContext,
        provider: SdkTracerProvider,
    ) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            tracer_name,
            common_context,
            provider: Mutex::new(Some(provider)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        let old = self.enabled.swap(enabled, Ordering::Relaxed);
        if old != enabled {
            debug!(
                "Telemetry enablement updated: old_enabled={}, new_enabled={}",
                old, enabled
            );
        }
    }

    pub fn uid(&self) -> &str {
        &self.common_context.uid
    }

    pub fn process_session_id(&self) -> &str {
        &self.common_context.process_session_id
    }

    pub fn emit_event(&self, event_name: &str, attrs: Vec<KeyValue>) {
        if !self.is_enabled() {
            return;
        }

        let merged_attrs = self.merge_with_common_attributes(event_name, attrs);
        debug!(
            "Telemetry event emitted locally: event_name={}, attrs={:?}",
            event_name, merged_attrs
        );
        let mut span = self.start_span_internal(event_name, merged_attrs);
        span.end();
    }

    pub fn start_request_span(
        &self,
        span_name: &str,
        attrs: Vec<KeyValue>,
    ) -> Option<TelemetryRequestSpan> {
        if !self.is_enabled() {
            return None;
        }

        let mut merged_attrs = self.merge_with_common_attributes(span_name, attrs);
        merged_attrs.extend(current_request_context_attributes());
        debug!(
            "Telemetry span started locally: span_name={}, attrs={:?}",
            span_name, merged_attrs
        );
        Some(TelemetryRequestSpan {
            span: Some(self.start_span_internal(span_name, merged_attrs.clone())),
            started_at: Instant::now(),
            finished: false,
            span_name: span_name.to_string(),
            base_attrs: merged_attrs,
        })
    }

    fn start_span_internal(&self, span_name: &str, attrs: Vec<KeyValue>) -> global::BoxedSpan {
        let tracer = global::tracer(self.tracer_name.clone());
        let mut span = tracer.start(span_name.to_string());
        for attr in attrs {
            span.set_attribute(attr);
        }
        span
    }

    fn merge_with_common_attributes(
        &self,
        event_name: &str,
        attrs: Vec<KeyValue>,
    ) -> Vec<KeyValue> {
        let mut merged = self.common_attributes(event_name);
        merged.extend(attrs);
        merged
    }

    fn common_attributes(&self, event_name: &str) -> Vec<KeyValue> {
        let mut attrs = vec![
            KeyValue::new(
                "timestamp",
                Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            ),
            KeyValue::new("uid", self.common_context.uid.clone()),
            KeyValue::new(
                "process_session_id",
                self.common_context.process_session_id.clone(),
            ),
            KeyValue::new("ide_version", self.common_context.ide_version.clone()),
            KeyValue::new("os", self.common_context.os.clone()),
            KeyValue::new("arch", self.common_context.arch.clone()),
            KeyValue::new("app_name", self.common_context.app_name.clone()),
            KeyValue::new("app_kind", self.common_context.app_kind.clone()),
            KeyValue::new("event_name", event_name.to_string()),
        ];

        if let Some(username) = self.common_context.username.clone() {
            attrs.push(KeyValue::new("username", username));
        }

        if let Some(os_version) = self.common_context.os_version.clone() {
            attrs.push(KeyValue::new("os_version", os_version));
        }

        attrs
    }

    pub fn shutdown(&self) {
        let provider = self.provider.lock().ok().and_then(|mut guard| guard.take());

        if let Some(provider) = provider {
            if let Err(error) = provider.shutdown() {
                warn!("Failed to shutdown telemetry provider cleanly: {}", error);
            }
        }
    }

    pub fn force_flush(&self) {
        let provider = self
            .provider
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());

        if let Some(provider) = provider {
            if let Err(error) = provider.force_flush() {
                warn!(
                    "Failed to force flush telemetry provider cleanly: {}",
                    error
                );
            }
        }
    }

    pub fn shutdown_with_timeout(&self, timeout: Duration) {
        let provider = self.provider.lock().ok().and_then(|mut guard| guard.take());

        if let Some(provider) = provider {
            if let Err(error) = provider.shutdown_with_timeout(timeout) {
                warn!(
                    "Failed to shutdown telemetry provider cleanly within {:?}: {}",
                    timeout, error
                );
            }
        }
    }
}

pub struct TelemetryRequestSpan {
    span: Option<global::BoxedSpan>,
    started_at: Instant,
    finished: bool,
    span_name: String,
    base_attrs: Vec<KeyValue>,
}

impl TelemetryRequestSpan {
    pub fn add_attribute(&mut self, attr: KeyValue) {
        self.base_attrs.retain(|existing| existing.key != attr.key);
        self.base_attrs.push(attr.clone());
        if let Some(span) = self.span.as_mut() {
            span.set_attribute(attr);
        }
    }

    pub fn mark_success(&mut self) {
        self.finish("success", Status::Ok, vec![KeyValue::new("success", true)]);
    }

    pub fn mark_cancelled(&mut self, reason: &str) {
        self.finish(
            "cancelled",
            Status::Ok,
            vec![
                KeyValue::new("success", false),
                KeyValue::new("cancelled", true),
                KeyValue::new("cancel_reason", reason.to_string()),
            ],
        );
    }

    pub fn mark_error(&mut self, error_message: impl Into<String>) {
        let error_message = error_message.into();
        self.finish(
            "error",
            Status::error(error_message.clone()),
            vec![
                KeyValue::new("success", false),
                KeyValue::new("cancelled", false),
                KeyValue::new("error", error_message),
            ],
        );
    }

    fn finish(&mut self, outcome: &str, status: Status, attrs: Vec<KeyValue>) {
        if self.finished {
            return;
        }

        let duration_ms = self.started_at.elapsed().as_millis() as i64;
        let mut logged_attrs = self.base_attrs.clone();
        logged_attrs.push(KeyValue::new("duration_ms", duration_ms));
        logged_attrs.extend(attrs.iter().cloned());
        debug!(
            "Telemetry span finished locally: span_name={}, outcome={}, duration_ms={}, attrs={:?}",
            self.span_name, outcome, duration_ms, logged_attrs
        );

        if let Some(mut span) = self.span.take() {
            span.set_status(status);
            span.set_attribute(KeyValue::new("duration_ms", duration_ms));
            for attr in attrs {
                span.set_attribute(attr);
            }
            span.end();
        }

        self.finished = true;
    }
}

impl Drop for TelemetryRequestSpan {
    fn drop(&mut self) {
        if !self.finished {
            self.mark_cancelled("dropped_before_completion");
        }
    }
}

pub struct TelemetryEventSubscriber {
    telemetry: Arc<TelemetryService>,
}

impl TelemetryEventSubscriber {
    pub fn new(telemetry: Arc<TelemetryService>) -> Self {
        Self { telemetry }
    }
}

#[async_trait::async_trait]
impl EventSubscriber for TelemetryEventSubscriber {
    async fn on_event(&self, event: &AgenticEvent) -> BitFunResult<()> {
        match event {
            AgenticEvent::DialogTurnStarted {
                session_id,
                turn_id,
                turn_index,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "chat_request_started",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("turn_index", *turn_index as i64),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::DialogTurnCompleted {
                session_id,
                turn_id,
                total_rounds,
                total_tools,
                duration_ms,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "chat_request_completed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("total_rounds", *total_rounds as i64),
                    KeyValue::new("total_tools", *total_tools as i64),
                    KeyValue::new("duration_ms", *duration_ms as i64),
                    KeyValue::new("success", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::DialogTurnCancelled {
                session_id,
                turn_id,
                subagent_parent_info,
            } => self.telemetry.emit_event(
                "chat_request_cancelled",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("cancelled", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::DialogTurnFailed {
                session_id,
                turn_id,
                error,
                subagent_parent_info,
            } => self.telemetry.emit_event(
                "chat_request_failed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("success", false),
                    KeyValue::new("error", error.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ImageAnalysisStarted {
                session_id,
                image_count,
                ..
            } => self.telemetry.emit_event(
                "image_analysis_started",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("image_count", *image_count as i64),
                ],
            ),
            AgenticEvent::ImageAnalysisCompleted {
                session_id,
                success,
                duration_ms,
            } => self.telemetry.emit_event(
                "image_analysis_completed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("success", *success),
                    KeyValue::new("duration_ms", *duration_ms as i64),
                ],
            ),
            AgenticEvent::TokenUsageUpdated {
                session_id,
                turn_id,
                model_id,
                input_tokens,
                output_tokens,
                total_tokens,
                max_context_tokens,
                is_subagent,
            } => {
                let mut attrs = vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("model_id", model_id.clone()),
                    KeyValue::new("input_tokens", *input_tokens as i64),
                    KeyValue::new("output_tokens", output_tokens.unwrap_or(0) as i64),
                    KeyValue::new("total_tokens", *total_tokens as i64),
                    KeyValue::new("is_subagent", *is_subagent),
                ];
                if let Some(max_context_tokens) = max_context_tokens {
                    attrs.push(KeyValue::new(
                        "max_context_tokens",
                        *max_context_tokens as i64,
                    ));
                }
                self.telemetry.emit_event("token_usage_updated", attrs);
            }
            AgenticEvent::ContextCompressionStarted {
                session_id,
                turn_id,
                compression_id,
                trigger,
                tokens_before,
                context_window,
                threshold,
                subagent_parent_info,
            } => self.telemetry.emit_event(
                "context_compression_started",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("compression_id", compression_id.clone()),
                    KeyValue::new("trigger", trigger.clone()),
                    KeyValue::new("tokens_before", *tokens_before as i64),
                    KeyValue::new("context_window", *context_window as i64),
                    KeyValue::new("threshold", *threshold as f64),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ContextCompressionCompleted {
                session_id,
                turn_id,
                compression_id,
                compression_count,
                tokens_before,
                tokens_after,
                compression_ratio,
                duration_ms,
                has_summary,
                summary_source,
                subagent_parent_info,
            } => self.telemetry.emit_event(
                "context_compression_completed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("compression_id", compression_id.clone()),
                    KeyValue::new("compression_count", *compression_count as i64),
                    KeyValue::new("tokens_before", *tokens_before as i64),
                    KeyValue::new("tokens_after", *tokens_after as i64),
                    KeyValue::new("compression_ratio", *compression_ratio),
                    KeyValue::new("duration_ms", *duration_ms as i64),
                    KeyValue::new("has_summary", *has_summary),
                    KeyValue::new("summary_source", summary_source.clone()),
                    KeyValue::new("success", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ContextCompressionFailed {
                session_id,
                turn_id,
                compression_id,
                error,
                subagent_parent_info,
            } => self.telemetry.emit_event(
                "context_compression_failed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("compression_id", compression_id.clone()),
                    KeyValue::new("success", false),
                    KeyValue::new("error", error.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ModelRoundStarted {
                session_id,
                turn_id,
                round_id,
                round_index,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "model_round_started",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("round_id", round_id.clone()),
                    KeyValue::new("round_index", *round_index as i64),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ModelRoundCompleted {
                session_id,
                turn_id,
                round_id,
                has_tool_calls,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "model_round_completed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("round_id", round_id.clone()),
                    KeyValue::new("has_tool_calls", *has_tool_calls),
                    KeyValue::new("success", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ModelRoundCancelled {
                session_id,
                turn_id,
                round_id,
                round_index,
                reason,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "model_round_cancelled",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("round_id", round_id.clone()),
                    KeyValue::new("round_index", *round_index as i64),
                    KeyValue::new("cancelled", true),
                    KeyValue::new("cancel_reason", reason.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ModelRoundFailed {
                session_id,
                turn_id,
                round_id,
                round_index,
                error,
                subagent_parent_info,
                ..
            } => self.telemetry.emit_event(
                "model_round_failed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("round_id", round_id.clone()),
                    KeyValue::new("round_index", *round_index as i64),
                    KeyValue::new("success", false),
                    KeyValue::new("error", error.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ToolEvent {
                session_id,
                turn_id,
                subagent_parent_info,
                tool_event:
                    ToolEventData::Started {
                        tool_id, tool_name, ..
                    },
                ..
            } => self.telemetry.emit_event(
                "tool_request_started",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("tool_id", tool_id.clone()),
                    KeyValue::new("tool_name", tool_name.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ToolEvent {
                session_id,
                turn_id,
                subagent_parent_info,
                tool_event:
                    ToolEventData::Completed {
                        tool_id,
                        tool_name,
                        duration_ms,
                        ..
                    },
                ..
            } => self.telemetry.emit_event(
                "tool_request_completed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("tool_id", tool_id.clone()),
                    KeyValue::new("tool_name", tool_name.clone()),
                    KeyValue::new("duration_ms", *duration_ms as i64),
                    KeyValue::new("success", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ToolEvent {
                session_id,
                turn_id,
                subagent_parent_info,
                tool_event:
                    ToolEventData::Failed {
                        tool_id,
                        tool_name,
                        error,
                    },
                ..
            } => self.telemetry.emit_event(
                "tool_request_failed",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("tool_id", tool_id.clone()),
                    KeyValue::new("tool_name", tool_name.clone()),
                    KeyValue::new("success", false),
                    KeyValue::new("error", error.clone()),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            AgenticEvent::ToolEvent {
                session_id,
                turn_id,
                subagent_parent_info,
                tool_event:
                    ToolEventData::Cancelled {
                        tool_id,
                        tool_name,
                        reason,
                    },
                ..
            } => self.telemetry.emit_event(
                "tool_request_cancelled",
                vec![
                    KeyValue::new("session_id", session_id.clone()),
                    KeyValue::new("turn_id", turn_id.clone()),
                    KeyValue::new("tool_id", tool_id.clone()),
                    KeyValue::new("tool_name", tool_name.clone()),
                    KeyValue::new("cancel_reason", reason.clone()),
                    KeyValue::new("cancelled", true),
                    KeyValue::new("is_subagent", subagent_parent_info.is_some()),
                ],
            ),
            _ => {}
        }

        Ok(())
    }
}

pub async fn with_telemetry_request_context<F>(
    context: TelemetryRequestContext,
    future: F,
) -> F::Output
where
    F: Future,
{
    ACTIVE_REQUEST_CONTEXT.scope(context, future).await
}

pub fn initialize_global_telemetry(
    config: TelemetryInitConfig,
) -> BitFunResult<ConfiguredTelemetry> {
    if let Some(existing) = GLOBAL_TELEMETRY.get() {
        existing.set_enabled(config.enabled);
        return Ok(ConfiguredTelemetry {
            service: existing.clone(),
        });
    }

    let system_info = system::get_system_info();
    let common_context = TelemetryCommonContext {
        uid: resolve_or_create_uid()?,
        username: resolve_current_username(),
        process_session_id: uuid::Uuid::new_v4().to_string(),
        ide_version: config.app_version.clone(),
        os: system_info.platform,
        os_version: system_info.os_version,
        arch: system_info.arch,
        app_name: config.app_name.clone(),
        app_kind: config.app_kind.clone(),
    };

    let resource = Resource::builder_empty()
        .with_attributes(vec![
            KeyValue::new("service.name", config.service_name.clone()),
            KeyValue::new("service.version", config.app_version.clone()),
            KeyValue::new("app.name", config.app_name.clone()),
            KeyValue::new("app.kind", config.app_kind.clone()),
            KeyValue::new("service.instance.id", common_context.uid.clone()),
        ])
        .build();

    let provider = build_tracer_provider(&config, resource)?;
    global::set_tracer_provider(provider.clone());

    let tracer_name = format!("{}.telemetry", config.service_name);
    let service = Arc::new(TelemetryService::new(
        config.enabled,
        tracer_name,
        common_context,
        provider,
    ));

    GLOBAL_TELEMETRY.set(service.clone()).map_err(|_| {
        BitFunError::service("Failed to initialize global telemetry service".to_string())
    })?;

    info!(
        "Telemetry initialized locally: service_name={}, app_name={}, app_kind={}, enabled={}",
        config.service_name, config.app_name, config.app_kind, config.enabled
    );

    Ok(ConfiguredTelemetry { service })
}
pub fn get_global_telemetry() -> Option<Arc<TelemetryService>> {
    GLOBAL_TELEMETRY.get().cloned()
}

pub fn get_telemetry_identity() -> BitFunResult<TelemetryIdentity> {
    if let Some(telemetry) = GLOBAL_TELEMETRY.get() {
        return Ok(TelemetryIdentity {
            uid: telemetry.uid().to_string(),
            process_session_id: Some(telemetry.process_session_id().to_string()),
        });
    }

    Ok(TelemetryIdentity {
        uid: resolve_or_create_uid()?,
        process_session_id: None,
    })
}

pub fn shutdown_global_telemetry() {
    if let Some(telemetry) = GLOBAL_TELEMETRY.get() {
        telemetry.shutdown();
    }
}

pub fn flush_and_shutdown_global_telemetry(timeout: Duration) {
    if let Some(telemetry) = GLOBAL_TELEMETRY.get() {
        telemetry.force_flush();
        telemetry.shutdown_with_timeout(timeout);
    }
}

pub fn shutdown_global_telemetry_with_timeout(timeout: Duration) {
    if let Some(telemetry) = GLOBAL_TELEMETRY.get() {
        telemetry.shutdown_with_timeout(timeout);
    }
}

fn current_request_context_attributes() -> Vec<KeyValue> {
    ACTIVE_REQUEST_CONTEXT
        .try_with(|context| {
            vec![
                KeyValue::new("session_id", context.session_id.clone()),
                KeyValue::new("turn_id", context.turn_id.clone()),
                KeyValue::new("round_id", context.round_id.clone()),
                KeyValue::new("is_subagent", context.is_subagent),
            ]
        })
        .unwrap_or_default()
}

fn resolve_or_create_uid() -> BitFunResult<String> {
    let path = telemetry_uid_path()?;
    if let Ok(existing) = fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Ok(existing.to_string());
        }
    }

    let uid = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            BitFunError::service(format!(
                "Failed to create telemetry directory '{}': {}",
                parent.display(),
                error
            ))
        })?;
    }
    fs::write(&path, uid.as_bytes()).map_err(|error| {
        BitFunError::service(format!(
            "Failed to persist telemetry uid '{}': {}",
            path.display(),
            error
        ))
    })?;

    Ok(uid)
}

fn resolve_current_username() -> Option<String> {
    let candidates = if cfg!(target_os = "windows") {
        ["USERNAME", "USER", "LOGNAME"]
    } else {
        ["USER", "LOGNAME", "USERNAME"]
    };

    candidates
        .into_iter()
        .filter_map(read_nonempty_env_var)
        .next()
}

fn read_nonempty_env_var(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn telemetry_uid_path() -> BitFunResult<PathBuf> {
    if let Ok(path_manager) = try_get_path_manager_arc() {
        return Ok(path_manager.user_data_dir().join("telemetry").join("uid"));
    }

    let path_manager = PathManager::new()?;
    Ok(path_manager.user_data_dir().join("telemetry").join("uid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_attributes_include_timestamp_and_process_session_id() {
        let telemetry = TelemetryService::new(
            true,
            "bitfun-desktop.telemetry".to_string(),
            TelemetryCommonContext {
                uid: "uid-1".to_string(),
                username: Some("emp001".to_string()),
                process_session_id: "process-1".to_string(),
                ide_version: "0.0.1".to_string(),
                os: "macos".to_string(),
                os_version: Some("14.0".to_string()),
                arch: "aarch64".to_string(),
                app_name: "BitFun Desktop".to_string(),
                app_kind: "desktop".to_string(),
            },
            SdkTracerProvider::builder().build(),
        );

        let attrs = telemetry.common_attributes("app_launch_succeeded");

        assert!(attrs.iter().any(|attr| attr.key.as_str() == "timestamp"));
        assert!(attrs
            .iter()
            .any(|attr| attr.key.as_str() == "process_session_id"));
        assert!(attrs.iter().any(|attr| {
            attr.key.as_str() == "username" && attr.value.as_str().as_ref() == "emp001"
        }));
    }
}
