# LLM Gateway 快速开始指南

## 1. 基础设置

```bash
cd /Users/simontan/dev/llm-gateway

# 复制环境变量模板
cp .env.template .env

# 编辑 .env 文件，添加你的 API keys
nano .env
```

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
3. 重启服务

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
