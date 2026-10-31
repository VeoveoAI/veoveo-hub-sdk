//! The registration client a server runs. It keeps the server's key in a directory,
//! prepares metadata offline and resumes private RFC 7592 registration state across
//! process restarts. Initial RFC 7591 admission is used only for an explicit new
//! registration; stopping the process does not retire its deployment.
//!
//! The caller supplies the `reqwest::Client`. A client that reaches the Hub over
//! HTTPS needs a process-wide rustls crypto provider installed first.
use std::{
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Utc;
use reqwest::{
    Method, StatusCode,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};
use secrecy::{ExposeSecret, SecretString};
use veoveo_types::OAuthClientId;

use crate::registration::{
    AccessTokenTrust, AppOrigin, AuthorizationDeclaration, AuthorizationServerMetadata,
    ClientInformation, ClientMetadata, ClientRegistrationRequest, ClientUpdateRequest,
    DatabaseGrant, DeclarationError, PublicBase, RegistrationDeclaration,
    RegistrationErrorResponse,
    statement::{ServerKeyPair, StatementError},
};

/// The file inside a [`KeyDirectory`] that holds the server's PKCS#8 private key.
pub const KEY_FILE: &str = "server-key.p8";
/// Standard private RFC 7592 ClientInformation, including its original database grant.
pub const REGISTRATION_FILE: &str = "registration.json";
/// The longest the client waits for one Hub response.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("server key file {path}: {source}")]
    KeyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("registration state file {path}: {source}")]
    RegistrationFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("stored registration state is invalid: {0}")]
    InvalidStoredRegistration(serde_json::Error),
    #[error(transparent)]
    Key(#[from] StatementError),
    #[error(transparent)]
    Metadata(#[from] DeclarationError),
    #[error("registration request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("the Hub answered HTTP {status}{}", describe(.error))]
    Rejected {
        status: StatusCode,
        error: Option<RegistrationErrorResponse>,
    },
    #[error("the Hub returned an invalid registration response: {0}")]
    InvalidResponse(serde_json::Error),
    #[error("the registration response {0}")]
    ResponseMismatch(&'static str),
}

fn describe(error: &Option<RegistrationErrorResponse>) -> String {
    match error {
        Some(RegistrationErrorResponse {
            error,
            error_description: Some(description),
        }) => format!(": {error}: {description}"),
        Some(RegistrationErrorResponse { error, .. }) => format!(": {error}"),
        None => String::new(),
    }
}

impl ClientError {
    /// Whether a later attempt can succeed without a change to the server.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Rejected { status, .. } => status.is_server_error(),
            _ => false,
        }
    }
}

/// A directory that holds the server's private key across restarts.
#[derive(Debug, Clone)]
pub struct KeyDirectory(PathBuf);

impl KeyDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn key_path(&self) -> PathBuf {
        self.0.join(KEY_FILE)
    }

    pub fn registration_path(&self) -> PathBuf {
        self.0.join(REGISTRATION_FILE)
    }

    /// Describe a candidate using its durable worker key, before admission. This
    /// performs no network, database or registration operation and needs no token.
    pub fn prepare(
        &self,
        declaration: RegistrationDeclaration,
        authorization: AuthorizationDeclaration,
    ) -> Result<ClientMetadata, ClientError> {
        let key = self.load_or_generate()?;
        Ok(ClientMetadata::new(
            *key.public_key(),
            declaration,
            authorization,
        )?)
    }

    fn load_registration(&self) -> Result<Option<ClientInformation>, ClientError> {
        let path = self.registration_path();
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(ClientError::InvalidStoredRegistration),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ClientError::RegistrationFile { path, source }),
        }
    }

    fn store_registration(&self, registration: &Registration) -> Result<(), ClientError> {
        let path = self.registration_path();
        let write = || -> std::io::Result<()> {
            create_private_dir(&self.0)?;
            let staged = self
                .0
                .join(format!(".{REGISTRATION_FILE}.{}", uuid::Uuid::new_v4()));
            let mut file = private_file(&staged)?;
            // Persist native ClientInformation, not a second credential envelope.
            let bytes =
                serde_json::to_vec(&registration.information).map_err(std::io::Error::other)?;
            let result = (|| {
                file.write_all(&bytes)?;
                file.sync_all()?;
                std::fs::rename(&staged, &path)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(staged);
            }
            result
        };
        write().map_err(|source| ClientError::RegistrationFile { path, source })
    }

    /// Load the server's key, generating and storing one on first start. The file is
    /// readable only by its owner and appears atomically.
    pub fn load_or_generate(&self) -> Result<ServerKeyPair, ClientError> {
        let path = self.key_path();
        match std::fs::read(&path) {
            Ok(der) => Ok(ServerKeyPair::from_pkcs8_der(&der)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let key = ServerKeyPair::generate()?;
                self.store(&key).map_err(|source| ClientError::KeyFile {
                    path: path.clone(),
                    source,
                })?;
                Ok(key)
            }
            Err(source) => Err(ClientError::KeyFile { path, source }),
        }
    }

    fn store(&self, key: &ServerKeyPair) -> std::io::Result<()> {
        create_private_dir(&self.0)?;
        let staged = self.0.join(format!(".{KEY_FILE}.{}", uuid::Uuid::new_v4()));
        let mut file = private_file(&staged)?;
        file.write_all(key.pkcs8_der())?;
        file.sync_all()?;
        std::fs::rename(&staged, self.key_path())
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}
#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(unix)]
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
#[cfg(not(unix))]
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// An admitted registration and what the Hub issued with it.
#[derive(Debug, Clone)]
pub struct Registration {
    information: ClientInformation,
}

impl Registration {
    pub fn client_id(&self) -> &OAuthClientId {
        &self.information.client_id
    }
    /// The issuer and keys that verify the access tokens the Hub forwards.
    pub fn access_tokens(&self) -> &AccessTokenTrust {
        &self.information.access_tokens
    }
    /// The server's database user, kept from the response that last carried it.
    pub fn database(&self) -> Option<&DatabaseGrant> {
        self.information.database.as_ref()
    }
    /// The public URL of this instance's mount, under which the server builds every
    /// public URL it hands out, when it declares a public route.
    pub fn public_base(&self) -> Option<&PublicBase> {
        self.information.public_base.as_ref()
    }
    /// The server's app origin, when it declares one.
    pub fn app_origin(&self) -> Option<&AppOrigin> {
        self.information.app_origin.as_ref()
    }
    pub fn information(&self) -> &ClientInformation {
        &self.information
    }
}

/// Registers one server with one Hub.
pub struct RegistrationClient {
    http: reqwest::Client,
    endpoint: url::Url,
    initial_access_token: SecretString,
    key: ServerKeyPair,
    metadata: ClientMetadata,
    request_timeout: Duration,
}

impl std::fmt::Debug for RegistrationClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationClient")
            .field("endpoint", &self.endpoint.as_str())
            .field("server", self.metadata.slug())
            .finish_non_exhaustive()
    }
}

impl RegistrationClient {
    /// `endpoint` is the Hub's RFC 7591 registration endpoint, and
    /// `initial_access_token` the installation's registration secret, or the
    /// placeholder an environment substitutes for it.
    pub fn new(
        http: reqwest::Client,
        endpoint: url::Url,
        initial_access_token: SecretString,
        key: ServerKeyPair,
        metadata: ClientMetadata,
    ) -> Result<Self, ClientError> {
        crate::registration::declaration::http_url(&endpoint, "registration_endpoint")?;
        metadata.check()?;
        if metadata.key() != key.public_key() {
            return Err(ClientError::Key(StatementError::Rejected(
                "metadata names another key than the signer",
            )));
        }
        Ok(Self {
            http,
            endpoint,
            initial_access_token,
            key,
            metadata,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        })
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Read the registration endpoint from the Hub's RFC 8414 metadata.
    pub async fn discover(
        http: &reqwest::Client,
        issuer: &url::Url,
    ) -> Result<url::Url, ClientError> {
        let response = http
            .get(AuthorizationServerMetadata::url_for(issuer))
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
        let metadata: AuthorizationServerMetadata =
            serde_json::from_slice(&body).map_err(ClientError::InvalidResponse)?;
        if &metadata.issuer != issuer {
            return Err(ClientError::ResponseMismatch(
                "names another issuer than the one discovered",
            ));
        }
        metadata
            .registration_endpoint
            .ok_or(ClientError::ResponseMismatch(
                "advertises no registration endpoint",
            ))
    }

    pub fn metadata(&self) -> &ClientMetadata {
        &self.metadata
    }

    /// Resume the saved RFC 7592 registration while it still describes this server and
    /// the Hub still knows it. Otherwise register again with the initial access token
    /// and replace the saved state, so a changed declaration, a lost registration or
    /// unreadable state never stops the server.
    pub async fn resume_or_register(
        &self,
        directory: &KeyDirectory,
    ) -> Result<Registration, ClientError> {
        self.check_directory_key(directory)?;
        let saved = match directory.load_registration() {
            Ok(information) => {
                information.and_then(|information| self.admit(information, None).ok())
            }
            Err(ClientError::InvalidStoredRegistration(_)) => None,
            Err(error) => return Err(error),
        };
        let Some(stored) = saved else {
            return self.register_and_store(directory).await;
        };
        let registration = match self.read(&stored).await {
            Err(ClientError::Rejected {
                status: StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND,
                ..
            }) => return self.register_and_store(directory).await,
            result => result?,
        };
        directory.store_registration(&registration)?;
        Ok(registration)
    }

    /// Register with the initial access token and replace any saved state.
    pub async fn register_and_store(
        &self,
        directory: &KeyDirectory,
    ) -> Result<Registration, ClientError> {
        self.check_directory_key(directory)?;
        let registration = self.register().await?;
        directory.store_registration(&registration)?;
        Ok(registration)
    }

    fn check_directory_key(&self, directory: &KeyDirectory) -> Result<(), ClientError> {
        if directory.load_or_generate()?.public_key() != self.key.public_key() {
            return Err(ClientError::ResponseMismatch(
                "registration state directory belongs to another worker key",
            ));
        }
        Ok(())
    }

    fn check_registration_authority(&self, registration: &Registration) -> Result<(), ClientError> {
        if registration.information.metadata != self.metadata {
            return Err(ClientError::ResponseMismatch(
                "registration state names another deployment or declaration",
            ));
        }
        if registration.information.registration_client_uri.origin() != self.endpoint.origin() {
            return Err(ClientError::ResponseMismatch(
                "saved registration belongs to another Hub authority",
            ));
        }
        Ok(())
    }

    /// RFC 7591 registration.
    pub async fn register(&self) -> Result<Registration, ClientError> {
        let request = ClientRegistrationRequest {
            metadata: self.metadata.clone(),
            software_statement: self.key.sign_statement(
                &self.metadata,
                &self.endpoint,
                Utc::now(),
            )?,
        };
        let information = self
            .exchange(
                Method::POST,
                &self.endpoint,
                self.initial_access_token.expose_secret(),
                Some(serde_json::to_vec(&request).expect("request serializes")),
                StatusCode::CREATED,
            )
            .await?;
        self.admit(information, None)
    }

    /// RFC 7592 read.
    pub async fn read(&self, registration: &Registration) -> Result<Registration, ClientError> {
        self.check_registration_authority(registration)?;
        let information = self
            .exchange(
                Method::GET,
                &registration.information.registration_client_uri,
                registration
                    .information
                    .registration_access_token
                    .expose_secret(),
                None,
                StatusCode::OK,
            )
            .await?;
        self.admit(information, Some(registration))
    }

    /// RFC 7592 update: replace the registered metadata with this client's.
    pub async fn update(&self, registration: &mut Registration) -> Result<(), ClientError> {
        self.check_registration_authority(registration)?;
        let uri = registration.information.registration_client_uri.clone();
        let request = ClientUpdateRequest {
            client_id: registration.information.client_id.clone(),
            metadata: self.metadata.clone(),
            software_statement: self.key.sign_statement(&self.metadata, &uri, Utc::now())?,
        };
        let information = self
            .exchange(
                Method::PUT,
                &uri,
                registration
                    .information
                    .registration_access_token
                    .expose_secret(),
                Some(serde_json::to_vec(&request).expect("request serializes")),
                StatusCode::OK,
            )
            .await?;
        *registration = self.admit(information, Some(registration))?;
        Ok(())
    }

    /// RFC 7592 delete.
    pub async fn deregister(&self, registration: Registration) -> Result<(), ClientError> {
        self.check_registration_authority(&registration)?;
        let response = self
            .send(
                Method::DELETE,
                &registration.information.registration_client_uri,
                registration
                    .information
                    .registration_access_token
                    .expose_secret(),
                None,
            )
            .await?;
        match response.status() {
            StatusCode::NO_CONTENT | StatusCode::UNAUTHORIZED => Ok(()),
            status => Err(rejected(status, &response.bytes().await?)),
        }
    }

    /// Wait for process shutdown without deleting restart credentials. The Hub keeps
    /// the registration while its login is live and applies its normal orphan grace.
    /// Explicit retirement calls `deregister`; restarting uses `resume_or_register`.
    pub async fn maintain(
        &self,
        _registration: Registration,
        shutdown: impl Future<Output = ()>,
    ) -> Result<(), ClientError> {
        shutdown.await;
        Ok(())
    }

    async fn exchange(
        &self,
        method: Method,
        url: &url::Url,
        bearer: &str,
        body: Option<Vec<u8>>,
        expected: StatusCode,
    ) -> Result<ClientInformation, ClientError> {
        let response = self.send(method, url, bearer, body).await?;
        let status = response.status();
        let body = response.bytes().await?;
        if status != expected {
            return Err(rejected(status, &body));
        }
        serde_json::from_slice(&body).map_err(ClientError::InvalidResponse)
    }

    async fn send(
        &self,
        method: Method,
        url: &url::Url,
        bearer: &str,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response, ClientError> {
        let mut request = self
            .http
            .request(method, url.clone())
            .header(AUTHORIZATION, format!("Bearer {bearer}"))
            .header(ACCEPT, "application/json")
            .timeout(self.request_timeout);
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        Ok(request.send().await?)
    }

    /// The Hub registers the metadata unchanged or rejects it.
    fn admit(
        &self,
        mut information: ClientInformation,
        previous: Option<&Registration>,
    ) -> Result<Registration, ClientError> {
        information.check()?;
        if information.metadata != self.metadata {
            return Err(ClientError::ResponseMismatch(
                "registered other metadata than the server sent",
            ));
        }
        if let Some(previous) = previous {
            if information.client_id != previous.information.client_id
                || information.registration_client_uri
                    != previous.information.registration_client_uri
            {
                return Err(ClientError::ResponseMismatch(
                    "read/update changed the registered client identity or configuration URI",
                ));
            }
            if information.database.is_none() {
                information.database = previous.information.database.clone();
            }
        }
        information.check()?;
        let registration = Registration { information };
        self.check_registration_authority(&registration)?;
        Ok(registration)
    }
}

fn rejected(status: StatusCode, body: &[u8]) -> ClientError {
    ClientError::Rejected {
        status,
        error: serde_json::from_slice(body).ok(),
    }
}
