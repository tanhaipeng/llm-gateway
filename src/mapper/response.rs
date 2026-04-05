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
        
        // 处理工具调用响应
        let has_tool_calls = anthropic_json.get("content")
            .and_then(|c| c.as_array())
            .map(|arr| arr.iter().any(|item| item.get("type").and_then(|t| t.as_str()) == Some("tool_use")))
            .unwrap_or(false);

        let mut choices = Vec::new();
        
        if has_tool_calls {
            // 处理工具调用响应
            let tool_calls = Self::extract_tool_calls(&anthropic_json)?;
            choices.push(serde_json::json!({
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": tool_calls
                },
                "finish_reason": Self::map_stop_reason(anthropic_json.get("stop_reason"))
            }));
        } else {
            // 处理普通文本响应
            let content = Self::extract_text_content(&anthropic_json);
            choices.push(serde_json::json!({
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": Self::map_stop_reason(anthropic_json.get("stop_reason"))
            }));
        }
        
        let mut openai_json = serde_json::json!({
            "id": anthropic_json.get("id").unwrap_or(&Value::String("chatcmpl-placeholder".to_string())),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": anthropic_json.get("model").unwrap_or(&Value::String("claude-3-5-sonnet".to_string())),
            "choices": choices,
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
    
    /// 提取文本内容
    fn extract_text_content(anthropic_json: &Value) -> String {
        if let Some(content) = anthropic_json.get("content") {
            if let Some(content_str) = content.as_str() {
                return content_str.to_string();
            }
            if let Some(content_arr) = content.as_array() {
                let texts: Vec<&str> = content_arr
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect();
                return texts.join("");
            }
        }
        String::new()
    }
    
    /// 提取工具调用
    fn extract_tool_calls(anthropic_json: &Value) -> Result<Value, crate::types::GatewayError> {
        let mut tool_calls = Vec::new();
        
        if let Some(content) = anthropic_json.get("content") {
            if let Some(content_arr) = content.as_array() {
                for (index, item) in content_arr.iter().enumerate() {
                    if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let tool_call = serde_json::json!({
                            "index": index,
                            "id": item.get("id").unwrap_or(&Value::String(format!("call_{}", index))),
                            "type": "function",
                            "function": {
                                "name": item.get("name").unwrap_or(&Value::String("".to_string())),
                                "arguments": item.get("input").unwrap_or(&Value::String("{}".to_string()))
                            }
                        });
                        tool_calls.push(tool_call);
                    }
                }
            }
        }
        
        Ok(Value::Array(tool_calls))
    }
    
    /// 映射 Stop Reason
    fn map_stop_reason(stop_reason: Option<&Value>) -> Value {
        match stop_reason.and_then(|s| s.as_str()) {
            Some("end_turn") | Some("stop_sequence") => Value::String("stop".to_string()),
            Some("max_tokens") => Value::String("length".to_string()),
            Some("tool_use") => Value::String("tool_calls".to_string()),
            Some("refusal") => Value::String("content_filter".to_string()),
            Some("error") => Value::String("error".to_string()),
            _ => Value::String("stop".to_string()),
        }
    }
    
    /// Anthropic 流式响应 → OpenAI 格式
    fn anthropic_stream_to_openai(anthropic_data: &str) -> Result<String, crate::types::GatewayError> {
        if let Ok(anthropic_json) = serde_json::from_str::<Value>(anthropic_data) {
            let event_type = anthropic_json.get("type").and_then(|t| t.as_str());
            
            let openai_json = match event_type {
                Some("message_start") => {
                    // 提取消息ID
                    let message_id = anthropic_json
                        .get("message")
                        .and_then(|m| m.get("id"))
                        .and_then(|id| id.as_str())
                        .unwrap_or("chatcmpl-placeholder");
                    
                    serde_json::json!({
                        "id": message_id,
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
                Some("content_block_start") => {
                    let index = anthropic_json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let content_block = anthropic_json.get("content_block");
                    
                    if let Some(block) = content_block {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            // 工具调用开始
                            let tool_id = block.get("id").and_then(|id| id.as_str()).unwrap_or("");
                            let tool_name = block.get("name").and_then(|name| name.as_str()).unwrap_or("");
                            
                            serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "placeholder",
                                "choices": [{
                                    "index": index,
                                    "delta": {
                                        "role": "assistant",
                                        "content": null,
                                        "tool_calls": [{
                                            "index": index,
                                            "id": tool_id,
                                            "type": "function",
                                            "function": {
                                                "name": tool_name,
                                                "arguments": ""
                                            }
                                        }]
                                    },
                                    "finish_reason": null
                                }]
                            })
                        } else {
                            // 其他内容块开始，发送空delta
                            serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "placeholder",
                                "choices": [{
                                    "index": index,
                                    "delta": {},
                                    "finish_reason": null
                                }]
                            })
                        }
                    } else {
                        serde_json::json!("[DONE]")
                    }
                }
                Some("content_block_delta") => {
                    let index = anthropic_json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let delta = anthropic_json.get("delta");
                    
                    if let Some(delta_obj) = delta {
                        if let Some(text) = delta_obj.get("text").and_then(|t| t.as_str()) {
                            // 文本增量
                            serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "placeholder",
                                "choices": [{
                                    "index": index,
                                    "delta": {
                                        "content": text
                                    },
                                    "finish_reason": null
                                }]
                            })
                        } else if let Some(partial_json) = delta_obj.get("partial_json").and_then(|j| j.as_str()) {
                            // 工具调用参数增量
                            serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "placeholder",
                                "choices": [{
                                    "index": index,
                                    "delta": {
                                        "role": "assistant",
                                        "content": null,
                                        "tool_calls": [{
                                            "index": index,
                                            "id": null,
                                            "type": "function",
                                            "function": {
                                                "name": null,
                                                "arguments": partial_json
                                            }
                                        }]
                                    },
                                    "finish_reason": null
                                }]
                            })
                        } else {
                            // 其他增量类型（thinking_delta, signature_delta），返回空
                            serde_json::json!("[DONE]")
                        }
                    } else {
                        serde_json::json!("[DONE]")
                    }
                }
                Some("content_block_stop") => {
                    // 内容块结束，不发送特殊内容
                    serde_json::json!("[DONE]")
                }
                Some("message_delta") => {
                    let delta = anthropic_json.get("delta");
                    let usage = anthropic_json.get("usage");
                    
                    // 处理 stop_reason
                    let finish_reason = delta
                        .and_then(|d| d.get("stop_reason"))
                        .and_then(|sr| sr.as_str())
                        .map(Self::map_stop_reason_ref);
                    
                    // 处理 usage
                    let openai_usage = if let Some(usage_obj) = usage {
                        Some(serde_json::json!({
                            "prompt_tokens": usage_obj.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                            "completion_tokens": usage_obj.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                            "total_tokens": usage_obj.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0) + 
                                            usage_obj.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0)
                        }))
                    } else {
                        None
                    };
                    
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": finish_reason
                        }],
                        "usage": openai_usage
                    })
                }
                Some("message_stop") => {
                    // 消息结束，发送 [DONE]
                    serde_json::json!("[DONE]")
                }
                Some("ping") => {
                    // Ping 事件，返回空
                    serde_json::json!("[DONE]")
                }
                Some("error") => {
                    // 错误事件
                    let error_msg = anthropic_json.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");
                    
                    serde_json::json!({
                        "id": "chatcmpl-placeholder",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "placeholder",
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": "error"
                        }],
                        "error": {
                            "message": error_msg,
                            "type": "api_error"
                        }
                    })
                }
                _ => {
                    // 其他类型，返回 [DONE]
                    serde_json::json!("[DONE]")
                }
            };
            
            Ok(serde_json::to_string(&openai_json)?)
        } else {
            // 如果不是有效的 JSON，直接返回原数据
            Ok(anthropic_data.to_string())
        }
    }
    
    /// 映射 Stop Reason (引用版本)
    fn map_stop_reason_ref(stop_reason: &str) -> Value {
        match stop_reason {
            "end_turn" | "stop_sequence" => Value::String("stop".to_string()),
            "max_tokens" => Value::String("length".to_string()),
            "tool_use" => Value::String("tool_calls".to_string()),
            "refusal" => Value::String("content_filter".to_string()),
            "error" => Value::String("error".to_string()),
            _ => Value::String("stop".to_string()),
        }
    }
}
