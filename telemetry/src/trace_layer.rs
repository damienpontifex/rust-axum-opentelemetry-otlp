use std::time::Duration;

use axum::{
    extract::MatchedPath,
    http::{header::USER_AGENT, Request, Response},
};
use opentelemetry::{
    global,
    trace::{SpanKind, TraceContextExt},
    Context,
};
use opentelemetry_http::HeaderExtractor;
use opentelemetry_semantic_conventions::{
    attribute::OTEL_STATUS_CODE,
    trace::{
        HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE, NETWORK_PROTOCOL_VERSION,
        URL_FULL, USER_AGENT_ORIGINAL,
    },
};
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{MakeSpan, OnFailure, OnResponse, TraceLayer},
};
use tracing::{field::Empty, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Clone)]
pub struct OtelMakeSpan {
    span_kind: SpanKind,
}

impl<B> MakeSpan<B> for OtelMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        if self.span_kind == SpanKind::Server {
            // Check whether the incoming request already has a trace id on it we should use as a
            // parent
            let parent_cx: Context = global::get_text_map_propagator(|propagator| {
                propagator.extract(&HeaderExtractor(request.headers()))
            });
            let has_parent_span = parent_cx.span().span_context().is_valid();

            // Extract the path template as the request path such that it is unique for all
            // requests to this endpoint regardless of parameters within the path
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
                { URL_FULL } = %request.uri().path(),
                { NETWORK_PROTOCOL_VERSION } = ?request.version(),
                { USER_AGENT_ORIGINAL } = %request.headers().get(USER_AGENT).and_then(|h| h.to_str().ok()).unwrap_or_default(),
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

/// What the tracing layer is used for
pub enum TracingFor {
    /// The tracing layer is used to trace incoming, or server, requests
    Server,
    /// The tracing layer is used to trace outgoing, or client, requests
    Client,
}

/// A Tower layer that traces requests with opentelemetry tags
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
