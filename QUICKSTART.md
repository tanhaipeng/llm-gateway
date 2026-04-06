# LLM Gateway 快速开始指南

## 1. 基础设置

```bash
cd /Users/simontan/dev/llm-gateway

# 复制环境变量模板
cp .env.template .env

# 编辑 .env 文件，添加你的 API keys
nano .env
```

**可选配置**：如果需要启用认证，在 `.env` 文件中设置：
```env
GATEWAY_API_KEY=your_secret_key_here
```

启用认证后，所有请求都需要在 header 中提供：
```
Authorization: Bearer your_secret_key_here
```
注意：`/health` 和 `/metrics` 端点不需要认证。

## 2. 配置 Providers

### 选项 A：仅使用内置 providers（OpenAI + Anthropic）

编辑 `.env` 文件：
```env
OPENAI_API_KEY=sk-proj-your-key-here
ANTHROPIC_API_KEY=sk-ant-your-key-here
```

### 选项 B：使用配置文件添加自定义 providers

1. 编辑 `config.yaml`，取消注释你想使用的 providers
2. 编辑 `.env` 文件，添加对应的 API keys

例如，要使用 Mistral：
```env
# .env 文件
MISTRAL_API_KEY=your-mistral-key-here
```

```yaml
# config.yaml 文件
providers:
  mistral:
    models:
      - "mistral-large"
    base-url: "https://api.mistral.ai"
```

如果某个下游 provider 只支持 OpenAI `responses` 协议，可加：

```yaml
providers:
  my-provider:
    models:
      - "gpt-4.1-mini"
    base-url: "https://api.my-provider.com"
    protocol: "responses"
```

说明：客户端仍然调用 `chat/completions`，网关会自动完成协议适配。

## 3. 启动服务

```bash
cargo run
```

服务将在 `http://localhost:8080` 启动。

## 4. 测试

### 健康检查
```bash
curl http://localhost:8080/health
```

### 性能监控
```bash
curl http://localhost:8080/metrics | jq
```

### 流式请求测试
```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Count to 5"}],
    "stream": true
  }'
```

注意：当 provider 配置了 `protocol: "responses"` 时，网关会自动把上游 `responses` 流事件转换为 `chat.completion.chunk` 流。

### 测试 OpenAI
```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 50
  }'
```

### 测试 Anthropic
```bash
curl -X POST http://localhost:8080/anthropic/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-haiku",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 50
  }'
```

### 测试自定义 Provider（如 Mistral）
```bash
curl -X POST http://localhost:8080/mistral/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 50
  }'
```

## 5. 使用 Python SDK

```python
from openai import OpenAI

# 连接到 gateway
client = OpenAI(
    base_url="http://localhost:8080/openai",
    api_key="dummy-key"  # gateway 处理认证
)

# 使用 OpenAI
response = client.chat.completions.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)

# 使用 Anthropic
client_anthropic = OpenAI(
    base_url="http://localhost:8080/anthropic",
    api_key="dummy-key"
)

response = client_anthropic.chat.completions.create(
    model="claude-3-5-haiku",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)

# 使用自定义 Provider（如 Mistral）
client_mistral = OpenAI(
    base_url="http://localhost:8080/mistral",
    api_key="dummy-key"
)

response = client_mistral.chat.completions.create(
    model="mistral-large",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)
```

## 6. 故障排除

### 问题：启动时显示 "Loaded configuration with 0 providers"

**原因**：没有配置任何 API keys

**解决**：
1. 检查 `.env` 文件是否存在
2. 确认 `.env` 文件中至少有一个 API key
3. 确认环境变量名称正确（大写，如 `OPENAI_API_KEY`）

### 问题：Provider not found

**原因**：使用了未配置的 provider

**解决**：
1. 检查 `config.yaml` 中是否定义了该 provider
2. 确认 `.env` 文件中有对应的 API key
3. 重启服务

### 问题：401 Unauthorized

**原因**：API key 无效或未配置

**解决**：
1. 检查 `.env` 文件中的 API key 是否正确
2. 确认环境变量名称格式正确（`{PROVIDER}_API_KEY`）
3. 如果启用了网关认证，确保请求包含正确的 `Authorization` header
4. 重启服务

### 问题：401 Unauthorized（网关认证）

**原因**：未提供或提供了错误的网关认证 key

**解决**：
1. 检查是否设置了 `GATEWAY_API_KEY` 环境变量
2. 确认请求中包含正确的 `Authorization: Bearer YOUR_KEY` header
3. 注意 `/health` 和 `/metrics` 端点不需要认证

## 7. 添加更多 Providers

只需两步：

1. 在 `config.yaml` 中添加 provider 配置
2. 在 `.env` 中添加对应的 API key

示例：
```yaml
# config.yaml
providers:
  my-provider:
    models:
      - "my-model"
    base-url: "https://api.my-provider.com"
```

```env
# .env
MY_PROVIDER_API_KEY=your-key-here
```

然后就可以使用了：
```bash
curl -X POST http://localhost:8080/my-provider/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-model",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 8. 生产部署建议

1. 使用 `cargo build --release` 构建优化版本
2. 使用环境变量管理敏感信息（不要将 API keys 提交到 git）
3. 配置适当的日志级别
4. 考虑使用反向代理（如 Nginx）处理 HTTPS
5. 设置适当的防火墙规则

更多详细信息请参考 [README.md](README.md)
