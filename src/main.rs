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
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::load_config;
use dispatcher::Dispatcher;
use router::{health_handler, metrics_handler, proxy_handler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llm_gateway=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 记录启动时间，用于 uptime / rps 计算
    let start_time = Arc::new(std::time::Instant::now());

    let config = load_config()?;
    info!("Loaded configuration with {} providers", config.providers.len());
    for (name, pc) in &config.providers {
        info!(
            "  Provider: {}, Models: {:?}, API Key: {}",
            name,
            pc.models,
            if pc.api_key.is_some() { "Set" } else { "Not set" }
        );
    }

    let dispatcher = Dispatcher::new(&config)?;
    let request_logger = logging::RequestLogger::new();
    let gateway_api_key = std::env::var("GATEWAY_API_KEY").ok();

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/{provider}/v1/chat/completions", post(proxy_handler))
        // 覆盖 Axum 默认 2MB body 限制为 10MB
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state((dispatcher, request_logger, start_time))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        );

    let app = if let Some(ref api_key) = gateway_api_key {
        info!("Authentication enabled");
        app.layer(axum::middleware::from_fn_with_state(
            api_key.clone(),
            auth::auth_middleware,
        ))
    } else {
        info!("Authentication disabled");
        app
    };

    let addr = SocketAddr::from((
        config.server.address.parse::<std::net::IpAddr>()?,
        config.server.port,
    ));
    info!("Starting LLM Gateway on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
