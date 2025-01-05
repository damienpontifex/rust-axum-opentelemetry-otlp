use std::{borrow::Cow, time::Duration};

use axum::{
    extract::MatchedPath,
    http::{request, Request, Response},
};
use opentelemetry::{
    global,
    trace::{SpanKind, TraceContextExt, TracerProvider},
    Context,
};
use opentelemetry_http::{HeaderExtractor, HeaderInjector};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_semantic_conventions::{
    attribute::OTEL_STATUS_CODE,
    trace::{HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE, NETWORK_PROTOCOL_VERSION},
};
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{MakeSpan, OnFailure, OnRequest, OnResponse, TraceLayer},
};
use tracing::{field::Empty, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::prelude::*;

#[derive(Clone)]
pub struct OtelMakeSpan {
    span_kind: SpanKind,
}

impl<B> MakeSpan<B> for OtelMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        if self.span_kind == SpanKind::Server {
            let parent_cx: Context = global::get_text_map_propagator(|propagator| {
                propagator.extract(&HeaderExtractor(request.headers()))
            });
            let has_parent_span = parent_cx.span().span_context().is_valid();

            let path_template = request
                .extensions()
                .get::<MatchedPath>()
                .map(MatchedPath::as_str)
                .unwrap_or("{unknown}");

            let span = tracing::info_span!(
                "request",
                otel.name = format!("{} {}", request.method(), path_template),
                span.kind = ?self.span_kind,
                { OTEL_STATUS_CODE } = Empty,
                { HTTP_REQUEST_METHOD } = ?request.method(),
                { HTTP_RESPONSE_STATUS_CODE } = Empty,
                { HTTP_ROUTE } = %request.uri().path(),
                { NETWORK_PROTOCOL_VERSION } = ?request.version(),
            );

            if has_parent_span {
                span.set_parent(parent_cx);
            }

            return span;
        } else {
            // TODO: Refine tags that a client would use e.g. reqwest
            return tracing::info_span!(
                "request",
                otel.name = ?request.method(),
                span.kind = ?self.span_kind,
                url.full = %request.uri(),
                { OTEL_STATUS_CODE } = Empty,
                { HTTP_REQUEST_METHOD } = ?request.method(),
                { HTTP_RESPONSE_STATUS_CODE } = Empty,
                { NETWORK_PROTOCOL_VERSION } = ?request.version(),
            );
        };
    }
}

// TODO: inject tracing headers into http client e.g. https://github.com/open-telemetry/opentelemetry-rust/blob/main/examples/tracing-http-propagator/src/client.rs
// Or might use `HttpClient` trait from opentelemetry_http https://docs.rs/opentelemetry-http/latest/src/opentelemetry_http/lib.rs.html#68
//#[derive(Clone)]
//pub struct OtelOnRequest {
//    span_kind: SpanKind,
//}
//impl<B> OnRequest<B> for OtelOnRequest {
//    fn on_request(&mut self, request: &Request<B>, span: &Span) {
//        if self.span_kind == SpanKind::Client {
//            global::get_text_map_propagator(|propagator| {
//                propagator.inject(&mut HeaderInjector(request.headers_mut()));
//            });
//        }
//    }
//}

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

pub enum TracingFor {
    Server,
    Client,
}

pub fn trace_layer(
    tracing_for: TracingFor,
) -> TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    OtelMakeSpan,
    (),
    OtelOnResponse,
    (),
    (),
    OtelOnFailure,
> {
    let span_kind = match tracing_for {
        TracingFor::Server => SpanKind::Server,
        TracingFor::Client => SpanKind::Client,
    };
    TraceLayer::new_for_http()
        .make_span_with(OtelMakeSpan { span_kind })
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
    global::set_text_map_propagator(TraceContextPropagator::new());

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
                .with_tracer(tracer(service_name, service_version))
                .with_location(false),
        )
        .init();

    TracingGuard
}
