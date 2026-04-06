use crate::types::Config;
use std::env;

pub fn load_config() -> Result<Config, crate::types::GatewayError> {
    // 从环境变量获取配置文件路径，默认为 config.yaml
    let config_path = env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yaml".to_string());

    tracing::info!("Attempting to load config from: {}", config_path);

    // 尝试从配置文件加载
    let config = match load_from_file(&config_path) {
        Ok(config) => {
            tracing::info!("Successfully loaded config from file: {}", config_path);
            config
        }
        Err(crate::types::GatewayError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                "Config file '{}' not found. Falling back to env variables.",
                config_path
            );
            load_from_env()
        }
        Err(e) => {
            tracing::error!(
                "Failed to load config from file '{}': {}. Refusing to start.",
                config_path,
                e
            );
            return Err(e);
        }
    };

    // 从环境变量加载或覆盖 API keys
    let config = load_api_keys_from_env(config);

    if config.providers.is_empty() {
        return Err(crate::types::GatewayError::InvalidConfig(
            "No providers configured. Define at least one provider in config file or set provider API keys in environment variables.".to_string(),
        ));
    }

    Ok(config)
}

fn load_from_file(path: &str) -> Result<Config, crate::types::GatewayError> {
    // 尝试读取文件
    let content =
        std::fs::read_to_string(path).map_err(|e| crate::types::GatewayError::IoError(e))?;

    // 解析 YAML
    let config: Config =
        serde_yml::from_str(&content).map_err(|e| crate::types::GatewayError::YamlError(e))?;

    Ok(config)
}

fn load_from_env() -> Config {
    let openai_key = env::var("OPENAI_API_KEY").ok();
    let anthropic_key = env::var("ANTHROPIC_API_KEY").ok();

    let mut config = Config {
        server: crate::types::ServerConfig::default(),
        providers: std::collections::HashMap::new(),
    };

    // Add OpenAI provider if key is set
    if let Some(key) = openai_key {
        config.providers.insert(
            "openai".to_string(),
            crate::types::ProviderConfig {
                models: vec![
                    "gpt-4o".to_string(),
                    "gpt-4o-mini".to_string(),
                    "gpt-4-turbo".to_string(),
                ],
                base_url: "https://api.openai.com".to_string(),
                api_key: Some(key),
                version: None,
            },
        );
    }

    // Add Anthropic provider if key is set
    if let Some(key) = anthropic_key {
        config.providers.insert(
            "anthropic".to_string(),
            crate::types::ProviderConfig {
                models: vec![
                    "claude-3-5-sonnet".to_string(),
                    "claude-3-5-haiku".to_string(),
                    "claude-3-opus".to_string(),
                ],
                base_url: "https://api.anthropic.com".to_string(),
                api_key: Some(key),
                version: Some("2023-06-01".to_string()),
            },
        );
    }

    config
}

fn load_api_keys_from_env(mut config: Config) -> Config {
    // 为所有 providers 从环境变量加载 API keys
    for (name, provider_config) in config.providers.iter_mut() {
        // 转换 provider name 为环境变量格式 (e.g., "mistral" -> "MISTRAL_API_KEY")
        let env_key = format!("{}_API_KEY", name.to_uppercase().replace('-', "_"));

        if let Ok(api_key) = env::var(&env_key) {
            provider_config.api_key = Some(api_key);
        }
    }

    config
}
