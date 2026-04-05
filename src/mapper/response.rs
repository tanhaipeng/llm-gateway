use serde_json::Value;

/// 响应映射器 - 将其他 providers 格式转换为 OpenAI 格式
pub struct ResponseMapper;

impl ResponseMapper {
    /// 将目标 provider 的响应转换为 OpenAI 格式
    pub fn convert_response(
        data: &str,
        source_provider: &crate::types::Provider,
        is_stream: bool,
    ) -> Result<String, crate::types::GatewayError> {
        match source_provider {
            crate::types::Provider::Anthropic => {
                if is_stream {
                    Self::anthropic_stream_to_openai(data)
                } else {
                    Self::anthropic_to_openai(data)
                }
            }
            crate::types::Provider::OpenAI | crate::types::Provider::GoogleGemini | crate::types::Provider::Deepseek | crate::types::Provider::Custom(_) => Ok(data.to_string()), // OpenAI 兼容的 providers
        }
    }
    
    /// Anthropic 非流式响应 → OpenAI 格式
    fn anthropic_to_openai(anthropic_data: &str) -> Result<String, crate::types::GatewayError> {
        let anthropic_json: Value = serde_json::from_str(anthropic_data)?;
        
        let mut openai_json = serde_json::json!({
            "id": anthropic_json.get("id").unwrap_or(&Value::String("chatcmpl-placeholder".to_string())),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": anthropic_json.get("model").unwrap_or(&Value::String("claude-3-5-sonnet".to_string())),
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": anthropic_json.get("content").and_then(|c| c.as_str()).unwrap_or("")
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": anthropic_json.get("usage").and_then(|u| u.get("input_tokens")).and_then(|t| t.as_i64()).unwrap_or(0),
                "completion_tokens": anthropic_json.get("usage").and_then(|u| u.get("output_tokens")).and_then(|t| t.as_i64()).unwrap_or(0),
                "total_tokens": 0
            }
        });
        
        // 计算 total_tokens
        if let Some(usage) = openai_json.get("usage") {
            if let (Some(prompt), Some(completion)) = (
                usage.get("prompt_tokens").and_then(|t| t.as_i64()),
                usage.get("completion_tokens").and_then(|t| t.as_i64())
            ) {
                openai_json["usage"]["total_tokens"] = Value::Number(serde_json::Number::from(prompt + completion));
            }
        }
        
        Ok(serde_json::to_string(&openai_json)?)
    }
    
    /// Anthropic 流式响应 → OpenAI 格式
    fn anthropic_stream_to_openai(anthropic_data: &str) -> Result<String, crate::types::GatewayError> {
        if let Ok(anthropic_json) = serde_json::from_str::<Value>(anthropic_data) {
            let openai_json = match anthropic_json.get("type").and_then(|t| t.as_str()) {
                Some("message_start") => {
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "role": "assistant",
                                "content": ""
                            },
                            "finish_reason": null
                        }]
                    })
                }
                Some("content_block_delta") => {
                    let delta = anthropic_json.get("delta").and_then(|d| d.get("text"));
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "content": delta
                            },
                            "finish_reason": null
                        }]
                    })
                }
                Some("content_block_stop") => {
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": "stop"
                        }]
                    })
                }
                Some("message_stop") => {
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": "stop"
                        }]
                    })
                }
                _ => {
                    // 对于其他类型（如 ping），返回 [DONE] 作为 JSON
                    serde_json::json!("[DONE]")
                }
            };
            
            Ok(serde_json::to_string(&openai_json)?)
        } else {
            // 如果不是有效的 JSON，直接返回原数据
            Ok(anthropic_data.to_string())
        }
    }
}
