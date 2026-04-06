use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
pub struct AuthState {
    pub api_key: String,
    pub metrics_require_auth: bool,
}

/// 认证中间件
pub async fn auth_middleware(
    State(auth): State<AuthState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    // 健康检查始终免认证
    let path = request.uri().path();
    if path == "/health" {
        return Ok(next.run(request).await);
    }
    if path == "/metrics" && !auth.metrics_require_auth {
        return Ok(next.run(request).await);
    }

    // H-3: 跳过 OPTIONS 预检请求（CORS preflight），让 CORS 中间件处理
    if request.method() == Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    // 获取 Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(AuthError::MissingAuthHeader)?;

    // 验证 API key（支持 Bearer token 格式）
    let provided_key = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    let provided = provided_key.as_bytes();
    let expected = auth.api_key.as_bytes();
    let is_valid = provided.len() == expected.len() && provided.ct_eq(expected).into();

    if !is_valid {
        return Err(AuthError::InvalidApiKey);
    }

    Ok(next.run(request).await)
}

#[derive(Debug)]
pub enum AuthError {
    MissingAuthHeader,
    InvalidApiKey,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingAuthHeader => {
                (StatusCode::UNAUTHORIZED, "Missing authorization header")
            }
            AuthError::InvalidApiKey => (StatusCode::UNAUTHORIZED, "Invalid API key"),
        };

        let body = serde_json::json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "code": status.as_u16().to_string()
            }
        });

        (status, axum::Json(body)).into_response()
    }
}
