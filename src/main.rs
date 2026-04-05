mod auth;
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

    // Check if authentication is enabled
    let gateway_api_key = std::env::var("GATEWAY_API_KEY").ok();
    
    // Build router
    let mut app = Router::new()
        .route("/health", get(health_handler))
        .route("/{provider}/v1/chat/completions", post(proxy_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        )
        .with_state(dispatcher);
    
    // Add authentication middleware if API key is configured
    if gateway_api_key.is_some() {
        info!("Authentication enabled - API key required");
        app = app
            .route("/{provider}/v1/chat/completions", post(proxy_handler))
            .layer(axum::middleware::from_fn_with_state(
                gateway_api_key.clone(),
                auth::auth_middleware,
            ))
            .with_state(dispatcher);
    } else {
        info!("Authentication disabled - no API key configured");
    }

    // Start server
    let addr = SocketAddr::from((
        config.server.address.parse::<std::net::IpAddr>()?,
        config.server.port,
    ));

    info!("Starting LLM Gateway on {}", addr);
    
    if gateway_api_key.is_some() {
        info!("To access the gateway, include header: Authorization: Bearer YOUR_API_KEY");
    }
    
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
