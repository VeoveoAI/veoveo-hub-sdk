//! A chat model behind an OpenAI-compatible chat completions endpoint, such as Hugging
//! Face's Inference Providers router. Tools go to the model with their MCP input schemas
//! unchanged, so a provider that cannot take a schema refuses the request.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// The longest one completion may take, reasoning included.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(300);
/// Attempts per completion when the endpoint answers 502, 503 or 504 or times out.
const ATTEMPTS: u32 = 3;

pub struct ChatModel {
    /// The API's base URL, such as `https://router.huggingface.co/v1`.
    pub base: Url,
    /// The model, with any provider suffix the endpoint expects.
    pub model: String,
    pub token: String,
    /// Output tokens per completion, reasoning included.
    pub max_tokens: u32,
    pub http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::text("system", content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::text("user", content)
    }

    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_call_id: Some(call_id.into()),
            ..Self::text("tool", content)
        }
    }

    fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_owned(),
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// The arguments as the model wrote them, a JSON object in a string.
    pub arguments: String,
}

/// One completion: the assistant's message, why it stopped, and the endpoint's usage.
#[derive(Debug, Clone)]
pub struct Completion {
    pub message: Message,
    pub finish_reason: Option<String>,
    pub usage: Value,
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Value,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
    finish_reason: Option<String>,
}

/// An MCP tool as a function the model may call, with its input schema as it is.
pub fn function(tool: &Tool) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description.as_deref().or(tool.title.as_deref()).unwrap_or(""),
            "parameters": Value::Object((*tool.input_schema).clone()),
        }
    })
}

impl ChatModel {
    pub async fn complete(&self, messages: &[Message], functions: &[Value]) -> Result<Completion> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.max_tokens,
        });
        if !functions.is_empty() {
            body["tools"] = Value::Array(functions.to_vec());
            body["tool_choice"] = Value::String("auto".to_owned());
        }
        let endpoint = self.base.join("chat/completions")?;
        let mut attempt = 1;
        let response = loop {
            let sent = self
                .http
                .post(endpoint.clone())
                .bearer_auth(&self.token)
                .json(&body)
                .timeout(COMPLETION_TIMEOUT)
                .send()
                .await;
            let transient = match &sent {
                Ok(response) => matches!(response.status().as_u16(), 502..=504),
                Err(error) => error.is_timeout() || error.is_connect(),
            };
            if !transient || attempt == ATTEMPTS {
                break sent.context("reach the model endpoint")?;
            }
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
            attempt += 1;
        };
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            bail!("the model endpoint answered {status} after {attempt} attempts: {detail}");
        }
        let mut answer: CompletionResponse = response.json().await?;
        if answer.choices.is_empty() {
            bail!("the model endpoint returned no choice");
        }
        let choice = answer.choices.swap_remove(0);
        Ok(Completion {
            message: choice.message,
            finish_reason: choice.finish_reason,
            usage: answer.usage,
        })
    }
}
