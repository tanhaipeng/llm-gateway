mod config;
mod dispatcher;
mod mapper;
mod router;
mod types;

use axum::{
    http::Method,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::load_config;
use dispatcher::Dispatcher;
use router::{health_handler, proxy_handler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llm_gateway=debug,tower_http=debug,axum=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = load_config()?;
    info!("Loaded configuration with {} providers", config.providers.len());

    // Create dispatcher
    let dispatcher = Dispatcher::new(&config);

    // Build router
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/{provider}/v1/chat/completions", post(proxy_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        )
        .with_state(dispatcher);

    // Start server
    let addr = SocketAddr::from((
        config.server.address.parse::<std::net::IpAddr>()?,
        config.server.port,
    ));

    info!("Starting LLM Gateway on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
