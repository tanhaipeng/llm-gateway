use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Provider;

    fn chunk(json: &str) -> Option<serde_json::Value> {
        let mut state = StreamState::new();
        ResponseMapper::convert_stream_chunk(json, &Provider::Anthropic, &mut state)
            .unwrap()
            .map(|s| serde_json::from_str(&s).unwrap())
    }

    #[test]
    fn test_message_start_extracts_model() {
        let result = chunk(r#"{"type":"message_start","message":{"id":"msg_01","model":"claude-haiku-4-5","usage":{"input_tokens":10},"role":"assistant","content":[]}}"#).unwrap();
        assert_eq!(result["model"], "claude-haiku-4-5");
        assert_eq!(result["choices"][0]["delta"]["role"], "assistant");
        // H-4: content should NOT be present in the first chunk delta
        assert!(result["choices"][0]["delta"].get("content").is_none());
    }

    #[test]
    fn test_text_delta() {
        let mut state = StreamState::new();
        // Prime state with message_start first
        let _ = ResponseMapper::convert_stream_chunk(
            r#"{"type":"message_start","message":{"id":"msg_01","model":"claude-haiku-4-5","usage":{"input_tokens":10},"role":"assistant","content":[]}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        );
        let result = ResponseMapper::convert_stream_chunk(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        ).unwrap().map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap()).unwrap();
        assert_eq!(result["choices"][0]["delta"]["content"], "Hello");
    }

    #[test]
    fn test_text_delta_choice_index_is_zero() {
        let mut state = StreamState::new();
        let _ = ResponseMapper::convert_stream_chunk(
            r#"{"type":"message_start","message":{"id":"msg_01","model":"claude-haiku-4-5","usage":{"input_tokens":10},"role":"assistant","content":[]}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        );
        let result = ResponseMapper::convert_stream_chunk(
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Hello"}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        )
        .unwrap()
        .map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap())
        .unwrap();
        assert_eq!(result["choices"][0]["index"], 0);
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
        let mut state = StreamState::new();
        let result =
            ResponseMapper::convert_stream_chunk(raw, &crate::types::Provider::OpenAI, &mut state)
                .unwrap()
                .unwrap();
        assert_eq!(result, raw);
    }

    #[test]
    fn test_anthropic_non_stream_tool_calls() {
        let anthropic = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-haiku-4-5","content":[{"type":"tool_use","id":"toolu_01","name":"search","input":{"q":"rust"}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":5}}"#;
        let result: serde_json::Value = serde_json::from_str(
            &ResponseMapper::convert_response(anthropic, &crate::types::Provider::Anthropic)
                .unwrap(),
        )
        .unwrap();
        let tc = &result["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "search");
        // arguments 应是 JSON 字符串
        let args: serde_json::Value =
            serde_json::from_str(tc["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["q"], "rust");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 5);
    }

    #[test]
    fn test_stream_state_carries_id_and_model() {
        let mut state = StreamState::new();
        // message_start sets id and model
        let start = ResponseMapper::convert_stream_chunk(
            r#"{"type":"message_start","message":{"id":"msg_real_id","model":"claude-opus-4","usage":{"input_tokens":20},"role":"assistant","content":[]}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        ).unwrap().map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap()).unwrap();
        assert_eq!(start["id"], "msg_real_id");
        assert_eq!(start["model"], "claude-opus-4");

        // subsequent chunk should use same id and model
        let delta = ResponseMapper::convert_stream_chunk(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        )
        .unwrap()
        .map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap())
        .unwrap();
        assert_eq!(delta["id"], "msg_real_id");
        assert_eq!(delta["model"], "claude-opus-4");
    }

    #[test]
    fn test_message_delta_uses_cached_prompt_tokens() {
        let mut state = StreamState::new();
        // message_start: input_tokens = 15
        let _ = ResponseMapper::convert_stream_chunk(
            r#"{"type":"message_start","message":{"id":"msg_x","model":"claude-3","usage":{"input_tokens":15},"role":"assistant","content":[]}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        );
        // message_delta: usage only has output_tokens
        let result = ResponseMapper::convert_stream_chunk(
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":30}}"#,
            &crate::types::Provider::Anthropic,
            &mut state,
        ).unwrap().map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap()).unwrap();
        // prompt_tokens should come from cached message_start value
        assert_eq!(result["usage"]["prompt_tokens"], 15);
        assert_eq!(result["usage"]["completion_tokens"], 30);
        assert_eq!(result["usage"]["total_tokens"], 45);
    }
}

/// 流式转换跨 chunk 状态（每个 Anthropic SSE 流一个实例）
#[derive(Debug, Default)]
pub struct StreamState {
    /// 从 message_start 中提取的消息 ID
    pub message_id: String,
    /// 从 message_start 中提取的模型名
    pub model: String,
    /// 从 message_start 中缓存的 input_tokens（message_delta 中没有该字段）
    pub prompt_tokens: u64,
    /// 当前已出现的 tool_call 数量（用于生成独立的 tool_calls 索引）
    pub tool_call_index: u32,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            message_id: "chatcmpl-placeholder".to_string(),
            model: "claude".to_string(),
            prompt_tokens: 0,
            tool_call_index: 0,
        }
    }
}

/// 响应映射器 - 将其他 providers 格式转换为 OpenAI 格式
pub struct ResponseMapper;

impl ResponseMapper {
    /// 将目标 provider 的非流式响应转换为 OpenAI 格式
    pub fn convert_response(
        data: &str,
        source_provider: &crate::types::Provider,
    ) -> Result<String, crate::types::GatewayError> {
        match source_provider {
            crate::types::Provider::Anthropic => Self::anthropic_to_openai(data),
            _ => Ok(data.to_string()),
        }
    }

    /// 将单个流式 JSON chunk 转换为 OpenAI 格式
    /// state 跨 chunk 保持 message_id / model / prompt_tokens / tool_call_index
    /// 返回 Ok(None) 表示该事件应跳过（不发给客户端）
    /// 返回 Ok(Some(json_string)) 表示转换后的 chunk
    pub fn convert_stream_chunk(
        json_str: &str,
        source_provider: &crate::types::Provider,
        state: &mut StreamState,
    ) -> Result<Option<String>, crate::types::GatewayError> {
        match source_provider {
            crate::types::Provider::Anthropic => Self::anthropic_chunk_to_openai(json_str, state),
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
                arr.iter()
                    .any(|item| item.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
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
                            item.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
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
                    let arguments =
                        serde_json::to_string(item.get("input").unwrap_or(&serde_json::json!({})))
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
    /// state 跨调用保持消息 ID、model、prompt_tokens、tool_call_index
    /// 返回 Ok(None) 表示该事件静默跳过
    fn anthropic_chunk_to_openai(
        json_str: &str,
        state: &mut StreamState,
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
                // 提取消息 ID 和 model，存入 state 供后续 chunk 使用
                let message = anthropic_json.get("message");
                let message_id = message
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .unwrap_or("chatcmpl-placeholder");
                let model = message
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("claude");

                // 缓存 input_tokens 供 message_delta 使用（message_delta 的 usage 中无此字段）
                let prompt_tokens = message
                    .and_then(|m| m.get("usage"))
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);

                state.message_id = message_id.to_string();
                state.model = model.to_string();
                state.prompt_tokens = prompt_tokens;
                state.tool_call_index = 0;

                Some(serde_json::json!({
                    "id": message_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant" },
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
                let content_block = anthropic_json.get("content_block");

                if let Some(block) = content_block {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let tool_id = block.get("id").and_then(|id| id.as_str()).unwrap_or("");
                        let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");

                        // 使用独立的 tool_call_index（不用 content_block index，避免与文本块混淆）
                        let tc_index = state.tool_call_index;
                        state.tool_call_index += 1;

                        Some(serde_json::json!({
                            "id": state.message_id,
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": state.model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [{
                                        "index": tc_index,
                                        "id": tool_id,
                                        "type": "function",
                                        "function": { "name": tool_name, "arguments": "" }
                                    }]
                                },
                                "finish_reason": null
                            }]
                        }))
                    } else {
                        // text block 开始，发空 delta（跳过减少不必要的 chunk）
                        None
                    }
                } else {
                    None
                }
            }

            Some("content_block_delta") => {
                let delta = anthropic_json.get("delta");

                if let Some(delta_obj) = delta {
                    let delta_type = delta_obj.get("type").and_then(|t| t.as_str());
                    match delta_type {
                        Some("text_delta") => {
                            let text = delta_obj.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            Some(serde_json::json!({
                                "id": state.message_id,
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": state.model,
                                "choices": [{
                                    "index": 0,
                                    "delta": { "content": text },
                                    "finish_reason": null
                                }]
                            }))
                        }
                        Some("input_json_delta") => {
                            // 工具调用参数增量：tool_call_index 已在 content_block_start 递增
                            // 这里用 tool_call_index - 1 作为当前正在流式输出的工具的索引
                            let tc_index = state.tool_call_index.saturating_sub(1);
                            let partial_json = delta_obj
                                .get("partial_json")
                                .and_then(|j| j.as_str())
                                .unwrap_or("");
                            Some(serde_json::json!({
                                "id": state.message_id,
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": state.model,
                                "choices": [{
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [{
                                            "index": tc_index,
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

                // message_delta 的 usage 只有 output_tokens；prompt_tokens 从 state 中取缓存值
                let prompt_tokens = state.prompt_tokens;
                let completion_tokens = usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);

                Some(serde_json::json!({
                    "id": state.message_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": state.model,
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
                    "id": state.message_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": state.model,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "error"
                    }],
                    "error": { "message": error_msg, "type": "api_error" }
                }))
            }

            _ => {
                // 未知事件类型，静默跳过
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
