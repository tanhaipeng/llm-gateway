use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// 认证中间件
pub async fn auth_middleware(
    State(api_key): State<String>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    // 跳过健康检查和监控端点
    let path = request.uri().path();
    if path == "/health" || path == "/metrics" {
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
    let provided_key = auth_header
        .strip_prefix("Bearer ")
        .unwrap_or(auth_header);
    
    if provided_key != api_key {
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
            AuthError::InvalidApiKey => {
                (StatusCode::UNAUTHORIZED, "Invalid API key")
            }
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
