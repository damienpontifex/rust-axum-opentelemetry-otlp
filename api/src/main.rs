use axum::extract::Path;
use axum::routing::get;
use axum::Router;
use std::net::Ipv4Addr;
use telemetry::TracingFor;

use tower::ServiceBuilder;

async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _guard = telemetry::init_tracing(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let service = ServiceBuilder::new().layer(telemetry::trace_layer(TracingFor::Server));

    let app = Router::new().route("/{name}", get(hello)).layer(service);

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
