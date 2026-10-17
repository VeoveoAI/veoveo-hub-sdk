//! Provider events for a hosted server's operations, on routes without Hub
//! authentication: a provider authenticates through the operation's callback token and
//! the signature the server's [`ProviderEvents`] parser checks.
//!
//! | Route under the server's mount | Event target |
//! |---|---|
//! | `POST /operations/{operation_id}/{token}` | The operation the callback URL names, whose token must match |
//! | `POST /providers/{provider}/events` | The operation the provider's job identifier names |
//!
//! The route stores the event through core's Task runtime before it answers. A new,
//! repeated or late event answers `204 No Content`, so the provider stops retrying. An
//! operation the route cannot find, a malformed path and a wrong token answer the same
//! `404`; a refused signature answers `401`, a body the parser cannot read `400`, and an
//! unavailable database `503`, which a provider retries. The request span records the
//! route template, never the token.
use std::{future::Future, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;
use veoveo_task::{
    CallbackToken, Delivery, EventTarget, ExternalJobId, OperationEvent, OperationEventId,
    ProviderName, StepOperationId, TaskRuntime,
};

/// The largest event body a route reads.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// A provider's request as the route received it.
#[derive(Debug, Clone, Copy)]
pub struct ProviderRequest<'a> {
    pub headers: &'a HeaderMap,
    pub body: &'a Bytes,
}

/// One event the server's parser read from a provider's request after checking its
/// signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEvent {
    /// The provider's job identifier, which the job route requires.
    pub job: Option<ExternalJobId>,
    /// The provider's identifier of this event; a repeated identifier is the same event.
    pub event_id: OperationEventId,
    pub payload: Value,
    /// Whether the payload is the operation's outcome.
    pub is_final: bool,
}

/// Why a parser refused a provider's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRejection {
    /// The signature is missing, wrong or outside its accepted time window.
    Unauthenticated,
    /// The signed body is no event the server reads.
    Malformed,
}

/// The server's provider-specific reading of event requests: the signature check and
/// the event's payload, identifier and finality.
pub trait ProviderEvents: Send + Sync + 'static {
    fn parse(
        &self,
        request: ProviderRequest<'_>,
    ) -> impl Future<Output = Result<ProviderEvent, ProviderRejection>> + Send;
}

struct Routes<P> {
    runtime: TaskRuntime,
    events: P,
}

/// `POST /operations/{operation_id}/{token}`: events a provider sends to an operation's
/// callback URL. Merge it into the server's public routes.
pub fn operation_callbacks<P: ProviderEvents>(runtime: TaskRuntime, events: P) -> Router {
    Router::new()
        .route("/operations/{operation_id}/{token}", post(callback::<P>))
        .layer(DefaultBodyLimit::max(MAX_EVENT_BYTES))
        .with_state(Arc::new(Routes { runtime, events }))
}

/// `POST /providers/{provider}/events`: events a provider sends to one fixed URL, which
/// name the operation by the provider's job identifier. Merge it into the server's
/// public routes.
pub fn provider_events<P: ProviderEvents>(
    runtime: TaskRuntime,
    provider: ProviderName,
    events: P,
) -> Router {
    Router::new()
        .route(
            &format!("/providers/{}/events", provider.as_str()),
            post(job_event::<P>),
        )
        .layer(DefaultBodyLimit::max(MAX_EVENT_BYTES))
        .with_state(Arc::new((Routes { runtime, events }, provider)))
}

async fn callback<P: ProviderEvents>(
    State(routes): State<Arc<Routes<P>>>,
    path: Result<Path<(String, String)>, PathRejection>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(Path((operation_id, token))) = path else {
        return unknown();
    };
    let (Ok(operation_id), Some(token)) = (
        StepOperationId::parse(operation_id.as_str()),
        CallbackToken::parse(&token),
    ) else {
        return unknown();
    };
    let event = match routes
        .events
        .parse(ProviderRequest {
            headers: &headers,
            body: &body,
        })
        .await
    {
        Ok(event) => event,
        Err(rejection) => return rejected(rejection),
    };
    deliver(
        &routes.runtime,
        OperationEvent {
            target: EventTarget::Callback {
                operation_id,
                token,
            },
            event_id: event.event_id,
            payload: event.payload,
            is_final: event.is_final,
        },
    )
    .await
}

async fn job_event<P: ProviderEvents>(
    State(state): State<Arc<(Routes<P>, ProviderName)>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (routes, provider) = &*state;
    let event = match routes
        .events
        .parse(ProviderRequest {
            headers: &headers,
            body: &body,
        })
        .await
    {
        Ok(event) => event,
        Err(rejection) => return rejected(rejection),
    };
    let Some(external_id) = event.job else {
        return rejected(ProviderRejection::Malformed);
    };
    deliver(
        &routes.runtime,
        OperationEvent {
            target: EventTarget::Job {
                provider: provider.clone(),
                external_id,
            },
            event_id: event.event_id,
            payload: event.payload,
            is_final: event.is_final,
        },
    )
    .await
}

async fn deliver(runtime: &TaskRuntime, event: OperationEvent) -> Response {
    match runtime.deliver(&event).await {
        Ok(Delivery::New | Delivery::Repeat | Delivery::Late) => {
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Delivery::Unknown) => unknown(),
        Err(error) => {
            // The span names the route template; the token never reaches the log.
            tracing::warn!(%error, "a provider event was not stored");
            (StatusCode::SERVICE_UNAVAILABLE, "the event was not stored").into_response()
        }
    }
}

fn unknown() -> Response {
    (StatusCode::NOT_FOUND, "no such operation").into_response()
}

fn rejected(rejection: ProviderRejection) -> Response {
    match rejection {
        ProviderRejection::Unauthenticated => {
            (StatusCode::UNAUTHORIZED, "invalid signature").into_response()
        }
        ProviderRejection::Malformed => {
            (StatusCode::BAD_REQUEST, "unreadable event").into_response()
        }
    }
}
