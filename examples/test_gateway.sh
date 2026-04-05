#!/bin/bash

# LLM Gateway 测试脚本

BASE_URL="http://localhost:8080"

echo "========================================="
echo "LLM Gateway 测试"
echo "========================================="
echo ""

# 健康检查
echo "1. 健康检查"
echo "GET $BASE_URL/health"
curl -s "$BASE_URL/health"
echo ""
echo ""

# OpenAI 请求示例
echo "2. OpenAI 请求示例"
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

# Anthropic 请求示例
echo "3. Anthropic 请求示例"
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

echo "========================================="
echo "测试完成"
echo "========================================="
