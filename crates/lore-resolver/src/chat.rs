use crate::{ProjectionInput, ProjectionProposal, ProjectionProposer};
use lore_core::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatConfig {
    pub base_url: String,
    pub model: String,
    #[serde(alias = "api_key_env")]
    pub token_env: String,
    pub system_prompt: String,
    pub timeout_seconds: u64,
    pub max_input_bytes: usize,
    pub max_output_tokens: usize,
    /// Additional non-secret API fields; model, messages, output limit, and streaming remain owned by Lore.
    pub request_options: serde_json::Map<String, serde_json::Value>,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://endpoint.access.furiosa.dev/v1".into(),
            model: "furiosa-ai/K-EXAONE-236B-A23B-NVFP4A16".into(),
            token_env: "FURIOSA_ACCESS_API_KEY".into(),
            system_prompt: include_str!("projection.md").into(),
            timeout_seconds: 120,
            max_input_bytes: 256 * 1024,
            max_output_tokens: 4096,
            request_options: serde_json::Map::new(),
        }
    }
}

pub struct ChatProposer {
    config: ChatConfig,
    client: reqwest::blocking::Client,
    endpoint: String,
    token: Option<String>,
}

impl ChatProposer {
    pub fn new(config: ChatConfig) -> Result<Self> {
        Self::with_token(config, None)
    }

    /// The configured environment variable overrides the supplied credential at request time.
    pub fn with_token(config: ChatConfig, token: Option<String>) -> Result<Self> {
        if ["model", "messages", "max_tokens", "stream"]
            .iter()
            .any(|key| config.request_options.contains_key(*key))
        {
            return Err(Error::invalid(
                "request_options cannot override model, messages, max_tokens, or stream",
            ));
        }
        let base = reqwest::Url::parse(&config.base_url)
            .map_err(|_| Error::invalid("Invalid model API URL"))?;
        if !matches!(base.scheme(), "https" | "http")
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || config.model.trim().is_empty()
            || config.system_prompt.trim().is_empty()
            || config.token_env.is_empty()
            || !config
                .token_env
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || config.timeout_seconds == 0
            || config.max_input_bytes == 0
            || config.max_output_tokens == 0
        {
            return Err(Error::invalid("Invalid model API configuration"));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                Error::new(ErrorKind::Unavailable, "Cannot initialize model API client")
            })?;
        let endpoint = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        Ok(Self {
            config,
            client,
            endpoint,
            token,
        })
    }
}

impl ProjectionProposer for ChatProposer {
    fn propose(&self, input: &ProjectionInput) -> Result<ProjectionProposal> {
        let mut request = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": self.config.system_prompt},
                {"role": "user", "content": serde_json::to_string(input)?},
            ],
            "max_tokens": self.config.max_output_tokens,
        });
        request
            .as_object_mut()
            .unwrap()
            .extend(self.config.request_options.clone());
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > self.config.max_input_bytes {
            return Err(Error::invalid(
                "Projection request exceeds max_input_bytes; narrow --path or adjust resolver configuration",
            ));
        }
        let token = std::env::var(&self.config.token_env)
            .ok()
            .filter(|token| !token.trim().is_empty())
            .or_else(|| self.token.clone())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::PermissionDenied,
                    format!(
                        "Set {} or save a model connection token for model-assisted projection",
                        self.config.token_env
                    ),
                )
            })?;
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes)
            .send()
            .map_err(|_| Error::new(ErrorKind::Unavailable, "Model API request failed"))?;
        if !response.status().is_success() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                format!("Model API returned HTTP {}", response.status().as_u16()),
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                Error::new(
                    ErrorKind::Unavailable,
                    "Model API response could not be read",
                )
            })?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Model API response exceeds 2 MB",
            ));
        }
        let envelope: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new(ErrorKind::Corrupt, "Model API response is not JSON"))?;
        let choice = &envelope["choices"][0];
        if choice["finish_reason"] == "length" {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Model exceeded max_output_tokens before completing a projection answer; increase the output limit or adjust reasoning options",
            ));
        }
        if choice["finish_reason"] != "stop" {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Model did not complete a projection answer",
            ));
        }
        let content = choice["message"]["content"].as_str().ok_or_else(|| {
            Error::new(
                ErrorKind::Corrupt,
                "Model API response has no final content",
            )
        })?;
        serde_json::from_str(content).map_err(|_| {
            Error::new(
                ErrorKind::Corrupt,
                "Model answer does not match the projection protocol",
            )
        })
    }
}
