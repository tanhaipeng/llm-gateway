mod auth;
mod config;
mod dispatcher;
mod guard;
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
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::load_config;
use dispatcher::Dispatcher;
use guard::RateLimitState;
use router::{health_handler, metrics_handler, proxy_handler, AppState};

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
    info!(
        "Loaded configuration with {} providers",
        config.providers.len()
    );
    for (name, pc) in &config.providers {
        info!(
            "  Provider: {}, Models: {:?}, API Key: {}",
            name,
            pc.models,
            if pc.api_key.is_some() {
                "Set"
            } else {
                "Not set"
            }
        );
    }

    let dispatcher = Dispatcher::new(&config)?;
    let request_logger = logging::RequestLogger::new();
    let gateway_api_key = std::env::var("GATEWAY_API_KEY").ok();
    let request_timeout_seconds = config.server.request_timeout_seconds.max(1);
    if config.server.request_timeout_seconds == 0 {
        warn!("server.request-timeout-seconds=0 is invalid, forcing to 1s");
    }

    let mut app = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/{provider}/v1/chat/completions", post(proxy_handler))
        // 覆盖 Axum 默认 2MB body 限制为 10MB
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(AppState {
            dispatcher,
            request_logger,
            start_time,
            request_timeout_seconds,
        })
        .layer(build_cors_layer(&config.server.cors));

    if let Some(max_in_flight) = config.server.limits.max_in_flight_requests {
        if max_in_flight > 0 {
            info!("Global concurrency limit enabled: {}", max_in_flight);
            app = app.layer(GlobalConcurrencyLimitLayer::new(max_in_flight));
        }
    }

    if let Some(rps) = config.server.limits.max_requests_per_second {
        if rps > 0 {
            info!("Global rate limit enabled: {} req/s", rps);
            app = app.layer(axum::middleware::from_fn_with_state(
                RateLimitState::new(rps),
                guard::rate_limit_middleware,
            ));
        }
    }

    let app = if let Some(ref api_key) = gateway_api_key {
        info!("Authentication enabled");
        app.layer(axum::middleware::from_fn_with_state(
            auth::AuthState {
                api_key: api_key.clone(),
                metrics_require_auth: config.server.metrics.require_auth,
            },
            auth::auth_middleware,
        ))
    } else {
        info!("Authentication disabled");
        if config.server.metrics.require_auth {
            warn!("server.metrics.require-auth=true is ignored because GATEWAY_API_KEY is not set");
        }
        app
    };

    let addr = SocketAddr::from((
        config.server.address.parse::<std::net::IpAddr>()?,
        config.server.port,
    ));
    info!("Starting LLM Gateway on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn build_cors_layer(cors: &crate::types::CorsConfig) -> CorsLayer {
    let base = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);
    if cors.allow_any_origin {
        return base.allow_origin(Any);
    }
    let origins: Vec<axum::http::HeaderValue> = cors
        .allow_origins
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    if origins.is_empty() {
        // backend-only 默认关闭跨域
        return base;
    }
    base.allow_origin(AllowOrigin::list(origins))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!(error = %e, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sigterm) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sigterm.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received, starting graceful shutdown");
}
