use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Provider;

    fn chunk(json: &str) -> Option<serde_json::Value> {
        ResponseMapper::convert_stream_chunk(json, &Provider::Anthropic)
            .unwrap()
            .map(|s| serde_json::from_str(&s).unwrap())
    }

    #[test]
    fn test_message_start_extracts_model() {
        let result = chunk(r#"{"type":"message_start","message":{"id":"msg_01","model":"claude-haiku-4-5","usage":{"input_tokens":10},"role":"assistant","content":[]}}"#).unwrap();
        assert_eq!(result["model"], "claude-haiku-4-5");
        assert_eq!(result["choices"][0]["delta"]["role"], "assistant");
    }

    #[test]
    fn test_text_delta() {
        let result = chunk(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#).unwrap();
        assert_eq!(result["choices"][0]["delta"]["content"], "Hello");
    }

    #[test]
    fn test_tool_use_start() {
        let result = chunk(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"search"}}"#).unwrap();
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["id"], "toolu_01");
        assert_eq!(tc["function"]["name"], "search");
    }

    #[test]
    fn test_input_json_delta() {
        let result = chunk(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"rust\"}"}}"#).unwrap();
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["function"]["arguments"], "{\"q\":\"rust\"}");
    }

    #[test]
    fn test_message_delta_with_usage() {
        let result = chunk(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#).unwrap();
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["completion_tokens"], 42);
    }

    #[test]
    fn test_ping_skipped() {
        assert!(chunk(r#"{"type":"ping"}"#).is_none());
    }

    #[test]
    fn test_content_block_stop_skipped() {
        assert!(chunk(r#"{"type":"content_block_stop","index":0}"#).is_none());
    }

    #[test]
    fn test_message_stop_skipped() {
        assert!(chunk(r#"{"type":"message_stop"}"#).is_none());
    }

    #[test]
    fn test_non_anthropic_passthrough() {
        let raw = r#"{"id":"chatcmpl-1","choices":[{"delta":{"content":"Hi"}}]}"#;
        let result = ResponseMapper::convert_stream_chunk(raw, &Provider::OpenAI)
            .unwrap()
            .unwrap();
        assert_eq!(result, raw);
    }

    #[test]
    fn test_anthropic_non_stream_tool_calls() {
        let anthropic = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-haiku-4-5","content":[{"type":"tool_use","id":"toolu_01","name":"search","input":{"q":"rust"}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":5}}"#;
        let result: serde_json::Value = serde_json::from_str(
            &ResponseMapper::convert_response(anthropic, &Provider::Anthropic, false).unwrap()
        ).unwrap();
        let tc = &result["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "search");
        // arguments 应是 JSON 字符串
        let args: serde_json::Value = serde_json::from_str(tc["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["q"], "rust");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 5);
    }
}

/// 响应映射器 - 将其他 providers 格式转换为 OpenAI 格式
pub struct ResponseMapper;

impl ResponseMapper {
    /// 将目标 provider 的非流式响应转换为 OpenAI 格式
    pub fn convert_response(
        data: &str,
        source_provider: &crate::types::Provider,
        _is_stream: bool,
    ) -> Result<String, crate::types::GatewayError> {
        match source_provider {
            crate::types::Provider::Anthropic => Self::anthropic_to_openai(data),
            _ => Ok(data.to_string()),
        }
    }

    /// 将单个流式 JSON chunk 转换为 OpenAI 格式
    /// 返回 Ok(None) 表示该事件应跳过（不发给客户端）
    /// 返回 Ok(Some(json_string)) 表示转换后的 chunk
    pub fn convert_stream_chunk(
        json_str: &str,
        source_provider: &crate::types::Provider,
    ) -> Result<Option<String>, crate::types::GatewayError> {
        match source_provider {
            crate::types::Provider::Anthropic => Self::anthropic_chunk_to_openai(json_str),
            _ => {
                // OpenAI 兼容 provider 直接透传
                Ok(Some(json_str.to_string()))
            }
        }
    }

    /// Anthropic 非流式响应 → OpenAI 格式
    fn anthropic_to_openai(anthropic_data: &str) -> Result<String, crate::types::GatewayError> {
        let anthropic_json: Value = serde_json::from_str(anthropic_data)?;

        let has_tool_calls = anthropic_json
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                })
            })
            .unwrap_or(false);

        let mut choices = Vec::new();

        if has_tool_calls {
            let tool_calls = Self::extract_tool_calls_from_response(&anthropic_json)?;
            // 提取文本内容（如果有的话）
            let text_content = Self::extract_text_content(&anthropic_json);
            choices.push(serde_json::json!({
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": if text_content.is_empty() { Value::Null } else { Value::String(text_content) },
                    "tool_calls": tool_calls
                },
                "finish_reason": Self::map_stop_reason(anthropic_json.get("stop_reason"))
            }));
        } else {
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

        let prompt_tokens = anthropic_json
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let completion_tokens = anthropic_json
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        let openai_json = serde_json::json!({
            "id": anthropic_json.get("id").unwrap_or(&Value::String("chatcmpl-placeholder".to_string())),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": anthropic_json.get("model").unwrap_or(&Value::String("claude".to_string())),
            "choices": choices,
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens
            }
        });

        Ok(serde_json::to_string(&openai_json)?)
    }

    /// 提取文本内容
    fn extract_text_content(anthropic_json: &Value) -> String {
        if let Some(content) = anthropic_json.get("content") {
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
            if let Some(arr) = content.as_array() {
                return arr
                    .iter()
                    .filter_map(|item| {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
            }
        }
        String::new()
    }

    /// 从非流式 Anthropic 响应中提取 tool_calls（转为 OpenAI 格式）
    fn extract_tool_calls_from_response(
        anthropic_json: &Value,
    ) -> Result<Value, crate::types::GatewayError> {
        let mut tool_calls = Vec::new();

        if let Some(content) = anthropic_json.get("content").and_then(|c| c.as_array()) {
            for (index, item) in content.iter().enumerate() {
                if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    // Anthropic tool_use → OpenAI tool_calls
                    let arguments = serde_json::to_string(
                        item.get("input").unwrap_or(&serde_json::json!({})),
                    )
                    .unwrap_or_else(|_| "{}".to_string());

                    tool_calls.push(serde_json::json!({
                        "index": index,
                        "id": item.get("id").unwrap_or(&Value::String(format!("call_{}", index))),
                        "type": "function",
                        "function": {
                            "name": item.get("name").unwrap_or(&Value::String(String::new())),
                            "arguments": arguments
                        }
                    }));
                }
            }
        }

        Ok(Value::Array(tool_calls))
    }

    /// 映射 Anthropic stop_reason → OpenAI finish_reason
    fn map_stop_reason(stop_reason: Option<&Value>) -> Value {
        match stop_reason.and_then(|s| s.as_str()) {
            Some("end_turn") | Some("stop_sequence") => Value::String("stop".to_string()),
            Some("max_tokens") => Value::String("length".to_string()),
            Some("tool_use") => Value::String("tool_calls".to_string()),
            _ => Value::String("stop".to_string()),
        }
    }

    fn map_stop_reason_str(stop_reason: &str) -> &'static str {
        match stop_reason {
            "end_turn" | "stop_sequence" => "stop",
            "max_tokens" => "length",
            "tool_use" => "tool_calls",
            _ => "stop",
        }
    }

    /// 将单个 Anthropic 流式 JSON chunk 转换为 OpenAI chunk 格式
    /// 返回 Ok(None) 表示该事件静默跳过
    fn anthropic_chunk_to_openai(
        json_str: &str,
    ) -> Result<Option<String>, crate::types::GatewayError> {
        let anthropic_json: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => {
                // 非 JSON 数据直接透传
                return Ok(Some(json_str.to_string()));
            }
        };

        let event_type = anthropic_json.get("type").and_then(|t| t.as_str());

        let result = match event_type {
            Some("message_start") => {
                // 提取消息 ID 和 model
                let message = anthropic_json.get("message");
                let message_id = message
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .unwrap_or("chatcmpl-placeholder");
                let model = message
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("claude");

                // message_start 中也可能包含 input_tokens（预填充 token 数）
                let prompt_tokens = message
                    .and_then(|m| m.get("usage"))
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);

                Some(serde_json::json!({
                    "id": message_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "content": "" },
                        "finish_reason": null
                    }],
                    // 预填充 token 数（部分客户端依赖此字段）
                    "usage": if prompt_tokens > 0 {
                        serde_json::json!({ "prompt_tokens": prompt_tokens, "completion_tokens": 0, "total_tokens": prompt_tokens })
                    } else {
                        Value::Null
                    }
                }))
            }

            Some("content_block_start") => {
                let index = anthropic_json
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(0) as u32;
                let content_block = anthropic_json.get("content_block");

                if let Some(block) = content_block {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let tool_id =
                            block.get("id").and_then(|id| id.as_str()).unwrap_or("");
                        let tool_name =
                            block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        Some(serde_json::json!({
                            "id": "chatcmpl-placeholder",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": "claude",
                            "choices": [{
                                "index": index,
                                "delta": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [{
                                        "index": index,
                                        "id": tool_id,
                                        "type": "function",
                                        "function": { "name": tool_name, "arguments": "" }
                                    }]
                                },
                                "finish_reason": null
                            }]
                        }))
                    } else {
                        // text block 开始，发空 delta
                        None // 跳过，减少不必要的 chunk
                    }
                } else {
                    None
                }
            }

            Some("content_block_delta") => {
                let index = anthropic_json
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(0) as u32;
                let delta = anthropic_json.get("delta");

                if let Some(delta_obj) = delta {
                    let delta_type = delta_obj.get("type").and_then(|t| t.as_str());
                    match delta_type {
                        Some("text_delta") => {
                            let text = delta_obj
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            Some(serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "claude",
                                "choices": [{
                                    "index": index,
                                    "delta": { "content": text },
                                    "finish_reason": null
                                }]
                            }))
                        }
                        Some("input_json_delta") => {
                            // 工具调用参数增量
                            let partial_json = delta_obj
                                .get("partial_json")
                                .and_then(|j| j.as_str())
                                .unwrap_or("");
                            Some(serde_json::json!({
                                "id": "chatcmpl-placeholder",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "claude",
                                "choices": [{
                                    "index": index,
                                    "delta": {
                                        "tool_calls": [{
                                            "index": index,
                                            "function": { "arguments": partial_json }
                                        }]
                                    },
                                    "finish_reason": null
                                }]
                            }))
                        }
                        _ => None, // thinking_delta 等其他类型跳过
                    }
                } else {
                    None
                }
            }

            Some("content_block_stop") => {
                // 内容块结束，不需要发给客户端
                None
            }

            Some("message_delta") => {
                let delta = anthropic_json.get("delta");
                let usage = anthropic_json.get("usage");

                let finish_reason = delta
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|sr| sr.as_str())
                    .map(Self::map_stop_reason_str)
                    .unwrap_or("stop");

                let prompt_tokens = usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0);
                let completion_tokens = usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0);

                Some(serde_json::json!({
                    "id": "chatcmpl-placeholder",
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": "claude",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason
                    }],
                    "usage": {
                        "prompt_tokens": prompt_tokens,
                        "completion_tokens": completion_tokens,
                        "total_tokens": prompt_tokens + completion_tokens
                    }
                }))
            }

            Some("message_stop") | Some("ping") => {
                // 这两个事件不发给客户端，[DONE] 在调用方处理
                None
            }

            Some("error") => {
                let error_msg = anthropic_json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown stream error");
                Some(serde_json::json!({
                    "id": "chatcmpl-placeholder",
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": "claude",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "error"
                    }],
                    "error": { "message": error_msg, "type": "api_error" }
                }))
            }

            _ => {
                // 未知事件类型，透传原始 JSON
                tracing::debug!(event_type = ?event_type, "Unknown Anthropic event type, skipping");
                None
            }
        };

        match result {
            None => Ok(None),
            Some(v) => Ok(Some(serde_json::to_string(&v)?)),
        }
    }
}
