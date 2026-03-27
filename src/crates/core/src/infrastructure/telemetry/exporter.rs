use super::TelemetryInitConfig;
use crate::util::errors::{BitFunError, BitFunResult};
use log::{info, warn};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

const DEFAULT_OTLP_ENDPOINT: &str = "http://7.183.57.199:4317";

#[derive(Debug, Clone)]
struct TelemetryExporterConfig {
    traces_endpoint: String,
    protocol: Protocol,
}

pub(super) fn build_tracer_provider(
    config: &TelemetryInitConfig,
    resource: Resource,
) -> BitFunResult<SdkTracerProvider> {
    let builder = SdkTracerProvider::builder().with_resource(resource);

    if !config.enabled {
        info!(
            "Telemetry initialized without exporter: service_name={}, reason=disabled",
            config.service_name
        );
        return Ok(builder.build());
    }

    match resolve_exporter_config() {
        Some(exporter_config) => {
            info!(
                "Telemetry exporter configured locally: service_name={}, protocol={:?}, traces_endpoint={}",
                config.service_name, exporter_config.protocol, exporter_config.traces_endpoint
            );
            let exporter = build_otlp_exporter(&exporter_config)?;
            Ok(builder.with_batch_exporter(exporter).build())
        }
        None => {
            warn!(
                "Telemetry is enabled but no exporter endpoint is configured; spans will remain local only"
            );
            Ok(builder.build())
        }
    }
}

fn build_otlp_exporter(config: &TelemetryExporterConfig) -> BitFunResult<SpanExporter> {
    let exporter = match config.protocol {
        Protocol::Grpc => SpanExporter::builder()
            .with_tonic()
            .with_protocol(Protocol::Grpc)
            .with_endpoint(config.traces_endpoint.clone())
            .build(),
        Protocol::HttpBinary => SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(config.traces_endpoint.clone())
            .build(),
        other => {
            return Err(BitFunError::service(format!(
                "Unsupported telemetry export protocol: {:?}",
                other
            )));
        }
    };

    exporter.map_err(|error| {
        BitFunError::service(format!(
            "Failed to build telemetry exporter for protocol={:?}, endpoint='{}': {}",
            config.protocol, config.traces_endpoint, error
        ))
    })
}

fn resolve_exporter_config() -> Option<TelemetryExporterConfig> {
    let protocol = resolve_export_protocol();

    let traces_endpoint = read_env_trimmed("BITFUN_OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
        .or_else(|| read_env_trimmed("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"))
        .or_else(|| {
            read_env_trimmed("BITFUN_OTEL_EXPORTER_OTLP_ENDPOINT")
                .or_else(|| read_env_trimmed("OTEL_EXPORTER_OTLP_ENDPOINT"))
                .map(|base| normalize_endpoint_for_protocol(&base, protocol))
        })
        .or_else(|| compiled_default_endpoint().map(|base| normalize_endpoint_for_protocol(base, protocol)))?;

    Some(TelemetryExporterConfig {
        traces_endpoint,
        protocol,
    })
}

fn read_env_trimmed(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_otlp_traces_endpoint(value: &str) -> String {
    let normalized = value.trim_end_matches('/');
    if normalized.ends_with("/v1/traces") {
        normalized.to_string()
    } else {
        format!("{normalized}/v1/traces")
    }
}

fn normalize_endpoint_for_protocol(value: &str, protocol: Protocol) -> String {
    match protocol {
        Protocol::Grpc => value.trim_end_matches('/').to_string(),
        Protocol::HttpBinary => normalize_otlp_traces_endpoint(value),
        _ => value.trim_end_matches('/').to_string(),
    }
}

fn resolve_export_protocol() -> Protocol {
    let value = read_env_trimmed("BITFUN_OTEL_EXPORTER_OTLP_PROTOCOL")
        .or_else(|| read_env_trimmed("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .or_else(compiled_default_protocol);

    match value.as_deref() {
        Some("grpc") => Protocol::Grpc,
        Some("http/protobuf") => Protocol::HttpBinary,
        Some(other) => {
            warn!(
                "Unsupported telemetry protocol '{}', falling back to grpc",
                other
            );
            Protocol::Grpc
        }
        None => Protocol::Grpc,
    }
}

fn compiled_default_endpoint() -> Option<&'static str> {
    option_env!("BITFUN_COMPILED_OTLP_ENDPOINT")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(Some(DEFAULT_OTLP_ENDPOINT))
}

fn compiled_default_protocol() -> Option<String> {
    option_env!("BITFUN_COMPILED_OTLP_PROTOCOL")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
