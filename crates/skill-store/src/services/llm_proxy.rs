use crate::config::LlmProvider;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct LlmProxy {
    http: reqwest::Client,
    providers: Vec<LlmProvider>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

pub struct LlmResponse {
    pub content: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl LlmProxy {
    pub fn new(providers: Vec<LlmProvider>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");
        Self { http, providers }
    }

    /// Send assembled prompt to the first available LLM provider
    pub async fn chat(&self, system_prompt: &str, user_input: &str) -> Result<LlmResponse> {
        let provider = self
            .providers
            .first()
            .ok_or_else(|| anyhow::anyhow!("No LLM provider configured"))?;

        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".into(),
                content: user_input.to_string(),
            },
        ];

        let req = ChatRequest {
            model: provider.model.clone(),
            messages,
            max_tokens: Some(4096),
            stream: Some(false),
        };

        let resp = self
            .http
            .post(&provider.api_url)
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {status}: {body}");
        }

        let data: ChatResponse = resp.json().await?;
        let content = data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let (input_tokens, output_tokens) = match data.usage {
            Some(u) => (u.prompt_tokens, u.completion_tokens),
            None => (0, 0),
        };

        Ok(LlmResponse {
            content,
            input_tokens,
            output_tokens,
        })
    }

    /// Send raw messages for defense detection (uses clewdr endpoint if available)
    pub async fn detect(
        &self,
        endpoint: &str,
        api_key: &str,
        prompt: &str,
    ) -> Result<DefenseResult> {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
        }];

        let req = ChatRequest {
            model: "claude-3-haiku-20240307".into(),
            messages,
            max_tokens: Some(256),
            stream: Some(false),
        };

        let resp = self
            .http
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            // If detection service is down, fail open (allow request)
            tracing::warn!("Defense detection service returned {}", resp.status());
            return Ok(DefenseResult {
                malicious: false,
                reason: "Detection service unavailable".into(),
            });
        }

        let data: ChatResponse = resp.json().await?;
        let content = data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Parse JSON from response
        let result: DefenseResult = serde_json::from_str(&content).unwrap_or(DefenseResult {
            malicious: false,
            reason: "Failed to parse detection result".into(),
        });

        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
pub struct DefenseResult {
    pub malicious: bool,
    pub reason: String,
}
