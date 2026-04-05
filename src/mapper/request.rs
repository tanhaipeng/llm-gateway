use bytes::Bytes;
use serde_json::Value;

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
            crate::types::Provider::OpenAI | crate::types::Provider::GoogleGemini | crate::types::Provider::Deepseek | crate::types::Provider::Custom(_) => Ok(body.clone()), // OpenAI 兼容的 providers 直接转发
        }
    }
    
    /// OpenAI → Anthropic 请求格式转换
    fn openai_to_anthropic(openai_json: Value) -> Result<Bytes, crate::types::GatewayError> {
        let mut anthropic_json = serde_json::json!({
            "model": openai_json.get("model").unwrap_or(&Value::String("claude-3-5-sonnet".to_string())),
            "max_tokens": openai_json.get("max_tokens").unwrap_or(&Value::Number(serde_json::Number::from(4096u32))),
            "messages": openai_json.get("messages"),
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
            // OpenAI 可以是字符串或数组，Anthropic 需要数组
            let stop_sequences: Vec<Value> = match stop {
                Value::String(s) => vec![Value::String(s.clone())],
                Value::Array(arr) => {
                    // 直接克隆 arr，它已经是 Vec<Value>
                    arr.clone()
                }
                _ => vec![],
            };
            anthropic_json["stop_sequences"] = Value::Array(stop_sequences);
        }
        
        // 提取 system prompt
        let empty_messages: Vec<Value> = vec![];
        let messages = openai_json.get("messages").and_then(|m| m.as_array()).unwrap_or(&empty_messages);
        let mut anthropic_messages = Vec::new();
        let mut system_prompt = None;
        
        for msg in messages {
            if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
                match role {
                    "system" => {
                        system_prompt = Some(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string());
                    }
                    "user" => {
                        let content = Self::convert_openai_user_content(msg.get("content"));
                        anthropic_messages.push(serde_json::json!({
                            "role": "user",
                            "content": content
                        }));
                    }
                    "assistant" => {
                        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        let mut anthropic_msg = serde_json::json!({
                            "role": "assistant",
                            "content": content
                        });
                        
                        // 处理 tool_calls
                        if let Some(tool_calls) = msg.get("tool_calls") {
                            anthropic_msg["tool_calls"] = tool_calls.clone();
                        }
                        
                        anthropic_messages.push(anthropic_msg);
                    }
                    _ => {}
                }
            }
        }
        
        anthropic_json["messages"] = Value::Array(anthropic_messages);
        
        // 如果有 system prompt，添加到消息开头
        if let Some(sys) = system_prompt {
            anthropic_json["system"] = Value::String(sys);
        }
        
        let result = serde_json::to_vec(&anthropic_json)?;
        Ok(Bytes::from(result))
    }
    
    /// 转换 OpenAI 用户消息内容
    fn convert_openai_user_content(content: Option<&Value>) -> Value {
        match content {
            Some(Value::String(text)) => Value::String(text.clone()),
            Some(Value::Array(parts)) => {
                let mut blocks = Vec::new();
                for part in parts {
                    if let Some(type_) = part.get("type").and_then(|t| t.as_str()) {
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
                                        let is_http = url.starts_with("http");
                                        let image = if is_http {
                                            serde_json::json!({
                                                "type": "image",
                                                "source": {
                                                    "type": "url",
                                                    "url": url
                                                }
                                            })
                                        } else {
                                            // Base64 编码的图片
                                            if let Some((mime, data)) = url.split_once(',') {
                                                serde_json::json!({
                                                    "type": "image",
                                                    "source": {
                                                        "type": "base64",
                                                        "media_type": mime,
                                                        "data": data
                                                    }
                                                })
                                            } else {
                                                continue;
                                            }
                                        };
                                        blocks.push(image);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                serde_json::json!(blocks)
            }
            _ => Value::String("".to_string()),
        }
    }
}
