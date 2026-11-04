//! A durable Task an ordinary tool's call starts for its caller: owned as a Task tool's
//! Task is, and named in the call's result as its related task, so the Hub records it
//! among the caller's Tasks.
use std::sync::Arc;

use rmcp::{ErrorData, RoleServer, service::RequestContext};
use serde_json::Value;
use veoveo_task::{
    CreateTask, PrincipalKind, RecoveryClass, TaskOwner, TaskRuntime, TaskSnapshot, TaskSurface,
};
use veoveo_types::{TaskId, TaskTypeName};

use crate::{HubInternalIdentity, hosting::hub_identity};

/// The Task a call starts: its type, its request and how it recovers, its time to live
/// and the interval at which a caller polls it.
#[derive(Debug, Clone)]
pub struct CallTask {
    pub task_type: TaskTypeName,
    pub request: Value,
    pub recovery_class: RecoveryClass,
    pub ttl_ms: Option<u64>,
    pub poll_interval_ms: Option<u64>,
}

/// The Task one tool call started, which the routes name in the call's result.
#[derive(Debug, Clone, Default)]
pub(crate) struct CallTasks(Arc<parking_lot::Mutex<Slot>>);

#[derive(Debug, Default)]
enum Slot {
    #[default]
    Empty,
    Starting,
    Started(TaskId),
}

impl CallTasks {
    /// The Task the call started, when it started one.
    pub(crate) fn started(&self) -> Option<TaskId> {
        match &*self.0.lock() {
            Slot::Started(task) => Some(*task),
            _ => None,
        }
    }

    fn reserve(&self) -> Result<(), ErrorData> {
        let mut slot = self.0.lock();
        match *slot {
            Slot::Empty => {
                *slot = Slot::Starting;
                Ok(())
            }
            _ => Err(ErrorData::internal_error(
                "a tool's call starts at most one Task",
                None,
            )),
        }
    }

    fn settle(&self, task: Option<TaskId>) {
        *self.0.lock() = match task {
            Some(task) => Slot::Started(task),
            None => Slot::Empty,
        };
    }
}

/// The Task owner of the verified caller, as a Task tool's Task records it: the actor, the
/// Hub profile it called through as the Task's surface, its tenant, data labels and
/// invocation authority. A Task with this owner calls other servers for the caller
/// through `ctx.client()`.
pub fn task_owner(identity: &HubInternalIdentity) -> Result<TaskOwner, ErrorData> {
    Ok(TaskOwner {
        principal_key: identity.actor.id.clone(),
        principal_kind: match identity.actor.kind {
            crate::PrincipalKind::User => PrincipalKind::User,
            crate::PrincipalKind::Service => PrincipalKind::Service,
        },
        issuer: identity.actor.issuer.clone(),
        subject: identity.actor.subject.clone(),
        surface: TaskSurface::parse(identity.profile.as_str())
            .map_err(|_| ErrorData::invalid_request("the profile is no Task surface", None))?,
        tenant_key: identity.actor.tenant.clone(),
        data_labels: identity.actor.data_labels.clone(),
        authority: identity.authority.clone(),
    })
}

/// Starts `task` in `runtime` for the verified caller of the tool call `context` answers,
/// and has the call's result name it in `_meta["io.modelcontextprotocol/related-task"]`.
/// A call starts at most one such Task, from a tool handler of routes that declare
/// [`starts_tasks`](crate::hosting::RoutesBuilder::starts_tasks). The Hub records it as
/// the caller's Task, which the caller follows and cancels like any other; the Task calls
/// other servers for the caller through `ctx.client()` when the server declares uses and
/// the call came from the caller directly, not from another server. A handler starts it
/// as its last step: a call that fails after starting it names it nowhere.
pub async fn start_call_task(
    runtime: &TaskRuntime,
    context: &RequestContext<RoleServer>,
    task: CallTask,
) -> Result<TaskSnapshot, ErrorData> {
    let slot = context
        .extensions
        .get::<CallTasks>()
        .cloned()
        .ok_or_else(|| {
            ErrorData::internal_error(
                "a call-started Task starts in a tool call of routes that declare starts_tasks",
                None,
            )
        })?;
    let identity = hub_identity(context)?;
    let owner = task_owner(&identity)?;
    slot.reserve()?;
    let created = runtime
        .create(CreateTask {
            task_id: TaskId::new(),
            owner,
            server: runtime.server().clone(),
            task_type: task.task_type,
            request: task.request,
            recovery_class: task.recovery_class,
            idempotency_key: None,
            ttl_ms: task.ttl_ms,
            poll_interval_ms: task.poll_interval_ms,
            retention_pins: Default::default(),
        })
        .await;
    match created {
        Ok(created) => {
            slot.settle(Some(created.snapshot.task_id));
            Ok(created.snapshot)
        }
        Err(_) => {
            slot.settle(None);
            Err(ErrorData::internal_error(
                "the call's Task could not be created",
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_starts_at_most_one_task_and_a_failed_start_frees_its_slot() {
        let calls = CallTasks::default();
        assert_eq!(calls.started(), None);
        calls.reserve().unwrap();
        // A start under way holds the slot without naming a Task.
        assert_eq!(calls.started(), None);
        assert!(calls.reserve().is_err());
        calls.settle(None);
        calls.reserve().unwrap();
        let task = TaskId::new();
        calls.settle(Some(task));
        assert_eq!(calls.started(), Some(task));
        let refused = calls.reserve().unwrap_err();
        assert!(refused.message.contains("at most one Task"));
        assert_eq!(calls.started(), Some(task));
    }
}
