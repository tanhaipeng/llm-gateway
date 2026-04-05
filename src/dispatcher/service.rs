use crate::types::{Config, Provider, ProviderConfig, SSEStream};
use crate::mapper::{RequestMapper, ResponseMapper};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct Dispatcher {
    providers: Arc<HashMap<String, ProviderClient>>,
}

#[derive(Clone)]
pub struct ProviderClient {
    client: reqwest::Client,
    config: Arc<ProviderConfig>,
    provider: Provider,
}

impl ProviderClient {
    pub fn new(provider: Provider, config: ProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config: Arc::new(config),
            provider,
        }
    }

    /// 构建请求URL
    fn build_url(&self) -> String {
        if self.provider == Provider::Anthropic {
            format!("{}v1/messages", self.config.base_url)
        } else {
            format!("{}v1/chat/completions", self.config.base_url)
        }
    }

    /// 添加provider特定的headers
    fn add_provider_headers(&self, mut request_builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.provider {
            Provider::OpenAI | Provider::GoogleGemini | Provider::Deepseek | Provider::Custom(_) => {
                if let Some(api_key) = &self.config.api_key {
                    request_builder = request_builder.header(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {}", api_key),
                    );
                }
            }
            Provider::Anthropic => {
                if let Some(api_key) = &self.config.api_key {
                    request_builder = request_builder.header("x-api-key", api_key);
                }
                if let Some(version) = &self.config.version {
                    request_builder = request_builder.header("anthropic-version", version);
                }
            }
        }
        request_builder
    }

    /// 非流式请求转发
    pub async fn forward_request(
        &self,
        body: bytes::Bytes,
    ) -> Result<axum::response::Response, crate::types::GatewayError> {
        let converted_body = RequestMapper::convert_request(&body, &self.provider)?;
        let url = self.build_url();
        
        let mut request_builder = self.add_provider_headers(self.client.post(&url));
        
        request_builder = request_builder
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(converted_body);

        let response = request_builder.send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::types::GatewayError::ProviderError(
                format!("Provider returned error {}: {}", status, error_text),
            ));
        }

        let status = response.status();
        let headers = response.headers().clone();
        let body_bytes = response.bytes().await?;
        
        let response_data = String::from_utf8_lossy(&body_bytes);
        let converted_response = ResponseMapper::convert_response(&response_data, &self.provider, false)?;
        
        let mut axum_response = axum::response::Response::builder()
            .status(status);
        
        for (name, value) in headers.iter() {
            axum_response = axum_response.header(name, value);
        }
        
        let response = axum_response.body(axum::body::Body::from(converted_response))?;
        Ok(response)
    }

    /// 流式请求转发
    pub async fn forward_request_stream(
        &self,
        body: bytes::Bytes,
    ) -> Result<axum::response::Response, crate::types::GatewayError> {
        let converted_body = RequestMapper::convert_request(&body, &self.provider)?;
        let url = self.build_url();
        let provider_clone = self.provider.clone();
        
        let mut request_builder = self.add_provider_headers(self.client.post(&url));

        request_builder = request_builder
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(converted_body);

        let response = request_builder.send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::types::GatewayError::ProviderError(
                format!("Provider returned error {}: {}", status, error_text),
            ));
        }

        let byte_stream = response.bytes_stream().map(|result| {
            result.map_err(|e| crate::types::GatewayError::StreamError(
                crate::types::stream::StreamError::BodyError(e.to_string())
            ))
        });
        
        let converted_stream = Box::pin(byte_stream.map(move |result| {
            result.and_then(|bytes| {
                let data = String::from_utf8_lossy(&bytes);
                let converted = ResponseMapper::convert_response(&data, &provider_clone, true)?;
                Ok(bytes::Bytes::from(format!("data: {}\n\n", converted)))
            })
        })) as SSEStream;
        
        let axum_response = axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/event-stream; charset=utf-8")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("Transfer-Encoding", "chunked");
        
        let body = axum::body::Body::from_stream(converted_stream);
        Ok(axum_response.body(body)?)
    }
}

impl Dispatcher {
    pub fn new(config: &Config) -> Self {
        let mut providers = HashMap::new();
        
        for (name, provider_config) in &config.providers {
            let provider = Provider::from_str(name)
                .unwrap_or_else(|_| Provider::Custom(name.clone()));
            
            let client = ProviderClient::new(provider, provider_config.clone());
            providers.insert(name.clone(), client);
        }
        
        Self {
            providers: Arc::new(providers),
        }
    }

    pub fn get_provider(&self, name: &str) -> Option<&ProviderClient> {
        self.providers.get(name)
    }
}
