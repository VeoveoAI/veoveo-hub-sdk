//! Projection from durable Platform Store tasks to the official MCP Tasks model.

use rmcp::model::{
    DetailedTask, ErrorData, InputRequest, JsonObject, Task, TaskPayload,
    TaskStatus as McpTaskStatus,
};
use serde_json::{Value, json};

use veoveo_task::{TaskError, TaskRuntime, TaskSnapshot, TaskStatus};

/// Projects durable metadata into the seed returned by `CreateTaskResult`. A waiting
/// Task is working in the seed: it asks for input only while it holds outstanding input
/// requests, which [`project_snapshot`] reads, and otherwise it is parked on an
/// operation or between steps.
pub fn task_seed(snapshot: &TaskSnapshot) -> Task {
    let mut task = Task::new(
        snapshot.task_id.to_string(),
        task_status(snapshot.status),
        snapshot.created_at.to_rfc3339(),
        snapshot.updated_at.to_rfc3339(),
    );
    task.status_message = snapshot.status_message.clone();
    task.ttl_ms = snapshot.ttl_ms;
    task.poll_interval_ms = snapshot.poll_interval_ms;
    task
}

/// Projects the current durable state into the status-specific official task union.
pub async fn project_snapshot(
    runtime: &TaskRuntime,
    snapshot: TaskSnapshot,
) -> Result<DetailedTask, TaskError> {
    let mut task = task_seed(&snapshot);
    // A completed result carries the Task's cost in its `_meta`.
    let usage = match snapshot.status {
        TaskStatus::Succeeded => crate::usage::task_usage_meta(runtime, &snapshot).await?,
        _ => None,
    };
    let payload = match snapshot.status {
        TaskStatus::Queued | TaskStatus::Running | TaskStatus::CancelRequested => {
            TaskPayload::Working
        }
        TaskStatus::Waiting => {
            let requests = runtime.outstanding_inputs(snapshot.task_id).await?;
            if requests.is_empty() {
                TaskPayload::Working
            } else {
                let input_requests = requests
                    .into_iter()
                    .map(|(key, request)| {
                        let envelope = json!({
                            "method": request.kind,
                            "params": request.params,
                        });
                        let request = serde_json::from_value::<InputRequest>(envelope).map_err(
                            |error| {
                                TaskError::InvalidRecord(format!(
                                    "task input request is not a supported MCP input request: {error}"
                                ))
                            },
                        )?;
                        Ok((key, request))
                    })
                    .collect::<Result<_, TaskError>>()?;
                task.status = McpTaskStatus::InputRequired;
                TaskPayload::InputRequired { input_requests }
            }
        }
        TaskStatus::Succeeded => {
            let result = match snapshot.result {
                Some(Value::Object(result)) => result,
                Some(value) => JsonObject::from_iter([("value".to_owned(), value)]),
                None => {
                    return Err(TaskError::InvalidRecord(
                        "completed task has no durable result".to_owned(),
                    ));
                }
            };
            let mut result = result;
            if let Some(usage) = usage {
                let meta = result
                    .entry("_meta")
                    .or_insert_with(|| Value::Object(JsonObject::new()));
                if let Value::Object(meta) = meta {
                    meta.insert(crate::usage::TASK_USAGE_META_KEY.to_owned(), usage);
                }
            }
            TaskPayload::Completed { result }
        }
        TaskStatus::Failed => {
            let failure = snapshot.error.ok_or_else(|| {
                TaskError::InvalidRecord("failed task has no durable error".to_owned())
            })?;
            let error = ErrorData::internal_error(
                failure.message,
                Some(json!({
                    "taskCode": failure.code,
                    "details": failure.details,
                })),
            );
            let error = serde_json::to_value(error)
                .map_err(|error| TaskError::InvalidRecord(error.to_string()))?
                .as_object()
                .cloned()
                .expect("MCP error serializes as an object");
            TaskPayload::Failed { error }
        }
        TaskStatus::Cancelled => TaskPayload::Cancelled,
    };
    Ok(DetailedTask::new(task, payload))
}

fn task_status(status: TaskStatus) -> McpTaskStatus {
    match status {
        TaskStatus::Queued
        | TaskStatus::Running
        | TaskStatus::Waiting
        | TaskStatus::CancelRequested => McpTaskStatus::Working,
        TaskStatus::Succeeded => McpTaskStatus::Completed,
        TaskStatus::Failed => McpTaskStatus::Failed,
        TaskStatus::Cancelled => McpTaskStatus::Cancelled,
    }
}

/// Admits a complete MCP result before writing the kernel's product address.
pub fn mcp_task_completion(
    message: impl Into<String>,
    result: rmcp::model::CallToolResult,
) -> Result<veoveo_task::TaskTransition, TaskError> {
    let result_uri = crate::task_completion::result_uri(&result).map_err(|_| {
        TaskError::InvalidRecord("invalid MCP Task completion product address".into())
    })?;
    let result = serde_json::to_value(result)
        .map_err(|_| TaskError::InvalidRecord("MCP Task completion serialization failed".into()))?;
    Ok(veoveo_task::TaskTransition::Succeeded {
        message: message.into(),
        result,
        result_uri,
    })
}
