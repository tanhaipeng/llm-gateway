#!/bin/bash

# LLM Gateway 测试脚本

BASE_URL="http://localhost:8080"

echo "========================================="
echo "LLM Gateway 功能测试"
echo "========================================="
echo ""

# 1. 健康检查
echo "1. 健康检查"
echo "GET $BASE_URL/health"
curl -s "$BASE_URL/health"
echo ""
echo ""

# 2. 性能监控测试
echo "2. 性能监控测试"
echo "GET $BASE_URL/metrics"
curl -s "$BASE_URL/metrics" | jq '{
  total_requests: .total_requests,
  success_rate: .success_rate,
  error_rate: .error_rate,
  avg_latency_ms: .avg_latency_ms
}'
echo ""
echo ""

# 3. OpenAI 非流式请求
echo "3. OpenAI 非流式请求"
echo "POST $BASE_URL/openai/v1/chat/completions"
curl -X POST "$BASE_URL/openai/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "user", "content": "Hello! Please respond in one sentence."}
    ],
    "max_tokens": 50
  }' 2>/dev/null | jq -r '.choices[0].message.content'
echo ""
echo ""

# 4. OpenAI 流式请求
echo "4. OpenAI 流式请求"
echo "POST $BASE_URL/openai/v1/chat/completions (stream=true)"
curl -X POST "$BASE_URL/openai/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "user", "content": "Count to 3"}
    ],
    "stream": true,
    "max_tokens": 50
  }' 2>/dev/null | head -n 5
echo ""
echo ""

# 5. Anthropic 请求
echo "5. Anthropic 请求"
echo "POST $BASE_URL/anthropic/v1/chat/completions"
curl -X POST "$BASE_URL/anthropic/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet",
    "messages": [
      {"role": "user", "content": "Hello! Please respond in one sentence."}
    ],
    "max_tokens": 50
  }' 2>/dev/null | jq -r '.content[0].text'
echo ""
echo ""

# 6. 请求大小限制测试（应被拒绝）
echo "6. 请求大小限制测试"
echo "POST $BASE_URL/openai/v1/chat/completions (超大请求)"
LARGE_DATA=$(python3 -c "print('x'*11000000)")
curl -X POST "$BASE_URL/openai/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"$LARGE_DATA\"}]}" \
  -w "\nHTTP Status: %{http_code}\n" 2>/dev/null
echo ""
echo ""

# 7. 工具调用测试
echo "7. 工具调用测试"
echo "POST $BASE_URL/openai/v1/chat/completions (with tools)"
curl -X POST "$BASE_URL/openai/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "What is the weather in Tokyo?"}],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get the current weather in a location",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {
              "type": "string",
              "description": "The city and state"
            }
          },
          "required": ["location"]
        }
      }
    }]
  }' 2>/dev/null | jq -r '.choices[0].message.tool_calls[0].function.name // "No tool call"'
echo ""
echo ""

# 8. 流式工具调用测试
echo "8. 流式工具调用测试"
echo "POST $BASE_URL/openai/v1/chat/completions (stream + tools)"
curl -X POST "$BASE_URL/openai/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Get weather for Paris"}],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {"type": "string"}
          }
        }
      }
    }],
    "stream": true
  }' 2>/dev/null | head -n 3
echo ""
echo ""

# 9. 错误处理测试（无效的 provider）
echo "9. 错误处理测试（无效的 provider）"
echo "POST $BASE_URL/invalid-provider/v1/chat/completions"
curl -X POST "$BASE_URL/invalid-provider/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "test-model",
    "messages": [{"role": "user", "content": "Hello"}]
  }' \
  -w "\nHTTP Status: %{http_code}\n" 2>/dev/null
echo ""
echo ""

echo "========================================="
echo "基础测试完成"
echo "========================================="
echo ""
echo "提示："
echo "- 如需测试认证功能，请设置 GATEWAY_API_KEY 环境变量"
echo "- 查看 metrics 端点获取详细的性能数据"
echo "- 查看日志了解请求处理详情"
