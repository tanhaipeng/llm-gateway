mod auth;
mod config;
mod dispatcher;
mod logging;
mod mapper;
mod metrics;
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
use router::{health_handler, metrics_handler, proxy_handler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

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
    
    // Log each provider
    for (name, provider_config) in &config.providers {
        info!("  Provider: {}, Models: {:?}, API Key: {}", 
              name, 
              provider_config.models, 
              if provider_config.api_key.is_some() { "Set" } else { "Not set" });
    }

    // Create dispatcher
    let dispatcher = Dispatcher::new(&config);

    // Create request logger (shared state)
    let request_logger = logging::RequestLogger::new();

    // Check if authentication is enabled
    let gateway_api_key = std::env::var("GATEWAY_API_KEY").ok();
    
    // Build router with shared state
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/{provider}/v1/chat/completions", post(proxy_handler))
        .with_state((dispatcher.clone(), request_logger.clone()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        );
    
    // Add authentication middleware if API key is configured
    let app = if let Some(ref api_key) = gateway_api_key {
        info!("Authentication enabled - API key required");
        Router::new()
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler))
            .route("/{provider}/v1/chat/completions", post(proxy_handler))
            .layer(axum::middleware::from_fn_with_state(
                api_key.clone(),
                auth::auth_middleware,
            ))
            .with_state((dispatcher, request_logger))
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                    .allow_headers(Any),
            )
    } else {
        info!("Authentication disabled - no API key configured");
        app
    };

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
