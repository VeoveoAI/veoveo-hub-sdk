//! Calls from a hosted server to other servers, for its own caller.
//!
//! [`ServerCalls`] holds the server's registration: its client identifier, the key
//! pinned to its namespace, the permissions it declared in `uses` and the Gateway's
//! token endpoint. A tool handler calls `ctx.client()` through [`ClientContext`], and a
//! Task worker calls [`ServerCalls::for_task`]. Both return a
//! [`GatewayClient`](crate::client::GatewayClient) bound to whom the call is for.
//!
//! The first call to a provider exchanges the binding's subject for a token at the
//! Gateway (RFC 8693). The subject is the forwarded token of the current request, or
//! the server's own Task, which stands for the caller that created it while it runs.
//! The request names the declared uses in the provider's namespace, and the Gateway
//! narrows them to what the caller holds. The token is cached per binding and provider
//! until shortly before it expires.
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use futures::future::BoxFuture;
use reqwest::{
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use rmcp::{ErrorData, RoleServer, service::RequestContext};
use secrecy::ExposeSecret;
use veoveo_task_runtime::TaskSnapshot;
use veoveo_types::{OAuthClientId, PermissionName, ServerSlug, TaskId};

use super::auth::ForwardedBearer;
use crate::{
    GatewayInternalIdentity, GatewayProfileId, JwtId,
    calls::{
        ExchangeSubject, TOKEN_EXCHANGE_GRANT_TYPE, TokenErrorCode, TokenErrorResponse,
        TokenExchangeResponse,
    },
    client::{Bearer, BearerSource, CallError, GatewayClient},
    registration::{
        AuthorizationServerMetadata, ClientMetadata, PermissionsDetail,
        assertion::CLIENT_ASSERTION_TYPE,
        client::{ClientError, DEFAULT_REQUEST_TIMEOUT, Registration},
        statement::ServerKeyPair,
    },
};

/// How long before its expiry a cached token is exchanged again.
const REFRESH_MARGIN: Duration = Duration::from_secs(5);
/// How long a Task's exchange is repeated while the Gateway refuses it as an invalid
/// grant. The Gateway records a Task's route once the call that created it returns,
/// so a Task that calls another server at once can reach the Gateway first.
const TASK_ROUTE_WAIT: Duration = Duration::from_secs(2);
const TASK_ROUTE_RETRY: Duration = Duration::from_millis(100);

/// The registration a server calls other servers with.
#[derive(Clone)]
pub struct ServerCalls {
    inner: Arc<Inner>,
}

struct Inner {
    server: ServerSlug,
    client: OAuthClientId,
    key: ServerKeyPair,
    issuer: url::Url,
    token_endpoint: url::Url,
    uses: Vec<PermissionName>,
    http: reqwest::Client,
    tokens: parking_lot::Mutex<HashMap<TokenKey, CachedToken>>,
}

impl std::fmt::Debug for ServerCalls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerCalls")
            .field("server", &self.inner.server)
            .field("client", &self.inner.client)
            .field("token_endpoint", &self.inner.token_endpoint.as_str())
            .finish_non_exhaustive()
    }
}

/// Whom a binding acts for: the request a forwarded token carried, or a Task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Binding {
    Request(JwtId),
    Task(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    binding: Binding,
    provider: ServerSlug,
}

struct CachedToken {
    bearer: Bearer,
    refresh_at: Instant,
}

impl ServerCalls {
    /// Calls as the registered server `metadata` describes, signing with `key`, through
    /// the Gateway issuer whose token endpoint is `token_endpoint`.
    pub fn new(
        registration: &Registration,
        metadata: &ClientMetadata,
        key: &ServerKeyPair,
        token_endpoint: url::Url,
        http: reqwest::Client,
    ) -> Result<Self, ClientError> {
        let issuer = url::Url::parse(registration.access_tokens().issuer.as_str())
            .map_err(|_| ClientError::ResponseMismatch("names an issuer that is no URL"))?;
        Self::with_identity(
            registration.client_id().clone(),
            metadata,
            key,
            issuer,
            token_endpoint,
            http,
        )
    }

    pub(crate) fn with_identity(
        client: OAuthClientId,
        metadata: &ClientMetadata,
        key: &ServerKeyPair,
        issuer: url::Url,
        token_endpoint: url::Url,
        http: reqwest::Client,
    ) -> Result<Self, ClientError> {
        if metadata.key() != key.public_key() {
            return Err(ClientError::ResponseMismatch(
                "names another key than the server signs with",
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                server: metadata.slug().clone(),
                client,
                key: ServerKeyPair::from_pkcs8_der(key.pkcs8_der())?,
                issuer,
                token_endpoint,
                uses: metadata.authorization.uses.clone(),
                http,
                tokens: parking_lot::Mutex::new(HashMap::new()),
            }),
        })
    }

    /// [`Self::new`] with the token endpoint read from the Gateway issuer's RFC 8414
    /// metadata.
    pub async fn discover(
        registration: &Registration,
        metadata: &ClientMetadata,
        key: &ServerKeyPair,
        http: reqwest::Client,
    ) -> Result<Self, ClientError> {
        let issuer = url::Url::parse(registration.access_tokens().issuer.as_str())
            .map_err(|_| ClientError::ResponseMismatch("names an issuer that is no URL"))?;
        let response = http
            .get(AuthorizationServerMetadata::url_for(&issuer))
            .header(ACCEPT, "application/json")
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if status != StatusCode::OK {
            return Err(ClientError::Rejected {
                status,
                error: None,
            });
        }
        let published: AuthorizationServerMetadata =
            serde_json::from_slice(&body).map_err(ClientError::InvalidResponse)?;
        if published.issuer != issuer {
            return Err(ClientError::ResponseMismatch(
                "names another issuer than the one discovered",
            ));
        }
        let token_endpoint = published.token_endpoint_url()?.clone();
        Self::new(registration, metadata, key, token_endpoint, http)
    }

    /// The server these calls come from.
    pub fn server(&self) -> &ServerSlug {
        &self.inner.server
    }

    /// The MCP endpoint of a Gateway profile, `{issuer}/mcp/{profile}`.
    pub fn profile_endpoint(&self, profile: &GatewayProfileId) -> url::Url {
        let mut endpoint = self.inner.issuer.clone();
        endpoint.set_path(&format!("/mcp/{profile}"));
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        endpoint
    }

    /// Calls for the caller of the request that `identity` and `bearer` came with. The
    /// binding lasts as long as that forwarded token.
    pub fn for_request(
        &self,
        identity: &GatewayInternalIdentity,
        bearer: &ForwardedBearer,
    ) -> GatewayClient {
        self.client(
            &identity.profile,
            Binding::Request(identity.jwt_id.clone()),
            ExchangeSubject::AccessToken(bearer.as_str().to_owned()),
        )
    }

    /// Calls for the caller that created `task` through the Gateway, while the Task
    /// runs.
    pub fn for_task(&self, task: &TaskSnapshot) -> Result<GatewayClient, CallError> {
        let profile = GatewayProfileId::parse(task.owner.surface.as_str()).map_err(|_| {
            CallError::Transport(format!(
                "Task {} was not created through a Gateway profile",
                task.task_id
            ))
        })?;
        Ok(self.for_task_id(task.task_id, &profile))
    }

    /// [`Self::for_task`] for a Task known by its identifier and the profile it was
    /// created through.
    pub fn for_task_id(&self, task: TaskId, profile: &GatewayProfileId) -> GatewayClient {
        let task = task.to_string();
        self.client(
            profile,
            Binding::Task(task.clone()),
            ExchangeSubject::Task(task),
        )
    }

    fn client(
        &self,
        profile: &GatewayProfileId,
        binding: Binding,
        subject: ExchangeSubject,
    ) -> GatewayClient {
        let endpoint = self.profile_endpoint(profile);
        let source = ExchangeSource {
            calls: self.clone(),
            binding,
            subject,
            resource: endpoint.clone(),
        };
        GatewayClient::with_source(endpoint, self.inner.http.clone(), Arc::new(source))
            .named(self.inner.server.as_str(), env!("CARGO_PKG_VERSION"))
    }

    fn forget(&self, key: &TokenKey) {
        self.inner.tokens.lock().remove(key);
    }

    fn cached(&self, key: &TokenKey) -> Option<Bearer> {
        let tokens = self.inner.tokens.lock();
        tokens
            .get(key)
            .filter(|token| token.refresh_at > Instant::now())
            .map(|token| token.bearer.clone())
    }

    fn cache(&self, key: TokenKey, bearer: Bearer, lifetime: Duration) {
        let now = Instant::now();
        let mut tokens = self.inner.tokens.lock();
        tokens.retain(|_, token| token.refresh_at > now);
        tokens.insert(
            key,
            CachedToken {
                bearer,
                refresh_at: now + lifetime.saturating_sub(REFRESH_MARGIN),
            },
        );
    }

    /// RFC 8693 §2.1: exchange `subject` for a token at `resource` that carries the
    /// declared uses in `provider`'s namespace, as far as the caller holds them.
    async fn exchange(
        &self,
        subject: &ExchangeSubject,
        resource: &url::Url,
        provider: &ServerSlug,
    ) -> Result<TokenExchangeResponse, CallError> {
        let requested: std::collections::BTreeSet<PermissionName> = self
            .inner
            .uses
            .iter()
            .filter(|permission| permission.is_owned_by(provider))
            .cloned()
            .collect();
        if requested.is_empty() {
            return Err(CallError::Undeclared {
                provider: provider.clone(),
            });
        }
        let assertion = self
            .inner
            .key
            .client_assertion(&self.inner.client, &self.inner.token_endpoint, Utc::now())
            .map_err(|error| CallError::Transport(error.to_string()))?;
        let details = serde_json::to_string(&[PermissionsDetail::new(requested)])?;
        let (subject_token, subject_token_type) = subject.parameters();
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", TOKEN_EXCHANGE_GRANT_TYPE)
            .append_pair("client_id", self.inner.client.as_str())
            .append_pair("client_assertion_type", CLIENT_ASSERTION_TYPE)
            .append_pair("client_assertion", assertion.expose_secret())
            .append_pair("subject_token", subject_token)
            .append_pair("subject_token_type", subject_token_type)
            .append_pair("resource", resource.as_str())
            .append_pair("authorization_details", &details)
            .finish();
        let response = self
            .inner
            .http
            .post(self.inner.token_endpoint.clone())
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ACCEPT, "application/json")
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .body(body)
            .send()
            .await
            .map_err(|error| CallError::Transport(format!("token exchange failed: {error}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| CallError::Transport(format!("token exchange failed: {error}")))?;
        if status == StatusCode::OK {
            return Ok(serde_json::from_slice(&body)?);
        }
        match serde_json::from_slice::<TokenErrorResponse>(&body) {
            Ok(refusal) => Err(CallError::Refused {
                error: refusal.error,
                description: refusal.error_description,
            }),
            Err(_) => Err(CallError::Transport(format!(
                "the token endpoint answered HTTP {status}"
            ))),
        }
    }
}

/// The bearer source of one binding.
struct ExchangeSource {
    calls: ServerCalls,
    binding: Binding,
    subject: ExchangeSubject,
    resource: url::Url,
}

impl BearerSource for ExchangeSource {
    fn bearer<'a>(&'a self, provider: &'a ServerSlug) -> BoxFuture<'a, Result<Bearer, CallError>> {
        Box::pin(async move {
            let key = TokenKey {
                binding: self.binding.clone(),
                provider: provider.clone(),
            };
            if let Some(bearer) = self.calls.cached(&key) {
                return Ok(bearer);
            }
            let started = Instant::now();
            let issued = loop {
                match self
                    .calls
                    .exchange(&self.subject, &self.resource, provider)
                    .await
                {
                    Err(CallError::Refused {
                        error: TokenErrorCode::InvalidGrant,
                        ..
                    }) if matches!(self.subject, ExchangeSubject::Task(_))
                        && started.elapsed() < TASK_ROUTE_WAIT =>
                    {
                        tokio::time::sleep(TASK_ROUTE_RETRY).await;
                    }
                    outcome => break outcome?,
                }
            };
            let bearer = Bearer::new(issued.access_token);
            self.calls
                .cache(key, bearer.clone(), Duration::from_secs(issued.expires_in));
            Ok(bearer)
        })
    }

    fn forget(&self, provider: &ServerSlug) {
        self.calls.forget(&TokenKey {
            binding: self.binding.clone(),
            provider: provider.clone(),
        });
    }
}

/// `ctx.client()` in a tool handler: calls to other servers for this request's caller.
pub trait ClientContext {
    /// A client bound to the caller of this request. It fails when the server was
    /// built without [`ServerCalls`] or the request did not come through the Gateway.
    fn client(&self) -> Result<GatewayClient, ErrorData>;
}

impl ClientContext for RequestContext<RoleServer> {
    fn client(&self) -> Result<GatewayClient, ErrorData> {
        let parts = self
            .extensions
            .get::<axum::http::request::Parts>()
            .ok_or_else(|| ErrorData::invalid_request(crate::GATEWAY_ROUTING_REQUIRED, None))?;
        let calls = parts.extensions.get::<ServerCalls>().ok_or_else(|| {
            ErrorData::internal_error("this server was built without calls to other servers", None)
        })?;
        let identity = parts
            .extensions
            .get::<GatewayInternalIdentity>()
            .ok_or_else(|| ErrorData::invalid_request(crate::GATEWAY_ROUTING_REQUIRED, None))?;
        let bearer = parts
            .extensions
            .get::<ForwardedBearer>()
            .ok_or_else(|| ErrorData::invalid_request(crate::GATEWAY_ROUTING_REQUIRED, None))?;
        Ok(calls.for_request(identity, bearer))
    }
}
