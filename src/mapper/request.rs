use bytes::Bytes;
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Provider;

    fn convert(body: serde_json::Value) -> serde_json::Value {
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let out = RequestMapper::convert_request(&bytes, &Provider::Anthropic).unwrap();
        serde_json::from_slice(&out).unwrap()
    }

    fn convert_to_provider_protocol(
        body: serde_json::Value,
        provider: Provider,
    ) -> serde_json::Value {
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let out = RequestMapper::convert_request_by_protocol(&bytes, &provider, true).unwrap();
        serde_json::from_slice(&out).unwrap()
    }

    #[test]
    fn test_system_string_content() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hi"}
            ]
        }));
        assert_eq!(result["system"], "You are helpful.");
    }

    #[test]
    fn test_system_array_content() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "system", "content": [{"type": "text", "text": "You are helpful."}]},
                {"role": "user", "content": "Hi"}
            ]
        }));
        assert_eq!(result["system"], "You are helpful.");
    }

    #[test]
    fn test_multiple_system_messages_merged() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "system", "content": "Part 1."},
                {"role": "system", "content": "Part 2."},
                {"role": "user", "content": "Hi"}
            ]
        }));
        assert_eq!(result["system"], "Part 1.\n\nPart 2.");
    }

    #[test]
    fn test_tool_calls_conversion() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Beijing\"}"}
                }]}
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        let assistant_msg = &msgs[1];
        assert_eq!(assistant_msg["role"], "assistant");
        let content = assistant_msg["content"].as_array().unwrap();
        let tool_use = content.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tool_use["name"], "get_weather");
        assert_eq!(tool_use["id"], "call_abc");
        assert_eq!(tool_use["input"]["city"], "Beijing");
    }

    #[test]
    fn test_tool_result_conversion() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{}"}
                }]},
                {"role": "tool", "tool_call_id": "call_abc", "content": "Sunny, 25C"}
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        let tool_result_msg = &msgs[2];
        assert_eq!(tool_result_msg["role"], "user");
        let content = tool_result_msg["content"].as_array().unwrap();
        let tr = &content[0];
        assert_eq!(tr["type"], "tool_result");
        assert_eq!(tr["tool_use_id"], "call_abc");
        assert_eq!(tr["content"], "Sunny, 25C");
    }

    #[test]
    fn test_base64_image_mime_type() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {"type": "image_url", "image_url": {
                        "url": "data:image/png;base64,iVBORw0KGgo="
                    }}
                ]
            }]
        }));
        let msgs = result["messages"].as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        let img = content.iter().find(|b| b["type"] == "image").unwrap();
        assert_eq!(img["source"]["type"], "base64");
        assert_eq!(img["source"]["media_type"], "image/png");
        assert_eq!(img["source"]["data"], "iVBORw0KGgo=");
    }

    #[test]
    fn test_tools_conversion() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search the web",
                    "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
                }
            }]
        }));
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "search");
        assert_eq!(tools[0]["description"], "Search the web");
        assert!(tools[0]["input_schema"].is_object());
    }

    #[test]
    fn test_max_completion_tokens_fallback() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_completion_tokens": 1024
        }));
        assert_eq!(result["max_tokens"], 1024);
    }

    #[test]
    fn test_tool_choice_none_removes_tools() {
        // H-4: tool_choice:"none" should remove tools entirely
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"type": "function", "function": {"name": "search", "parameters": {}}}],
            "tool_choice": "none"
        }));
        assert!(result.get("tools").is_none() || result["tools"].is_null());
        assert!(result.get("tool_choice").is_none() || result["tool_choice"].is_null());
    }

    #[test]
    fn test_consecutive_user_messages_merged() {
        // C-6: consecutive user messages should be merged to avoid Anthropic 400
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "user", "content": "World"}
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        // Should be merged into single user message
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn test_empty_assistant_message_skipped_without_breaking_alternation() {
        // C-5: empty assistant message skip should not break user/assistant alternation
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": null},
                {"role": "user", "content": "Still here"}
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        // The two user messages should be merged (since empty assistant was skipped)
        // or appear as user -> user -> merged; either way Anthropic won't get consecutive user msgs
        // After skip: user("Hi") then user("Still here") => merged to 1
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn test_empty_user_content_skipped() {
        let body = serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{
                "role": "user",
                "content": [{"type": "unsupported_part", "value": "x"}]
            }]
        });
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let err = RequestMapper::convert_request(&bytes, &Provider::Anthropic).unwrap_err();
        assert!(matches!(err, crate::types::GatewayError::InvalidRequest(_)));
    }

    #[test]
    fn test_tool_result_array_content() {
        // H-5: tool_result content supports array format
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": {"name": "fn", "arguments": "{}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "content": [
                    {"type": "text", "text": "Result part 1"},
                    {"type": "text", "text": "Result part 2"}
                ]}
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        let tr_msg = &msgs[2];
        assert_eq!(tr_msg["role"], "user");
        let tr_block = &tr_msg["content"][0];
        assert_eq!(tr_block["type"], "tool_result");
        // content should be an array, not a string
        assert!(tr_block["content"].is_array());
    }

    #[test]
    fn test_assistant_array_text_content_preserved() {
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hi"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Part 1"},
                        {"type": "text", "text": "Part 2"}
                    ]
                }
            ]
        }));
        let msgs = result["messages"].as_array().unwrap();
        let assistant = &msgs[1];
        assert_eq!(assistant["role"], "assistant");
        let blocks = assistant["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Part 1");
        assert_eq!(blocks[1]["text"], "Part 2");
    }

    #[test]
    fn test_empty_messages_after_mapping_returns_invalid_request() {
        let body = serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{
                "role": "user",
                "content": [{"type": "unsupported_part", "value": "x"}]
            }]
        });
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let err = RequestMapper::convert_request(&bytes, &Provider::Anthropic).unwrap_err();
        match err {
            crate::types::GatewayError::InvalidRequest(msg) => {
                assert!(msg.contains("No valid messages"));
            }
            other => panic!("unexpected error: {}", other),
        }
    }

    #[test]
    fn test_invalid_tool_arguments_returns_invalid_request() {
        let body = serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hi"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "fn", "arguments": "{not-json"}
                    }]
                }
            ]
        });
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let err = RequestMapper::convert_request(&bytes, &Provider::Anthropic).unwrap_err();
        match err {
            crate::types::GatewayError::InvalidRequest(msg) => {
                assert!(msg.contains("Invalid tool call arguments JSON"));
            }
            other => panic!("unexpected error: {}", other),
        }
    }

    #[test]
    fn test_non_object_tool_arguments_returns_invalid_request() {
        let body = serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "Hi"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "fn", "arguments": "[]"}
                    }]
                }
            ]
        });
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let err = RequestMapper::convert_request(&bytes, &Provider::Anthropic).unwrap_err();
        match err {
            crate::types::GatewayError::InvalidRequest(msg) => {
                assert!(msg.contains("expected object"));
            }
            other => panic!("unexpected error: {}", other),
        }
    }

    #[test]
    fn test_chat_completions_to_responses_for_openai_provider() {
        let result = convert_to_provider_protocol(
            serde_json::json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "system", "content": "You are helpful"},
                    {"role": "user", "content": "Hello"}
                ],
                "max_tokens": 128
            }),
            Provider::OpenAI,
        );
        assert_eq!(result["model"], "gpt-4o-mini");
        assert_eq!(result["max_output_tokens"], 128);
        assert_eq!(result["instructions"], "You are helpful");
        let input = result["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_chat_completions_to_responses_keeps_model_for_any_provider() {
        let result = convert_to_provider_protocol(
            serde_json::json!({
                "model": "claude-3-5-sonnet",
                "messages": [{"role": "user", "content": "Hello Anthropic"}]
            }),
            Provider::Anthropic,
        );
        assert_eq!(result["model"], "claude-3-5-sonnet");
        assert_eq!(result["input"][0]["role"], "user");
    }

    #[test]
    fn test_chat_completions_to_responses_with_tool_calls_and_tool_output() {
        let result = convert_to_provider_protocol(
            serde_json::json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role":"user","content":"Weather in Beijing?"},
                    {"role":"assistant","content":null,"tool_calls":[
                        {"id":"call_123","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Beijing\"}"}}
                    ]},
                    {"role":"tool","tool_call_id":"call_123","content":"Sunny"}
                ]
            }),
            Provider::OpenAI,
        );
        let input = result["input"].as_array().unwrap();
        assert!(input
            .iter()
            .any(|v| v["type"] == "function_call" && v["call_id"] == "call_123"));
        assert!(input
            .iter()
            .any(|v| v["type"] == "function_call_output" && v["call_id"] == "call_123"));
    }
}

/// 请求映射器 - 将 OpenAI 格式转换为其他 providers 格式
pub struct RequestMapper;

impl RequestMapper {
    pub fn convert_request_by_protocol(
        body: &Bytes,
        target_provider: &crate::types::Provider,
        is_responses_protocol: bool,
    ) -> Result<Bytes, crate::types::GatewayError> {
        if is_responses_protocol {
            let json: Value = serde_json::from_slice(body)?;
            let responses_json = Self::chat_completions_to_responses(json)?;
            return Ok(Bytes::from(serde_json::to_vec(&responses_json)?));
        }

        Self::convert_request(body, target_provider)
    }

    /// 将 OpenAI 格式的请求体转换为目标 provider 格式
    pub fn convert_request(
        body: &Bytes,
        target_provider: &crate::types::Provider,
    ) -> Result<Bytes, crate::types::GatewayError> {
        let json: Value = serde_json::from_slice(body)?;

        match target_provider {
            crate::types::Provider::Anthropic => Self::openai_to_anthropic(json),
            crate::types::Provider::OpenAI
            | crate::types::Provider::GoogleGemini
            | crate::types::Provider::Deepseek
            | crate::types::Provider::Custom(_) => Ok(body.clone()),
        }
    }

    fn chat_completions_to_responses(
        chat_json: Value,
    ) -> Result<Value, crate::types::GatewayError> {
        let model = chat_json
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("gpt-4o-mini")
            .to_string();

        let messages = chat_json
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if messages.is_empty() {
            return Err(crate::types::GatewayError::InvalidRequest(
                "Chat completions request must contain non-empty `messages`".to_string(),
            ));
        }

        let mut instructions: Vec<String> = Vec::new();
        let mut input: Vec<Value> = Vec::new();

        for msg in &messages {
            let role = match msg.get("role").and_then(|r| r.as_str()) {
                Some(r) => r,
                None => continue,
            };
            let content = msg.get("content").unwrap_or(&Value::Null);
            if role == "system" {
                let text = Self::extract_text_content(Some(content));
                if !text.is_empty() {
                    instructions.push(text);
                }
                continue;
            }
            match role {
                "tool" => {
                    let call_id = msg
                        .get("tool_call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if call_id.is_empty() {
                        continue;
                    }
                    let output = Self::chat_tool_output_to_responses_output(content);
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": output
                    }));
                }
                "assistant" => {
                    let mapped_content = Self::chat_content_to_responses_input(content);
                    if !mapped_content.is_empty() {
                        input.push(serde_json::json!({
                            "role": "assistant",
                            "content": mapped_content
                        }));
                    }

                    if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tool_calls {
                            let call_id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let arguments = tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}")
                                .to_string();

                            if !call_id.is_empty() && !name.is_empty() {
                                input.push(serde_json::json!({
                                    "type": "function_call",
                                    "call_id": call_id,
                                    "name": name,
                                    "arguments": arguments
                                }));
                            }
                        }
                    }
                }
                _ => {
                    let mapped_content = Self::chat_content_to_responses_input(content);
                    if mapped_content.is_empty() {
                        continue;
                    }
                    input.push(serde_json::json!({
                        "role": "user",
                        "content": mapped_content
                    }));
                }
            }
        }

        if input.is_empty() {
            return Err(crate::types::GatewayError::InvalidRequest(
                "No valid messages after completions->responses mapping".to_string(),
            ));
        }

        let mut out = serde_json::json!({
            "model": model,
            "input": input,
            "stream": chat_json.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
        });

        if !instructions.is_empty() {
            out["instructions"] = Value::String(instructions.join("\n\n"));
        }

        if let Some(v) = chat_json.get("max_tokens") {
            out["max_output_tokens"] = v.clone();
        }
        if let Some(v) = chat_json.get("temperature") {
            out["temperature"] = v.clone();
        }
        if let Some(v) = chat_json.get("top_p") {
            out["top_p"] = v.clone();
        }
        if let Some(v) = chat_json.get("tools") {
            out["tools"] = v.clone();
        }
        if let Some(v) = chat_json.get("tool_choice") {
            out["tool_choice"] = v.clone();
        }
        if let Some(v) = chat_json.get("parallel_tool_calls") {
            out["parallel_tool_calls"] = v.clone();
        }
        if let Some(v) = chat_json.get("metadata") {
            out["metadata"] = v.clone();
        }

        Ok(out)
    }

    fn chat_content_to_responses_input(content: &Value) -> Vec<Value> {
        match content {
            Value::String(s) => {
                if s.is_empty() {
                    Vec::new()
                } else {
                    vec![serde_json::json!({
                        "type": "input_text",
                        "text": s
                    })]
                }
            }
            Value::Array(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    match part.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                out.push(serde_json::json!({
                                    "type": "input_text",
                                    "text": text
                                }));
                            }
                        }
                        Some("image_url") => {
                            if let Some(url) = part
                                .get("image_url")
                                .and_then(|v| v.get("url"))
                                .and_then(|v| v.as_str())
                            {
                                out.push(serde_json::json!({
                                    "type": "input_image",
                                    "image_url": url
                                }));
                            }
                        }
                        _ => {}
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn chat_tool_output_to_responses_output(content: &Value) -> Value {
        match content {
            Value::String(s) => Value::String(s.clone()),
            Value::Array(parts) => {
                let mut texts = Vec::new();
                for part in parts {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                }
                if texts.is_empty() {
                    Value::String(String::new())
                } else {
                    Value::String(texts.join(""))
                }
            }
            other => Value::String(other.to_string()),
        }
    }

    /// OpenAI → Anthropic 请求格式转换
    fn openai_to_anthropic(openai_json: Value) -> Result<Bytes, crate::types::GatewayError> {
        // M-1: 直接使用模型名，不做无意义的版本映射
        let model = openai_json
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("claude-3-5-sonnet");

        let mut anthropic_json = serde_json::json!({
            "model": model,
            "max_tokens": openai_json.get("max_tokens")
                .or_else(|| openai_json.get("max_completion_tokens"))
                .unwrap_or(&Value::Number(serde_json::Number::from(4096u32))),
            "stream": openai_json.get("stream").unwrap_or(&Value::Bool(false))
        });

        // 可选参数映射
        if let Some(temp) = openai_json.get("temperature") {
            anthropic_json["temperature"] = temp.clone();
        }

        if let Some(top_p) = openai_json.get("top_p") {
            anthropic_json["top_p"] = top_p.clone();
        }

        if let Some(stop) = openai_json.get("stop") {
            let stop_sequences: Vec<Value> = match stop {
                Value::String(s) => vec![Value::String(s.clone())],
                Value::Array(arr) => arr.clone(),
                _ => vec![],
            };
            anthropic_json["stop_sequences"] = Value::Array(stop_sequences);
        }

        // 转换 tools（OpenAI functions → Anthropic tools）
        if let Some(tools) = openai_json.get("tools") {
            if let Some(tools_arr) = tools.as_array() {
                let anthropic_tools: Vec<Value> = tools_arr
                    .iter()
                    .filter_map(|tool| {
                        // OpenAI: {type: "function", function: {name, description, parameters}}
                        // Anthropic: {name, description, input_schema}
                        let func = tool.get("function")?;
                        Some(serde_json::json!({
                            "name": func.get("name").unwrap_or(&Value::String(String::new())),
                            "description": func.get("description").unwrap_or(&Value::String(String::new())),
                            "input_schema": func.get("parameters").unwrap_or(&serde_json::json!({"type": "object", "properties": {}}))
                        }))
                    })
                    .collect();
                if !anthropic_tools.is_empty() {
                    anthropic_json["tools"] = Value::Array(anthropic_tools);
                }
            }
        }

        // tool_choice 转换
        // H-4: tool_choice:"none" → 移除 tools 字段且不传 tool_choice（Anthropic 不支持 none）
        let mut remove_tools = false;
        if let Some(tool_choice) = openai_json.get("tool_choice") {
            match tool_choice {
                Value::String(s) => match s.as_str() {
                    "auto" => {
                        anthropic_json["tool_choice"] = serde_json::json!({"type": "auto"});
                    }
                    "required" => {
                        anthropic_json["tool_choice"] = serde_json::json!({"type": "any"});
                    }
                    "none" => {
                        // Anthropic 不支持 none，移除 tools 整个字段
                        remove_tools = true;
                    }
                    _ => {}
                },
                Value::Object(obj) => {
                    // {"type": "function", "function": {"name": "xxx"}}
                    if let Some(func) = obj.get("function") {
                        if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                            anthropic_json["tool_choice"] =
                                serde_json::json!({"type": "tool", "name": name});
                        }
                    }
                }
                _ => {}
            }
        }
        if remove_tools {
            if let Some(obj) = anthropic_json.as_object_mut() {
                obj.remove("tools");
            }
        }

        // 解析消息列表，提取 system prompt 并转换消息格式
        let empty_messages: Vec<Value> = vec![];
        let messages = openai_json
            .get("messages")
            .and_then(|m| m.as_array())
            .unwrap_or(&empty_messages);

        let mut anthropic_messages: Vec<Value> = Vec::new();
        let mut system_parts: Vec<String> = Vec::new();

        for msg in messages {
            let role = match msg.get("role").and_then(|r| r.as_str()) {
                Some(r) => r,
                None => continue,
            };

            match role {
                "system" => {
                    // 支持字符串和数组两种 content 格式，收集所有 system 消息合并
                    let text = Self::extract_text_content(msg.get("content"));
                    if !text.is_empty() {
                        system_parts.push(text);
                    }
                }
                "user" => {
                    let content = Self::convert_openai_user_content(msg.get("content"));
                    if Self::is_empty_content(&content) {
                        continue;
                    }
                    // C-6: 如果上一条消息已经是 user，将内容合并，避免连续 user 消息触发 Anthropic 400
                    let last_is_user = anthropic_messages.last().map_or(false, |m| {
                        m.get("role").and_then(|r| r.as_str()) == Some("user")
                    });
                    if last_is_user {
                        if let Some(last) = anthropic_messages.last_mut() {
                            // 将新的 user 内容追加到上一条消息
                            match (&mut last["content"], &content) {
                                (Value::Array(existing), Value::Array(new_parts)) => {
                                    existing.extend(new_parts.clone());
                                }
                                (Value::Array(existing), Value::String(s)) => {
                                    if !s.is_empty() {
                                        existing
                                            .push(serde_json::json!({"type": "text", "text": s}));
                                    }
                                }
                                (Value::String(existing), Value::String(s)) => {
                                    if !s.is_empty() {
                                        let merged = format!("{}\n{}", existing, s);
                                        last["content"] = Value::String(merged);
                                    }
                                }
                                (Value::String(existing_str), Value::Array(new_parts)) => {
                                    let mut blocks = vec![
                                        serde_json::json!({"type": "text", "text": existing_str.clone()}),
                                    ];
                                    blocks.extend(new_parts.clone());
                                    last["content"] = Value::Array(blocks);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        anthropic_messages.push(serde_json::json!({
                            "role": "user",
                            "content": content
                        }));
                    }
                }
                "assistant" => {
                    let mut content_blocks =
                        Self::convert_openai_assistant_content(msg.get("content"));

                    // 转换 tool_calls → Anthropic tool_use 内容块
                    // OpenAI: [{id, type:"function", function:{name, arguments}}]
                    // Anthropic: [{type:"tool_use", id, name, input}]
                    if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()) {
                        for tc in tool_calls {
                            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let func = tc.get("function");
                            let name = func
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                .unwrap_or("");
                            // arguments 是 JSON 字符串，需要解析为对象
                            let input = match func
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())
                            {
                                Some(raw) => Self::parse_tool_arguments(raw)?,
                                None => serde_json::json!({}),
                            };
                            content_blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input
                            }));
                        }
                    }

                    // H-1: 不插入空文本块；如果 content_blocks 为空则跳过此消息
                    // Anthropic 不接受空 text 块，也不接受空 content 数组
                    // C-5: 跳过空 assistant 消息时需要继续处理后续消息，不会破坏交替顺序
                    // （连续 assistant 消息合并，避免触发 Anthropic 400）
                    if content_blocks.is_empty() {
                        continue;
                    }

                    // C-6: 如果上一条消息已经是 assistant，将内容块合并
                    let last_is_assistant = anthropic_messages.last().map_or(false, |m| {
                        m.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    });
                    if last_is_assistant {
                        if let Some(last) = anthropic_messages.last_mut() {
                            if let Some(arr) = last["content"].as_array_mut() {
                                arr.extend(content_blocks);
                            }
                        }
                    } else {
                        anthropic_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": content_blocks
                        }));
                    }
                }
                "tool" => {
                    // OpenAI tool 结果消息 → Anthropic tool_result
                    // OpenAI: {role:"tool", tool_call_id, content}
                    // Anthropic: {role:"user", content:[{type:"tool_result", tool_use_id, content}]}
                    let tool_use_id = msg
                        .get("tool_call_id")
                        .and_then(|id| id.as_str())
                        .unwrap_or("");

                    // H-5: content 支持字符串和数组两种格式
                    let result_content: Value = match msg.get("content") {
                        Some(Value::String(s)) => Value::String(s.clone()),
                        Some(Value::Array(arr)) => {
                            // 数组格式直接透传（Anthropic tool_result content 支持数组）
                            Value::Array(arr.clone())
                        }
                        _ => Value::String(String::new()),
                    };

                    // 如果前一条消息也是 tool_result user 消息，合并进去
                    let last_is_tool_result = anthropic_messages.last().map_or(false, |last| {
                        last.get("role").and_then(|r| r.as_str()) == Some("user")
                            && last
                                .get("content")
                                .and_then(|c| c.as_array())
                                .map_or(false, |arr| {
                                    arr.first()
                                        .and_then(|b| b.get("type"))
                                        .and_then(|t| t.as_str())
                                        == Some("tool_result")
                                })
                    });

                    let tool_result_block = serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": result_content
                    });

                    if last_is_tool_result {
                        if let Some(last) = anthropic_messages.last_mut() {
                            if let Some(arr) = last["content"].as_array_mut() {
                                arr.push(tool_result_block);
                            }
                        }
                    } else {
                        anthropic_messages.push(serde_json::json!({
                            "role": "user",
                            "content": [tool_result_block]
                        }));
                    }
                }
                _ => {}
            }
        }

        anthropic_json["messages"] = Value::Array(anthropic_messages);
        if anthropic_json["messages"]
            .as_array()
            .is_some_and(|arr| arr.is_empty())
        {
            return Err(crate::types::GatewayError::InvalidRequest(
                "No valid messages after request mapping".to_string(),
            ));
        }

        // 合并所有 system 消息
        if !system_parts.is_empty() {
            anthropic_json["system"] = Value::String(system_parts.join("\n\n"));
        }

        let result = serde_json::to_vec(&anthropic_json)?;
        Ok(Bytes::from(result))
    }

    /// 提取消息 content 字段的纯文本（支持字符串和数组格式）
    fn extract_text_content(content: Option<&Value>) -> String {
        match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => {
                let texts: Vec<String> = parts
                    .iter()
                    .filter_map(|p| {
                        if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                            p.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                texts.join("")
            }
            _ => String::new(),
        }
    }

    /// 转换 OpenAI 用户消息内容（支持文本、图片 URL、Base64 图片）
    fn convert_openai_user_content(content: Option<&Value>) -> Value {
        match content {
            Some(Value::String(text)) => Value::String(text.clone()),
            Some(Value::Array(parts)) => {
                let mut blocks = Vec::new();
                for part in parts {
                    let type_ = match part.get("type").and_then(|t| t.as_str()) {
                        Some(t) => t,
                        None => continue,
                    };
                    match type_ {
                        "text" => {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                blocks.push(serde_json::json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                        }
                        "image_url" => {
                            if let Some(image_url) = part.get("image_url") {
                                if let Some(url) = image_url.get("url").and_then(|u| u.as_str()) {
                                    if url.starts_with("http://") || url.starts_with("https://") {
                                        // HTTP URL 图片
                                        blocks.push(serde_json::json!({
                                            "type": "image",
                                            "source": {
                                                "type": "url",
                                                "url": url
                                            }
                                        }));
                                    } else if url.starts_with("data:") {
                                        // Base64 编码图片: data:<mime>;base64,<data>
                                        // 正确提取 MIME 类型：去掉 "data:" 前缀和 ";base64" 后缀
                                        if let Some((meta, data)) = url.split_once(',') {
                                            let media_type = meta
                                                .trim_start_matches("data:")
                                                .split(';')
                                                .next()
                                                .unwrap_or("image/jpeg");
                                            blocks.push(serde_json::json!({
                                                "type": "image",
                                                "source": {
                                                    "type": "base64",
                                                    "media_type": media_type,
                                                    "data": data
                                                }
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Value::Array(blocks)
            }
            _ => Value::String(String::new()),
        }
    }

    fn convert_openai_assistant_content(content: Option<&Value>) -> Vec<Value> {
        match content {
            Some(Value::String(text)) if !text.is_empty() => {
                vec![serde_json::json!({ "type": "text", "text": text })]
            }
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| {
                    let type_ = part.get("type").and_then(|t| t.as_str())?;
                    if type_ == "text" {
                        part.get("text").and_then(|t| t.as_str()).map(|text| {
                            serde_json::json!({
                                "type": "text",
                                "text": text
                            })
                        })
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn parse_tool_arguments(raw: &str) -> Result<Value, crate::types::GatewayError> {
        match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(obj)) => Ok(Value::Object(obj)),
            Ok(_) => Err(crate::types::GatewayError::InvalidRequest(
                "Invalid tool call arguments JSON: expected object".to_string(),
            )),
            Err(e) => {
                let preview: String = raw.chars().take(256).collect();
                tracing::warn!(
                    error = %e,
                    arguments = %preview,
                    "Invalid tool call arguments JSON"
                );
                Err(crate::types::GatewayError::InvalidRequest(
                    "Invalid tool call arguments JSON".to_string(),
                ))
            }
        }
    }

    fn is_empty_content(content: &Value) -> bool {
        match content {
            Value::String(s) => s.is_empty(),
            Value::Array(arr) => arr.is_empty(),
            _ => true,
        }
    }
}
