use crate::types::Config;
use std::env;

pub fn load_config() -> Result<Config, crate::types::GatewayError> {
    // 尝试从配置文件加载
    let config = if let Ok(config) = load_from_file("config.yaml") {
        config
    } else {
        // 回退到环境变量配置
        load_from_env()
    };

    // 从环境变量加载或覆盖 API keys
    let config = load_api_keys_from_env(config);

    Ok(config)
}

fn load_from_file(path: &str) -> Result<Config, crate::types::GatewayError> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&content)?;
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
