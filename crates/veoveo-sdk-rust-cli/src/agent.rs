//! The `ask` loop: a chat model works on a request with the Gateway's tools, as the
//! session's caller, until it answers or reaches the step limit. Every tool call goes
//! through the Gateway, which authorizes it as for any other client.

use std::time::Duration;

use anyhow::Result;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    mcp::Session,
    model::{ChatModel, Message, function},
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Model turns, each of which may call several tools.
    pub max_turns: usize,
    /// How long one tool call may take, a Task's wait included.
    pub tool_deadline: Duration,
}

/// What happened, in order, for a reader or a test.
#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    pub request: String,
    pub model: String,
    pub steps: Vec<Step>,
    pub answer: Option<String>,
    pub stop: Stop,
    pub usage: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub tool: String,
    pub arguments: Value,
    /// The result as the model received it.
    pub result: Value,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    /// The model answered without calling another tool.
    Answered,
    /// The model was still calling tools after the last allowed turn.
    TurnLimit,
    /// The model ran out of output tokens before it answered or called a tool.
    Length,
}

/// The system message: who the model acts for, through what, and the current time,
/// which requests such as "tomorrow" need.
fn instructions() -> String {
    format!(
        "You act for the user through the tools of a Veoveo installation, reached through its \
         MCP Gateway. Use the tools to carry out the request rather than describing how. When a \
         tool refuses or fails, read its error and adjust or report it. Finish with a short \
         answer saying what you did and what each tool returned that matters. The current time \
         is {} UTC.",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S")
    )
}

/// Work on `request` with every tool the session's caller sees. `observe` receives each
/// step as it completes.
pub async fn ask(
    session: &mut Session,
    model: &ChatModel,
    request: &str,
    limits: Limits,
    mut observe: impl FnMut(&Step),
) -> Result<Transcript> {
    let tools = session.tools().await?;
    let functions: Vec<Value> = tools.iter().map(function).collect();
    let mut messages = vec![Message::system(instructions()), Message::user(request)];
    let mut transcript = Transcript {
        request: request.to_owned(),
        model: model.model.clone(),
        steps: Vec::new(),
        answer: None,
        stop: Stop::TurnLimit,
        usage: Vec::new(),
    };
    for _ in 0..limits.max_turns {
        let completion = model.complete(&messages, &functions).await?;
        transcript.usage.push(completion.usage);
        let calls = completion.message.tool_calls.clone();
        let content = completion.message.content.clone();
        messages.push(completion.message);
        if calls.is_empty() {
            transcript.answer = content.filter(|answer| !answer.trim().is_empty());
            transcript.stop = if completion.finish_reason.as_deref() == Some("length") {
                Stop::Length
            } else {
                Stop::Answered
            };
            return Ok(transcript);
        }
        for call in calls {
            session.refresh().await?;
            let step = run(
                session,
                &call.function.name,
                &call.function.arguments,
                limits,
            )
            .await;
            observe(&step);
            messages.push(Message::tool(call.id, step.result.to_string()));
            transcript.steps.push(step);
        }
    }
    Ok(transcript)
}

/// Run one tool call. A refusal, a malformed argument string and a failed Task all come
/// back as an error result for the model to read.
async fn run(session: &Session, tool: &str, arguments: &str, limits: Limits) -> Step {
    let parsed = if arguments.trim().is_empty() {
        Ok(JsonObject::new())
    } else {
        serde_json::from_str::<JsonObject>(arguments)
    };
    let (arguments, outcome) = match parsed {
        Ok(object) => (
            Value::Object(object.clone()),
            session.call(tool, object, limits.tool_deadline).await,
        ),
        Err(error) => (
            Value::String(arguments.to_owned()),
            Err(anyhow::anyhow!(
                "the arguments are not a JSON object: {error}"
            )),
        ),
    };
    match outcome {
        Ok(result) => Step {
            tool: tool.to_owned(),
            arguments,
            is_error: result.is_error == Some(true),
            result: result_value(&result),
        },
        Err(error) => Step {
            tool: tool.to_owned(),
            arguments,
            is_error: true,
            result: json!({ "isError": true, "error": format!("{error:#}") }),
        },
    }
}

/// A tool result as the model reads it: its text, its structured content, and each
/// other block by kind and address.
pub fn result_value(result: &CallToolResult) -> Value {
    let content: Vec<Value> = result
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => json!({ "type": "text", "text": text.text }),
            ContentBlock::ResourceLink(link) => json!({
                "type": "resource_link",
                "uri": link.uri,
                "name": link.name,
                "mimeType": link.mime_type,
            }),
            other => json!({ "type": "other", "description": format!("{other:?}").chars().take(200).collect::<String>() }),
        })
        .collect();
    let mut value = json!({ "isError": result.is_error == Some(true), "content": content });
    if let Some(structured) = &result.structured_content {
        value["structuredContent"] = structured.clone();
    }
    value
}
