use axum::routing::get;
use axum::Router;
use std::net::Ipv4Addr;

use tower::ServiceBuilder;

mod telemetry;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _guard = telemetry::init_tracing();

    let service = ServiceBuilder::new().layer(telemetry::trace_layer());

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(service);

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
