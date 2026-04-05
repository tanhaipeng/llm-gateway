# LLM Gateway

一个轻量级的 LLM API 网关，支持将请求转发到多个 LLM providers（OpenAI、Anthropic 等）。

## 特性

- 🚀 轻量级，基于 Rust 构建
- 🔌 支持多个 LLM providers
- 🎛️ 支持自定义 OpenAI 兼容的 providers
- ⚡ 高性能异步转发
- 🛡️ CORS 支持
- 📊 结构化日志和性能监控
- 🔒 可选的 API 认证
- 🔄 完整的流式处理支持
- 🛠️ 工具调用支持
- ⚙️ 灵活的配置方式（环境变量或 YAML 配置文件）

## 支持的 Providers

### 内置 Providers
- **OpenAI** (GPT-4, GPT-3.5, etc.)
- **Anthropic** (Claude)

### 自定义 Providers (OpenAI 兼容)
任何实现 OpenAI 兼容 API 的 provider 都可以添加，例如：
- Mistral
- Groq
- DeepSeek
- xAI
- 你自己的模型服务

## 快速开始

### 方式一：使用环境变量（简单）

1. **配置环境变量**

```bash
cd /Users/simontan/dev/llm-gateway
cp .env.template .env
# 编辑 .env 文件，填入你的 API keys
```

`.env` 文件示例：
```env
OPENAI_API_KEY=sk-proj-...
ANTHROPIC_API_KEY=sk-ant-...
MISTRAL_API_KEY=your_mistral_key
GROQ_API_KEY=your_groq_key
```

2. **运行服务**

```bash
cargo run
```

服务将在 `http://localhost:8080` 启动。

### 方式二：使用配置文件（推荐）

1. **复制配置文件**

```bash
cp config.yaml.example config.yaml
```

2. **编辑配置文件**

```yaml
server:
  address: "0.0.0.0"
  port: 8080

providers:
  openai:
    models:
      - "gpt-4o-mini"
    base-url: "https://api.openai.com"
  
  anthropic:
    models:
      - "claude-3-5-sonnet"
    base-url: "https://api.anthropic.com"
    version: "2023-06-01"
  
  mistral:
    models:
      - "mistral-large"
    base-url: "https://api.mistral.ai"
```

3. **配置环境变量**

```bash
cp .env.template .env
# 编辑 .env，填入对应的 API keys
```

4. **运行服务**

```bash
cargo run
```

## 使用 API

### 健康检查

```bash
curl http://localhost:8080/health
```

### OpenAI 请求

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'
```

### Anthropic 请求

```bash
curl -X POST http://localhost:8080/anthropic/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'
```

### 自定义 Provider 请求

```bash
# Mistral
curl -X POST http://localhost:8080/mistral/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'

# Groq
curl -X POST http://localhost:8080/groq/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama-3.3-70b-versatile",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'
```

## 添加自定义 Provider

### 步骤 1：在配置文件中添加 provider

编辑 `config.yaml`：

```yaml
providers:
  # ... 其他 providers
  
  my-custom-provider:
    models:
      - "my-model-1"
      - "my-model-2"
    base-url: "https://api.my-provider.com"
```

### 步骤 2：配置 API key

在 `.env` 文件中添加：

```env
MY_CUSTOM_PROVIDER_API_KEY=your_api_key_here
```

**注意**：Provider 名称会被转换为大写并替换 `-` 为 `_` 来匹配环境变量名。

### 步骤 3：使用自定义 provider

```bash
curl -X POST http://localhost:8080/my-custom-provider/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-model-1",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'
```

## API 端点

| 端点 | 方法 | 描述 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/metrics` | GET | 性能监控数据 |
| `/{provider}/v1/chat/completions` | POST | 聊天补全请求 |

其中 `{provider}` 可以是：
- `openai` - OpenAI
- `anthropic` - Anthropic
- `mistral` - Mistral AI
- `groq` - Groq
- `deepseek` - DeepSeek
- 或任何你配置的自定义 provider 名称

## 配置说明

### 配置文件 (config.yaml)

```yaml
server:
  address: "0.0.0.0"  # 服务器地址
  port: 8080          # 服务器端口

providers:
  provider-name:
    models:                    # 支持的模型列表
      - "model-1"
      - "model-2"
    base-url: "https://api.example.com"  # API 基础 URL
    version: "2023-06-01"     # API 版本（可选，主要用于 Anthropic）
```

### 环境变量

| 变量 | 描述 | 必需 |
|------|------|------|
| `GATEWAY_API_KEY` | 网关 API 认证 key | 否 |
| `OPENAI_API_KEY` | OpenAI API key | 否 |
| `ANTHROPIC_API_KEY` | Anthropic API key | 否 |
| `{PROVIDER}_API_KEY` | 自定义 provider API key | 否 |

## 自定义 Provider 要求

自定义 provider 必须满足以下要求：

1. **OpenAI 兼容 API**：实现 OpenAI 兼容的 API 端点
2. **标准认证**：使用 Bearer token 认证
3. **标准端点**：支持 `/v1/chat/completions` 端点
4. **标准格式**：请求和响应格式与 OpenAI 兼容

## 架构

```
Request → Axum Router → Dispatcher → Provider Client → LLM Provider → Response
                                      ↓
                                 OpenAI Compatible
                                 (Custom Providers)
```

## 高级功能

### 认证

网关支持 API 认证功能，可保护所有请求端点：

1. 在 `.env` 文件中设置 `GATEWAY_API_KEY`
2. 在请求中添加 `Authorization: Bearer YOUR_GATEWAY_API_KEY` header
3. `/health` 和 `/metrics` 端点不需要认证

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_GATEWAY_API_KEY" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 流式处理

支持流式响应处理，包括：

- 完整的流式事件处理（10+ 种事件类型）
- Stop Reason 映射（max_tokens→length, tool_use→tool_calls 等）
- 工具调用流式支持
- 实时数据转换和转发

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### 性能监控

内置性能监控系统，提供详细的运行指标：

```bash
curl http://localhost:8080/metrics
```

监控数据包括：
- 总请求数和成功率
- 平均延迟和 P50/P95/P99 百分位
- Token 使用统计
- 按提供商分组的性能数据
- 错误率统计

### 工具调用

支持 LLM 工具调用功能（Function Calling）：

```bash
curl -X POST http://localhost:8080/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "What is the weather?"}],
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
    }]
  }'
```

### 日志系统

完整的请求日志记录，包括：

- 请求 ID 追踪
- 提供商、模型、状态码记录
- 请求持续时间
- Token 使用统计
- 详细的错误日志

## 开发

### 运行测试

```bash
cargo test
```

### 构建 Release 版本

```bash
cargo build --release
```

### 运行测试脚本

```bash
./examples/test_gateway.sh
```

## 注意事项

- 仅保留了基础的模型转发功能
- 不包含缓存、限流、监控等高级功能
- 适合简单的代理和转发场景
- 自定义 providers 必须是 OpenAI 兼容的 API
- 如需完整功能，请参考原始的 [ai-gateway](https://github.com/Helicone/ai-gateway) 项目

## 常见问题

### Q: 如何添加多个自定义 providers？

A: 在 `config.yaml` 的 `providers` 部分添加多个配置，并在 `.env` 文件中为每个 provider 配置对应的 API key。

### Q: API key 可以在配置文件中直接设置吗？

A: 可以，但推荐使用环境变量。配置文件中的 `api_key` 字段会被环境变量覆盖。

### Q: 如何调试配置问题？

A: 查看启动日志，会显示加载的 providers 数量。如果为 0，检查配置文件路径和环境变量是否正确。

## 许可证

本项目基于 ai-gateway 项目简化而来，保留了基础转发功能。
