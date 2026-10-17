//! The RFC 7523 §2.2 client assertion a registered server authenticates with at the
//! Hub's token endpoint, signed with the key pinned to its namespace.
//!
//! `iss` and `sub` name the registration's client identifier, `aud` the token endpoint
//! URL, `jti` is a UUIDv7, and the assertion lives at most 60 seconds. The Hub
//! records each `jti` until the assertion expires, so an assertion authenticates once.
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use veoveo_types::OAuthClientId;

use crate::JwtId;
use crate::registration::{
    ServerPublicKey,
    statement::{CLOCK_SKEW, MAX_STATEMENT_LIFETIME, ServerKeyPair, StatementError},
};

/// RFC 7523 §2.2 `client_assertion_type`.
pub const CLIENT_ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssertionClaims {
    iss: OAuthClientId,
    sub: OAuthClientId,
    aud: String,
    iat: i64,
    exp: i64,
    jti: String,
}

/// A verified assertion: the client it authenticates and the `jti` the replay guard
/// records until `expires_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedServerAssertion {
    pub client: OAuthClientId,
    pub jwt_id: JwtId,
    pub expires_at: DateTime<Utc>,
}

impl ServerKeyPair {
    /// Sign an assertion that authenticates `client` at `token_endpoint`.
    pub fn client_assertion(
        &self,
        client: &OAuthClientId,
        token_endpoint: &url::Url,
        now: DateTime<Utc>,
    ) -> Result<SecretString, StatementError> {
        ensure_crypto_provider();
        let claims = AssertionClaims {
            iss: client.clone(),
            sub: client.clone(),
            aud: token_endpoint.as_str().to_owned(),
            iat: now.timestamp(),
            exp: (now + MAX_STATEMENT_LIFETIME).timestamp(),
            jti: uuid::Uuid::now_v7().to_string(),
        };
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some("JWT".into());
        header.kid = Some(self.public_key().thumbprint().into());
        jsonwebtoken::encode(
            &header,
            &claims,
            &EncodingKey::from_ed_der(self.pkcs8_der()),
        )
        .map(SecretString::from)
        .map_err(|_| StatementError::Signing)
    }
}

/// The client an assertion names in `sub`, before verification, to find its key.
pub fn asserted_client(assertion: &str) -> Option<OAuthClientId> {
    let claims = jsonwebtoken::dangerous::insecure_decode::<AssertionClaims>(assertion).ok()?;
    (claims.claims.iss == claims.claims.sub).then_some(claims.claims.sub)
}

/// Verify an assertion for `client`, signed by `key`, addressed to `token_endpoint`.
pub fn verify_client_assertion(
    assertion: &str,
    client: &OAuthClientId,
    key: &ServerPublicKey,
    token_endpoint: &url::Url,
    now: DateTime<Utc>,
) -> Result<VerifiedServerAssertion, StatementError> {
    ensure_crypto_provider();
    let header =
        jsonwebtoken::decode_header(assertion).map_err(|_| StatementError::InvalidSignature)?;
    let thumbprint = key.thumbprint();
    if header.alg != Algorithm::EdDSA || header.kid.as_deref() != Some(thumbprint.as_str()) {
        return Err(StatementError::Rejected(
            "header must name the registered key",
        ));
    }
    let decoding =
        DecodingKey::from_ed_components(&key.x()).map_err(|_| StatementError::InvalidSignature)?;
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(&[token_endpoint.as_str()]);
    validation.set_issuer(&[client.as_str()]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 0;
    validation.validate_exp = false;
    let claims = jsonwebtoken::decode::<AssertionClaims>(assertion, &decoding, &validation)
        .map_err(|_| StatementError::InvalidSignature)?
        .claims;
    if &claims.sub != client {
        return Err(StatementError::Rejected("subject is not the client"));
    }
    let issued = DateTime::from_timestamp(claims.iat, 0)
        .ok_or(StatementError::Rejected("has an invalid issue time"))?;
    let expires_at = DateTime::from_timestamp(claims.exp, 0)
        .ok_or(StatementError::Rejected("has an invalid expiry"))?;
    if issued > now + CLOCK_SKEW {
        return Err(StatementError::Rejected("was issued in the future"));
    }
    if expires_at <= now - CLOCK_SKEW {
        return Err(StatementError::Rejected("has expired"));
    }
    if expires_at - issued > MAX_STATEMENT_LIFETIME {
        return Err(StatementError::Rejected("lives longer than 60 seconds"));
    }
    let jwt_id = JwtId::parse(claims.jti.clone())
        .map_err(|_| StatementError::Rejected("jti must be a UUIDv7"))?;
    if uuid::Uuid::parse_str(&claims.jti)
        .ok()
        .is_none_or(|jti| jti.get_version_num() != 7)
    {
        return Err(StatementError::Rejected("jti must be a UUIDv7"));
    }
    Ok(VerifiedServerAssertion {
        client: claims.sub,
        jwt_id,
        expires_at,
    })
}

fn ensure_crypto_provider() {
    let _ = jsonwebtoken::crypto::aws_lc::DEFAULT_PROVIDER.install_default();
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use secrecy::ExposeSecret;

    use super::*;

    fn endpoint() -> url::Url {
        url::Url::parse("https://hub.example/oauth/token").unwrap()
    }

    #[test]
    fn assertions_authenticate_the_registration_once_at_the_token_endpoint() {
        let key = ServerKeyPair::generate().unwrap();
        let client = OAuthClientId::parse("media-0123456789abcd").unwrap();
        let now = Utc::now();
        let assertion = key.client_assertion(&client, &endpoint(), now).unwrap();
        assert_eq!(
            asserted_client(assertion.expose_secret()),
            Some(client.clone())
        );
        let verified = verify_client_assertion(
            assertion.expose_secret(),
            &client,
            key.public_key(),
            &endpoint(),
            now,
        )
        .unwrap();
        assert_eq!(verified.client, client);
        assert!(verified.expires_at <= now + MAX_STATEMENT_LIFETIME);

        let other = ServerKeyPair::generate().unwrap();
        assert!(
            verify_client_assertion(
                assertion.expose_secret(),
                &client,
                other.public_key(),
                &endpoint(),
                now
            )
            .is_err()
        );
        let elsewhere = url::Url::parse("https://other.example/oauth/token").unwrap();
        assert!(
            verify_client_assertion(
                assertion.expose_secret(),
                &client,
                key.public_key(),
                &elsewhere,
                now
            )
            .is_err()
        );
        let impostor = OAuthClientId::parse("time-0123456789abcd").unwrap();
        assert!(
            verify_client_assertion(
                assertion.expose_secret(),
                &impostor,
                key.public_key(),
                &endpoint(),
                now
            )
            .is_err()
        );
        let later = now + TimeDelta::seconds(120);
        assert!(
            verify_client_assertion(
                assertion.expose_secret(),
                &client,
                key.public_key(),
                &endpoint(),
                later
            )
            .is_err()
        );
    }
}
