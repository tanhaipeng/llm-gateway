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
        let result = convert(serde_json::json!({
            "model": "claude-haiku-4-5",
            "messages": [{
                "role": "user",
                "content": [{"type": "unsupported_part", "value": "x"}]
            }]
        }));
        let msgs = result["messages"].as_array().unwrap();
        assert!(msgs.is_empty());
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
}

/// 请求映射器 - 将 OpenAI 格式转换为其他 providers 格式
pub struct RequestMapper;

impl RequestMapper {
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
                    let mut content_blocks: Vec<Value> = Vec::new();

                    // 文本内容（非空才添加）
                    if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            content_blocks.push(serde_json::json!({
                                "type": "text",
                                "text": text
                            }));
                        }
                    }

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
                            let input = func
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())
                                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                                .unwrap_or_else(|| serde_json::json!({}));
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

    fn is_empty_content(content: &Value) -> bool {
        match content {
            Value::String(s) => s.is_empty(),
            Value::Array(arr) => arr.is_empty(),
            _ => true,
        }
    }
}
