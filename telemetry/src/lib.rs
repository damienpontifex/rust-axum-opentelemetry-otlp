use std::{borrow::Cow, time::Duration};

use axum::{
    extract::MatchedPath,
    http::{Request, Response},
};
use opentelemetry::{global, trace::SpanKind, trace::TracerProvider};
use opentelemetry_semantic_conventions::{
    attribute::OTEL_STATUS_CODE,
    trace::{HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE, NETWORK_PROTOCOL_VERSION},
};
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{MakeSpan, OnFailure, OnResponse, TraceLayer},
};
use tracing::{field::Empty, Span};
use tracing_subscriber::prelude::*;

#[derive(Clone)]
pub struct OtelMakeSpan;
impl<B> MakeSpan<B> for OtelMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let matched_path = request
            .extensions()
            .get::<MatchedPath>()
            .map(MatchedPath::as_str)
            .unwrap_or("{unknown}");

        tracing::info_span!(
            "request",
            otel.name = format!("{} {}", request.method(), matched_path),
            span.kind = ?SpanKind::Server,
            { OTEL_STATUS_CODE } = Empty,
            { HTTP_REQUEST_METHOD } = ?request.method(),
            { HTTP_RESPONSE_STATUS_CODE } = Empty,
            { HTTP_ROUTE } = %request.uri().path(),
            { NETWORK_PROTOCOL_VERSION } = ?request.version(),
        )
    }
}

#[derive(Clone)]
pub struct OtelOnResponse;
impl<B> OnResponse<B> for OtelOnResponse {
    fn on_response(self, response: &Response<B>, _latency: Duration, span: &Span) {
        let status_code = response.status().as_u16();
        let is_failure = if status_code < 300 { "ok" } else { "error" };
        span.record(OTEL_STATUS_CODE, is_failure);
        span.record(HTTP_RESPONSE_STATUS_CODE, status_code);
    }
}

#[derive(Clone)]
pub struct OtelOnFailure;
impl<B> OnFailure<B> for OtelOnFailure {
    fn on_failure(&mut self, _failure_classification: B, _latency: Duration, span: &Span) {
        span.record(OTEL_STATUS_CODE, "error");
    }
}

pub fn trace_layer() -> TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    OtelMakeSpan,
    (),
    OtelOnResponse,
    (),
    (),
    OtelOnFailure,
> {
    TraceLayer::new_for_http()
        .make_span_with(OtelMakeSpan)
        .on_request(())
        .on_response(OtelOnResponse)
        .on_body_chunk(())
        .on_eos(())
        .on_failure(OtelOnFailure)
}

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

fn tracer(
    service_name: impl Into<Cow<'static, str>> + Clone,
    service_version: impl Into<opentelemetry::Value>,
) -> opentelemetry_sdk::trace::Tracer {
    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_resource(resource(service_name.clone().into(), service_version))
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
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
                .with_tracer(tracer(service_name, service_version))
                .with_location(false),
        )
        .init();

    TracingGuard
}
