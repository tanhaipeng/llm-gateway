# LLM Gateway Quick Start

## 1. Setup

```bash
cd /Users/simontan/dev/llm-gateway
cp .env.template .env
cp config.yaml.example config.yaml
```

Fill `.env`:

```env
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
GATEWAY_API_KEY=...   # optional
```

If gateway auth is enabled, send: `Authorization: Bearer <GATEWAY_API_KEY>`.

## 2. Configure providers

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

Clients always call `chat/completions`; protocol adaptation is handled by the gateway.

## 3. Recommended server protection config

```yaml
server:
  request-timeout-seconds: 930
  cors:
    allow-any-origin: false
    allow-origins: []  # backend-only default; set explicit origins for browser use
  limits:
    max-in-flight-requests: 512
    max-requests-per-second: 200
  metrics:
    require-auth: true  # recommended in production to protect /metrics
  resilience:
    provider-max-concurrency: 128
    retry-max-attempts: 3
    circuit-breaker-failure-threshold: 8
```

`/health` is always public; `/metrics` auth depends on `require-auth`.

## 4. Run

```bash
cargo run
```

Address: `http://localhost:8080`

## 5. Verify

### Health check

```bash
curl http://localhost:8080/health
```

### Metrics

```bash
curl http://localhost:8080/metrics | jq
```

### OpenAI example

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role":"user","content":"Hello!"}]
  }'
```

### Anthropic example

```bash
curl -X POST http://localhost:8080/anthropic/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet",
    "messages": [{"role":"user","content":"Hello!"}]
  }'
```

### Streaming example

```bash
curl -N -X POST http://localhost:8080/my-provider/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4.1-mini",
    "stream": true,
    "messages": [{"role":"user","content":"Count to 5"}]
  }'
```

With `protocol: "responses"`, the gateway emits standard `chat.completion.chunk` SSE.

## 6. Python SDK example

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

## 7. Troubleshooting

- `Loaded configuration with 0 providers`: ensure at least one valid API key in `.env`
- `Provider not found`: ensure provider exists in `config.yaml` and key is set
- `401 Unauthorized`: verify `GATEWAY_API_KEY` and `Authorization` header match
