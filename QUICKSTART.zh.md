# LLM Gateway 快速开始

## 1. 基础设置

```bash
cd /Users/simontan/dev/llm-gateway
cp .env.template .env
cp config.yaml.example config.yaml
```

在 `.env` 中填写：

```env
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
GATEWAY_API_KEY=...   # 可选
```

启用网关认证后，请求需带：`Authorization: Bearer <GATEWAY_API_KEY>`。

## 2. 配置 provider

```yaml
providers:
  openai:
    models: ["gpt-4o-mini"]
    base-url: "https://api.openai.com"

  anthropic:
    models: ["claude-3-5-sonnet"]
    base-url: "https://api.anthropic.com"
    version: "2023-06-01"

  my-provider:
    models: ["gpt-4.1-mini"]
    base-url: "https://api.example.com"
    protocol: "responses"
```

客户端始终调用 `chat/completions`，网关自动完成协议适配。

## 3. 建议的服务保护配置

```yaml
server:
  request-timeout-seconds: 930
  cors:
    allow-any-origin: false
    allow-origins: []  # backend-only 默认不允许跨域；前端场景请填具体 origin
  limits:
    max-in-flight-requests: 512
    max-requests-per-second: 200
  metrics:
    require-auth: true  # 建议生产开启，保护 /metrics
```

`/health` 永远免认证，`/metrics` 由 `require-auth` 控制。

## 4. 启动

```bash
cargo run
```

地址：`http://localhost:8080`

## 5. 验证

### 健康检查

```bash
curl http://localhost:8080/health
```

### 指标

```bash
curl http://localhost:8080/metrics | jq
```

### OpenAI 示例

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role":"user","content":"Hello!"}]
  }'
```

### Anthropic 示例

```bash
curl -X POST http://localhost:8080/anthropic/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet",
    "messages": [{"role":"user","content":"Hello!"}]
  }'
```

### 流式示例

```bash
curl -N -X POST http://localhost:8080/my-provider/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4.1-mini",
    "stream": true,
    "messages": [{"role":"user","content":"Count to 5"}]
  }'
```

当 `protocol: "responses"` 时，网关会输出标准 `chat.completion.chunk` SSE。

## 6. Python SDK 示例

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/openai",
    api_key="dummy"
)

resp = client.chat.completions.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello"}]
)
print(resp.choices[0].message.content)
```

## 7. 故障排除

- `Loaded configuration with 0 providers`：检查 `.env` 是否至少配置一个有效 API key
- `Provider not found`：确认 `config.yaml` 已定义该 provider 且 key 已设置
- `401 Unauthorized`：检查 `GATEWAY_API_KEY` 与 `Authorization` header 是否匹配
