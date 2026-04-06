use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RateLimitState {
    max_requests_per_second: u64,
    window: Arc<Mutex<RateLimitWindow>>,
}

struct RateLimitWindow {
    started_at: Instant,
    count: u64,
}

impl RateLimitState {
    pub fn new(max_requests_per_second: u64) -> Self {
        Self {
            max_requests_per_second,
            window: Arc::new(Mutex::new(RateLimitWindow {
                started_at: Instant::now(),
                count: 0,
            })),
        }
    }
}

pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS || request.uri().path() == "/health" {
        return next.run(request).await;
    }

    {
        let mut window = state.window.lock().await;
        let elapsed = window.started_at.elapsed();
        if elapsed.as_secs() >= 1 {
            window.started_at = Instant::now();
            window.count = 0;
        }
        if window.count >= state.max_requests_per_second {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": {
                        "message": "Rate limit exceeded",
                        "type": "rate_limit_error",
                        "code": "429"
                    }
                })),
            )
                .into_response();
        }
        window.count += 1;
    }

    next.run(request).await
}
