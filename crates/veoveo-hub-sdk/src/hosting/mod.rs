//! Hosting for MCP servers behind the Veoveo hub.
//!
//! A server supplies its domain and nothing else:
//!
//! - its checked [`McpServerSetup`]: scopes, typed resource addresses, templates,
//!   and embedded documents;
//! - a [`DomainServer`] implementation exposing ready [`Routes`], with typed
//!   tool outputs and [`ResourceBuilder`] readers, explicit policies/cache and
//!   native prompt renderers, plus optional completion;
//! - optional [`TaskSupport`] for durable tasks and subscriptions;
//! - optional [`ToolDescriptions`] for tool descriptions it changes while it runs.
//!
//! This module supplies the rest of the server contract, identically for every
//! server. [`Hosted`] is the one `ServerHandler`, and [`HostedServer`] builds the
//! HTTP service around it: routes, hub authentication, host validation, health,
//! readiness, administrative documents, the stateless MCP transport, and shutdown.
//! Inside a domain method, [`hub_identity`] and [`forwarded_bearer`] return the
//! verified caller, and `ctx.client()` through [`ClientContext`] calls other servers for
//! that caller when the server was built with [`ServerCalls`].
//!
//! ```ignore
//! let server = HostedServer::for_domain::<MyDomain>()
//!     .deployment(&public_deployment, args.allow_loopback_hosts)?
//!     .internal_trust(registration.access_tokens(), http.clone())?
//!     .handler(move || Hosted::new(MyDomain::new(state.clone())))
//!     .build();
//! server.serve(address).await
//! ```
//!
//! [`McpServerSetup`]: crate::server_contract::McpServerSetup

mod admin;
mod arguments;
mod auth;
mod calls;
mod catalog;
mod descriptions;
mod domain;
mod host;
mod listing;
pub mod operations;
mod results;
mod routes;
pub use crate::definition::TemplateRead;
pub use routes::{
    AppBuilder, ResourceBuilder, RoutePolicy, Routes, RoutesBuilder, ToolError, task_result,
};
mod schema;
mod server;
mod subscriptions;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use auth::{ForwardedBearer, forwarded_bearer, hub_identity};
pub use calls::{ClientContext, ServerCalls};
pub use descriptions::ToolDescriptions;
pub use domain::{
    DomainServer, Hosted, NoTasks, ReadCache, TaskSupport, served_by_host, unknown_prompt,
};
pub use listing::{CATALOG_PAGE_SIZE, Listing};
pub use results::{completion, rank_completions};
pub use schema::{
    AmbiguousArgument, UndeclaredType, ambiguous_arguments, publish_input_schema, undeclared_types,
};
pub use server::{Deployment, HostedServer, HostedServerBuilder, Missing, Provided, PublicRoutes};
pub use subscriptions::{
    ListenOnly, ResourceSubscriptions, ResourcesOnly, SubscriptionListener,
    admit_resource_subscriptions, requested_addresses,
};
