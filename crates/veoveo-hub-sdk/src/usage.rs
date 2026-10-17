//! A server's usage in core's usage ledger, shaped by FOCUS 1.4.
//!
//! A server declares its kinds of usage at startup from a closed enum that implements
//! [`UsageKinds`], each under its own slug, with its FOCUS service category, unit and
//! whether it carries money. A Task's work records usage through `ctx.usage()`: an
//! estimate before billed work, which is the work's reservation, and the actual amount
//! once the provider reports it. Each record is for the caller the Task runs for, names
//! the Task, and is written once per source identifier. Every Task result and
//! `tasks/get` answer carries the Task's cost under [`TASK_USAGE_META_KEY`].
pub use veoveo_usage_contract::{
    Amount, ChargeCategory, ChargePeriod, Currency, Money, Quantity, ServiceCategory, TaskUsage,
    UsageKind, UsageKindDeclaration, UsagePhase, UsageSourceId, UsageUnit,
};

/// The `_meta` key of a Task's cost: its [`TaskUsage`] per kind and currency, actual
/// records when a kind has any and estimates otherwise.
pub const TASK_USAGE_META_KEY: &str = "ai.veoveo/usage";

/// A server's closed vocabulary of usage kinds.
pub trait UsageKinds: Copy + Send + Sync + 'static {
    /// Every kind the server records.
    fn all() -> &'static [Self];
    /// The kind's declaration: its name under the server's slug, FOCUS service category,
    /// unit and whether it carries money.
    fn declaration(self) -> UsageKindDeclaration;
}

/// Why a usage record was not written.
#[derive(Debug, thiserror::Error)]
pub enum UsageRecordError {
    #[error("the Task worker was built without the usage ledger")]
    NoLedger,
    #[error(transparent)]
    Invalid(#[from] veoveo_usage_contract::UsageError),
    #[error("invalid usage source identifier: {0}")]
    Source(String),
    #[cfg(feature = "server")]
    #[error(transparent)]
    Store(#[from] veoveo_usage::UsageStoreError),
}

#[cfg(feature = "server")]
pub use ledger::{TaskUsageRecorder, UsageLedger, UsageRecord, task_usage_meta};

#[cfg(feature = "server")]
mod ledger {
    use std::future::{Future, IntoFuture};
    use std::pin::Pin;

    use veoveo_store::PlatformStore;
    use veoveo_task::{OperationKey, TaskError, TaskRuntime, TaskSnapshot};
    use veoveo_types::TaskId;
    use veoveo_usage::{UsageStore, UsageWrite};
    use veoveo_usage_contract::{UsageActor, UsageDraft};

    use super::*;

    /// The usage ledger as a server's login reaches it.
    #[derive(Clone)]
    pub struct UsageLedger {
        platform: PlatformStore,
        store: UsageStore,
    }

    impl UsageLedger {
        /// The ledger through the server's own login: the store its Task runtime uses.
        pub fn new(runtime: &TaskRuntime) -> Self {
            let platform = runtime.platform_store().clone();
            Self {
                store: UsageStore::new(platform.clone()),
                platform,
            }
        }

        /// Declare the server's kinds. Repeating an identical declaration changes
        /// nothing; a kind outside the server's slug is refused.
        pub async fn declare<K: UsageKinds>(&self) -> Result<(), UsageRecordError> {
            let declarations: Vec<UsageKindDeclaration> =
                K::all().iter().map(|kind| kind.declaration()).collect();
            self.store.declare_as_server(&declarations).await?;
            Ok(())
        }

        /// Records for the caller `task` runs for.
        pub fn for_task(&self, task: &TaskSnapshot) -> TaskUsageRecorder {
            let authority = &task.owner.authority;
            TaskUsageRecorder {
                platform: self.platform.clone(),
                store: self.store.clone(),
                actor: UsageActor {
                    tenant: authority.tenant.clone(),
                    principal: task.owner.principal_key.clone(),
                    work_context: authority.work_context.clone(),
                },
                task: task.task_id,
                created_at: task.created_at,
            }
        }

        /// A Task's cost, as its owner reads it.
        pub async fn task_usage(
            &self,
            task: &TaskSnapshot,
        ) -> Result<Vec<TaskUsage>, UsageRecordError> {
            Ok(self
                .store
                .task_usage(&task.owner.authority.tenant, task.task_id)
                .await?)
        }
    }

    /// The `_meta` value of a Task's cost, or `None` when it has no usage records.
    pub async fn task_usage_meta(
        runtime: &TaskRuntime,
        task: &TaskSnapshot,
    ) -> Result<Option<serde_json::Value>, TaskError> {
        let usage = UsageLedger::new(runtime)
            .task_usage(task)
            .await
            .map_err(|error| {
                TaskError::InvalidRecord(format!("the Task's usage is unreadable: {error}"))
            })?;
        if usage.is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_json::to_value(usage)?))
    }

    /// The usage records of one Task's work.
    #[derive(Clone)]
    pub struct TaskUsageRecorder {
        platform: PlatformStore,
        store: UsageStore,
        actor: UsageActor,
        task: TaskId,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    impl TaskUsageRecorder {
        /// An estimate before billed work: the work's reservation, which a budget can
        /// refuse before the provider is called.
        pub fn estimate<K: UsageKinds>(&self, kind: K, quantity: Quantity) -> UsageRecord<'_> {
            UsageRecord::new(
                self,
                kind.declaration().kind,
                UsagePhase::Estimate,
                quantity,
            )
        }

        /// The usage the provider reported.
        pub fn actual<K: UsageKinds>(&self, kind: K, quantity: Quantity) -> UsageRecord<'_> {
            UsageRecord::new(self, kind.declaration().kind, UsagePhase::Actual, quantity)
        }
    }

    /// One usage record of a Task, written when awaited. Its source identifier defaults
    /// to `task:<task>:<operation key>`, or `task:<task>` without an operation, so a step
    /// that runs again writes nothing new.
    pub struct UsageRecord<'a> {
        recorder: &'a TaskUsageRecorder,
        kind: UsageKind,
        phase: UsagePhase,
        quantity: Quantity,
        money: Option<Money>,
        operation: Option<OperationKey>,
        target: Option<String>,
        source: Option<UsageSourceId>,
        period: Option<ChargePeriod>,
        category: ChargeCategory,
        correction: bool,
    }

    impl<'a> UsageRecord<'a> {
        fn new(
            recorder: &'a TaskUsageRecorder,
            kind: UsageKind,
            phase: UsagePhase,
            quantity: Quantity,
        ) -> Self {
            Self {
                recorder,
                kind,
                phase,
                quantity,
                money: None,
                operation: None,
                target: None,
                source: None,
                period: None,
                category: ChargeCategory::Usage,
                correction: false,
            }
        }

        pub fn money(mut self, money: Money) -> Self {
            self.money = Some(money);
            self
        }

        /// The operation of the Task the usage belongs to.
        pub fn operation(mut self, key: &OperationKey) -> Self {
            self.operation = Some(key.clone());
            self
        }

        /// The resource the usage is about, as its owner's URI.
        pub fn target(mut self, uri: impl Into<String>) -> Self {
            self.target = Some(uri.into());
            self
        }

        /// The producer's identifier of what the record counts, unique within its kind
        /// and phase, such as `task:<id>:output:2`.
        pub fn source(mut self, source: UsageSourceId) -> Self {
            self.source = Some(source);
            self
        }

        /// When the usage happened. It defaults to the Task's creation, so a record a
        /// replayed run writes again is identical; pass the provider's reported time for
        /// actual usage.
        pub fn period(mut self, period: ChargePeriod) -> Self {
            self.period = Some(period);
            self
        }

        /// A credit, such as a refund, or an adjustment, instead of usage.
        pub fn charge_category(mut self, category: ChargeCategory) -> Self {
            self.category = category;
            self
        }

        /// The record corrects an earlier one (FOCUS ChargeClass `Correction`).
        pub fn correction(mut self) -> Self {
            self.correction = true;
            self
        }

        fn draft(&self) -> Result<UsageDraft, UsageRecordError> {
            let task = self.recorder.task;
            let source = match &self.source {
                Some(source) => source.clone(),
                None => {
                    let id = match &self.operation {
                        Some(key) => format!("task:{}:{}", task.as_uuid(), key.as_str()),
                        None => format!("task:{}", task.as_uuid()),
                    };
                    UsageSourceId::parse(id)
                        .map_err(|error| UsageRecordError::Source(error.to_string()))?
                }
            };
            let period = self
                .period
                .unwrap_or_else(|| ChargePeriod::at(self.recorder.created_at));
            let mut builder = UsageDraft::builder(
                self.kind.clone(),
                self.phase,
                self.recorder.actor.clone(),
                self.quantity.clone(),
                source,
                period,
            )
            .task(task, self.operation.as_ref().map(OperationKey::as_str))
            .charge_category(self.category);
            if let Some(money) = &self.money {
                builder = builder.money(money.clone());
            }
            if let Some(target) = &self.target {
                builder = builder.target(target.clone());
            }
            if self.correction {
                builder = builder.correction();
            }
            Ok(builder.build()?)
        }
    }

    impl<'a> IntoFuture for UsageRecord<'a> {
        type Output = Result<(), UsageRecordError>;
        type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

        fn into_future(self) -> Self::IntoFuture {
            Box::pin(async move {
                let draft = self.draft()?;
                match self.phase {
                    UsagePhase::Estimate => {
                        let mut connection = self
                            .recorder
                            .platform
                            .pool()
                            .acquire()
                            .await
                            .map_err(veoveo_usage::UsageStoreError::from)?;
                        UsageWrite::reserve(&mut connection, &draft).await?;
                    }
                    UsagePhase::Actual => {
                        self.recorder
                            .store
                            .append_as_server(std::slice::from_ref(&draft))
                            .await?;
                    }
                }
                Ok(())
            })
        }
    }
}
