use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct RateLimitState {
    bucket: Arc<Mutex<TokenBucket>>,
    rate_per_second: u64,
    capacity_mtokens: u64,
}

#[derive(Debug)]
struct TokenBucket {
    available_mtokens: u64,
    last_refill_at: Instant,
}

impl RateLimitState {
    pub fn new(max_requests_per_second: u64) -> Self {
        // 1 token = 1000 mtokens
        let capacity_mtokens = max_requests_per_second.saturating_mul(1000);
        Self {
            bucket: Arc::new(Mutex::new(TokenBucket {
                available_mtokens: capacity_mtokens,
                last_refill_at: Instant::now(),
            })),
            rate_per_second: max_requests_per_second,
            capacity_mtokens,
        }
    }

    fn try_acquire(&self, now: Instant) -> bool {
        let mut bucket = match self.bucket.lock() {
            Ok(guard) => guard,
            // Poisoned mutex should not take service down; continue with inner state.
            Err(poisoned) => poisoned.into_inner(),
        };

        refill_bucket(
            &mut bucket,
            now,
            self.rate_per_second,
            self.capacity_mtokens,
        );
        if bucket.available_mtokens < 1000 {
            return false;
        }
        bucket.available_mtokens -= 1000;
        true
    }
}

fn refill_bucket(
    bucket: &mut TokenBucket,
    now: Instant,
    rate_per_second: u64,
    capacity_mtokens: u64,
) {
    let elapsed_ms = now.duration_since(bucket.last_refill_at).as_millis() as u64;
    if elapsed_ms == 0 {
        return;
    }
    // rate_per_second / 1000 token per ms => rate_per_second mtokens per ms
    let refill_mtokens = elapsed_ms.saturating_mul(rate_per_second);
    bucket.available_mtokens = bucket
        .available_mtokens
        .saturating_add(refill_mtokens)
        .min(capacity_mtokens);
    bucket.last_refill_at = now;
}

pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS || request.uri().path() == "/health" {
        return next.run(request).await;
    }

    if !state.try_acquire(Instant::now()) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", "1")],
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

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_token_bucket_refill_and_consume() {
        let state = RateLimitState::new(2);
        let now = Instant::now();

        assert!(state.try_acquire(now));
        assert!(state.try_acquire(now));
        assert!(!state.try_acquire(now));

        // 500ms with 2rps refills 1 token
        assert!(state.try_acquire(now + Duration::from_millis(500)));
        assert!(!state.try_acquire(now + Duration::from_millis(500)));
    }
}
