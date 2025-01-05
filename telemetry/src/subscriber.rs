use std::{borrow::Cow, time::Duration};

use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_subscriber::prelude::*;

/// Create a Resource that captures information about the entity for which telemetry will be
/// recorded
fn resource(
    service_name: impl Into<opentelemetry::Value>,
    service_version: impl Into<opentelemetry::Value>,
) -> opentelemetry_sdk::Resource {
    use opentelemetry::KeyValue;
    use opentelemetry_semantic_conventions::{
        resource::{SERVICE_NAME, SERVICE_VERSION},
        SCHEMA_URL,
    };

    opentelemetry_sdk::Resource::from_schema_url(
        [
            KeyValue::new(SERVICE_NAME, service_name),
            KeyValue::new(SERVICE_VERSION, service_version),
        ],
        SCHEMA_URL,
    )
}

fn init_trace_provider(
    service_name: impl Into<Cow<'static, str>> + Clone,
    service_version: impl Into<opentelemetry::Value>,
) -> opentelemetry_sdk::trace::Tracer {
    // Set this globally so it can be used to extract trace context when making
    // a span from incoming request headers
    global::set_text_map_propagator(TraceContextPropagator::new());

    // Setup opentelemetry trace provider with OTLP exporter
    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_resource(resource(service_name.clone().into(), service_version))
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

    global::set_tracer_provider(provider.clone());

    provider.tracer(service_name)
}

pub struct TracingGuard;
impl Drop for TracingGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

#[must_use]
pub fn init_tracing(
    service_name: impl Into<Cow<'static, str>> + Clone,
    service_version: impl Into<opentelemetry::Value>,
) -> TracingGuard {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(false)
                .with_file(false)
                .json(),
        )
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(init_trace_provider(service_name, service_version))
                .with_location(false),
        )
        .init();

    TracingGuard
}
