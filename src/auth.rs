use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// 认证中间件
pub async fn auth_middleware<B>(
    State(api_key): State<Option<String>>,
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, AuthError> {
    let api_key = api_key.ok_or(AuthError::NoApiKey)?;
    
    // 跳过健康检查端点
    if request.uri().path() == "/health" {
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
    NoApiKey,
    MissingAuthHeader,
    InvalidApiKey,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::NoApiKey => {
                (StatusCode::INTERNAL_SERVER_ERROR, "API key not configured")
            }
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
