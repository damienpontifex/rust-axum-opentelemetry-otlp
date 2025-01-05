use axum::routing::get;
use axum::Router;
use std::net::Ipv4Addr;
use telemetry::TracingFor;

use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _guard = telemetry::init_tracing(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let service = ServiceBuilder::new().layer(telemetry::trace_layer(TracingFor::Server));

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(service);

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
