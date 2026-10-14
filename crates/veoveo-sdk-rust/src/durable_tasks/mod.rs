//! The MCP Tasks adapter over core's durable Task runtime: it projects Task snapshots
//! onto MCP Task results, serves `tasks/*` requests for the caller's own Tasks and lets a
//! hosted server run a tool as a durable Task.
mod hosting;
mod mcp;
mod service;
mod work;

pub use hosting::{DurableListener, DurableTasks, TasksOnly, WithResources};
pub use mcp::{mcp_task_completion, project_snapshot, task_seed};
pub use service::{
    DurableTaskService, DurableTaskSubscription, DurableTaskUpdateStream,
    TASK_RETENTION_PIN_META_KEY, authorized_snapshot, cancel_durable_task, durable_input_responses,
    get_durable_task, listen_durable_subscriptions, restore_task_retention_meta, retention_pins,
    start_durable_tool_task, subscribe_authorized_snapshots, subscribe_durable_tasks,
    update_durable_task,
};
pub use work::{
    CallbackUrlError, CallbackUrls, DEFAULT_CANCELLATION, DEFAULT_DRAIN, OperationDispatch,
    OperationStep, SETTLE_RETRY_FIRST_DELAY, SETTLE_RETRY_MAX_DELAY, SETTLE_RETRY_WINDOW,
    TaskClientError, TaskContext, TaskStop, TaskWorkError, TaskWorker,
};
