use std::net::Ipv4Addr;
use std::time::Duration;

use axum::extract::MatchedPath;
use axum::http::Request;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use opentelemetry::trace::SpanKind;
use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_semantic_conventions::attribute::OTEL_STATUS_CODE;
use opentelemetry_semantic_conventions::trace::{
    HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE, NETWORK_PROTOCOL_VERSION,
};
use tower_http::trace::TraceLayer;
use tracing::Span;
use tracing_subscriber::prelude::*;

use tower::ServiceBuilder;

fn resource() -> opentelemetry_sdk::Resource {
    use opentelemetry::KeyValue;
    use opentelemetry_semantic_conventions::{
        resource::{SERVICE_NAME, SERVICE_VERSION},
        SCHEMA_URL,
    };

    opentelemetry_sdk::Resource::from_schema_url(
        [
            KeyValue::new(SERVICE_NAME, env!("CARGO_PKG_NAME")),
            KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        ],
        SCHEMA_URL,
    )
}

fn tracer() -> opentelemetry_sdk::trace::Tracer {
    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_resource(resource())
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .build()
                .unwrap(),
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

    global::set_tracer_provider(provider.clone());

    provider.tracer(env!("CARGO_PKG_NAME"))
}

fn init_tracing() {
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
                .with_tracer(tracer())
                .with_location(false),
        )
        .init();
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    init_tracing();

    let service = ServiceBuilder::new().layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                use tracing::field::Empty;

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
            })
            .on_response(|response: &Response, _latency: Duration, span: &Span| {
                span.record(OTEL_STATUS_CODE, "ok");
                span.record(HTTP_RESPONSE_STATUS_CODE, response.status().as_u16());
            }),
    );

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(service);

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
