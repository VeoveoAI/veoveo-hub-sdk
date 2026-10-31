//! Wire format, schema and admission checks for the registration declarations and grants.
use serde_json::json;
use veoveo_hub_sdk::identity::{LocalToolName, PromptName};
use veoveo_hub_sdk::registration::*;
use veoveo_types::{PermissionName, ResourceScheme, ResourceTemplateUri, ResourceUri, ServerSlug};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
enum ClockPermission {
    #[vocabulary(rename = "clock.read")]
    Read,
    #[vocabulary(rename = "clock.events.write")]
    EventsWrite,
    #[vocabulary(rename = "clock.admin")]
    Admin,
}

impl PermissionDefinition for ClockPermission {
    fn resource(self) -> &'static str {
        match self {
            Self::Read | Self::Admin => "clock.authority",
            Self::EventsWrite => "clock.event",
        }
    }
    fn access(self) -> Access {
        match self {
            Self::Read => Access::Read,
            Self::EventsWrite | Self::Admin => Access::Write,
        }
    }
    fn risks(self) -> &'static [Risk] {
        match self {
            Self::Admin => &[Risk::Administer],
            _ => &[],
        }
    }
}

fn slug() -> ServerSlug {
    ServerSlug::parse("clock").unwrap()
}
fn scheme() -> ResourceScheme {
    ResourceScheme::parse("clock").unwrap()
}
fn tool(name: &str) -> LocalToolName {
    LocalToolName::parse(name).unwrap()
}
fn template(value: &str) -> ResourceTemplateUri {
    ResourceTemplateUri::new(value).unwrap()
}

// Direct wire fixtures exercise the registration trust boundary. Hosted authors
// construct these declarations from their ready Routes table.
fn authorization() -> AuthorizationDeclaration {
    let permissions = [
        ClockPermission::Read,
        ClockPermission::EventsWrite,
        ClockPermission::Admin,
    ]
    .into_iter()
    .map(|permission| PermissionDeclaration {
        permission: PermissionName::parse(permission.as_str()).unwrap(),
        resource: veoveo_types::ResourceTypeName::parse(permission.resource()).unwrap(),
        access: permission.access(),
        risks: permission.risks().iter().copied().collect(),
    })
    .collect();
    AuthorizationDeclaration {
        permissions,
        requirements: Requirements {
            tools: [
                (tool("resolve_time"), policy(ClockPermission::Read)),
                (tool("create_event"), policy(ClockPermission::EventsWrite)),
                (tool("activate_release"), policy(ClockPermission::Admin)),
            ]
            .into(),
            resources: [(
                ResourceUri::new("clock://authorities/current").unwrap(),
                policy(ClockPermission::Read),
            )]
            .into(),
            resource_templates: [
                (
                    template("clock://events/{event}"),
                    policy(ClockPermission::Read),
                ),
                (template("ui://clock/{view}"), policy(ClockPermission::Read)),
            ]
            .into(),
            prompts: [(
                PromptName::parse("plan_window").unwrap(),
                policy(ClockPermission::Read),
            )]
            .into(),
            app_origin: None,
        },
        uses: Vec::new(),
        calls_hub: false,
    }
}
fn policy(permission: ClockPermission) -> RouteAccess {
    RouteAccess::Permission(PermissionName::parse(permission.as_str()).unwrap())
}

#[test]
fn authorization_declaration_wire_format() {
    let wire = serde_json::to_value(authorization()).unwrap();
    assert_eq!(
        wire,
        json!({
            "permissions": [
                {"permission": "clock.read", "resource": "clock.authority", "access": "read"},
                {"permission": "clock.events.write", "resource": "clock.event", "access": "write"},
                {"permission": "clock.admin", "resource": "clock.authority", "access": "write", "risks": ["administer"]}
            ],
            "requirements": {
                "tools": {
                    "activate_release": {"kind":"permission","permission":"clock.admin"},
                    "create_event": {"kind":"permission","permission":"clock.events.write"},
                    "resolve_time": {"kind":"permission","permission":"clock.read"}
                },
                "resources": {"clock://authorities/current": {"kind":"permission","permission":"clock.read"}},
                "resourceTemplates": {
                    "clock://events/{event}": {"kind":"permission","permission":"clock.read"},
                    "ui://clock/{view}": {"kind":"permission","permission":"clock.read"}
                },
                "prompts": {"plan_window": {"kind":"permission","permission":"clock.read"}}
            }
        })
    );
    let back: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert_eq!(back, authorization());
    back.check(&slug(), &scheme()).unwrap();
}

#[test]
fn required_permission_lookup() {
    let declaration = authorization();
    let required = |target| {
        declaration
            .access(target)
            .and_then(RouteAccess::permission)
            .map(|permission| permission.as_str().to_owned())
    };
    let create_event = tool("create_event");
    assert_eq!(
        required(RequirementTarget::Tool(&create_event)).as_deref(),
        Some("clock.events.write")
    );
    let event = ResourceUri::new("clock://events/abc").unwrap();
    assert_eq!(
        required(RequirementTarget::Resource(&event)).as_deref(),
        Some("clock.read")
    );
    let unknown = ResourceUri::new("clock://elsewhere/abc").unwrap();
    assert_eq!(required(RequirementTarget::Resource(&unknown)), None);
    let undeclared = tool("undeclared");
    assert_eq!(required(RequirementTarget::Tool(&undeclared)), None);
}

#[test]
fn ambiguous_template_matches_require_nothing() {
    let mut declaration = authorization();
    declaration.requirements.resource_templates.insert(
        template("clock://events/{+tail}"),
        policy(ClockPermission::EventsWrite),
    );
    declaration.check(&slug(), &scheme()).unwrap();
    let event = ResourceUri::new("clock://events/abc").unwrap();
    assert!(
        declaration
            .access(RequirementTarget::Resource(&event))
            .is_none()
    );
}

#[test]
fn authorization_matches_native_collection_queries_and_member_paths() {
    let mut declaration = authorization();
    declaration.requirements.resource_templates = [
        (
            template("clock://events{?role,state,limit,cursor}"),
            policy(ClockPermission::Read),
        ),
        (
            template("clock://events/{event}"),
            policy(ClockPermission::EventsWrite),
        ),
        (
            template("clock://model/{+model_id}"),
            policy(ClockPermission::Read),
        ),
    ]
    .into();
    declaration.check(&slug(), &scheme()).unwrap();
    for uri in [
        "clock://events",
        "clock://events?limit=2&cursor=opaque",
        "clock://events?cursor=opaque&state=pending&limit=2&role=decider",
        "clock://events?%63ursor=opaque",
        "clock://model/owner/model/edit",
        "clock://model",
    ] {
        let uri = ResourceUri::new(uri).unwrap();
        assert_eq!(
            declaration.access(RequirementTarget::Resource(&uri)),
            Some(&RouteAccess::Permission(
                PermissionName::parse("clock.read").unwrap()
            )),
            "{uri}"
        );
    }
    let member = ResourceUri::new("clock://events/one").unwrap();
    assert_eq!(
        declaration.access(RequirementTarget::Resource(&member)),
        Some(&RouteAccess::Permission(
            PermissionName::parse("clock.events.write").unwrap()
        ))
    );
    for uri in [
        "clock://events?unknown=1",
        "clock://events?cursor=a&cursor=b",
        "clock://events?cursor=a&%63ursor=b",
        "clock://events?",
        "clock://events/one/nested",
        "clock://events/one?cursor=opaque",
    ] {
        let uri = ResourceUri::new(uri).unwrap();
        assert!(
            declaration
                .access(RequirementTarget::Resource(&uri))
                .is_none(),
            "{uri}"
        );
    }
}

#[test]
fn authorization_refuses_templates_outside_the_native_route_profile() {
    for uri in [
        "clock://{authority}/{id}",
        "clock://events/prefix-{id}",
        "clock://events/{+tail}/member",
        "clock://events/{id:2}",
        "clock://events/{id*}",
        "clock://events{&cursor}",
        "clock://events?cursor={cursor}",
        "clock://events{?cursor,cursor}",
        "clock://events/{id}{?id}",
        "clock://events//",
    ] {
        let mut declaration = authorization();
        declaration
            .requirements
            .resource_templates
            .insert(template(uri), policy(ClockPermission::Read));
        let result = declaration.check(&slug(), &scheme());
        assert!(
            matches!(result, Err(DeclarationError::UnsupportedTemplate { .. })),
            "{uri}: {result:?}"
        );
    }
}

#[test]
fn declarations_outside_the_namespace_are_rejected() {
    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["permissions"][0]["permission"] = json!("map.read");
    wire["requirements"]["tools"]["resolve_time"] =
        json!({"kind":"permission","permission":"map.read"});
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::PermissionOutsideNamespace { .. })
    ));

    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["permissions"][0]["resource"] = json!("map.layer");
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::ResourceOutsideNamespace { .. })
    ));

    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["requirements"]["resources"] =
        json!({"map://layers/a": {"kind":"permission","permission":"clock.read"}});
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::ResourceOutsideScheme { .. })
    ));

    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["requirements"]["tools"]["resolve_time"] =
        json!({"kind":"permission","permission":"clock.undeclared"});
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::UndeclaredPermission { .. })
    ));

    let mut wire = serde_json::to_value(authorization()).unwrap();
    let first = wire["permissions"][0].clone();
    wire["permissions"].as_array_mut().unwrap().push(first);
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::DuplicatePermission { .. })
    ));

    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["requirements"]["prompts"]["plan_window"] =
        json!({"kind":"permission","permission":"clock.events.write"});
    let declaration: AuthorizationDeclaration = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        declaration.check(&slug(), &scheme()),
        Err(DeclarationError::PromptRequiresWrite { .. })
    ));
}

#[test]
fn wire_admission_rejects_unknown_vocabulary() {
    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["permissions"][0]["access"] = json!("execute");
    assert!(serde_json::from_value::<AuthorizationDeclaration>(wire).is_err());
    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["permissions"][2]["risks"] = json!(["radioactive"]);
    assert!(serde_json::from_value::<AuthorizationDeclaration>(wire).is_err());
    let mut wire = serde_json::to_value(authorization()).unwrap();
    wire["roles"] = json!({});
    assert!(serde_json::from_value::<AuthorizationDeclaration>(wire).is_err());
}

#[test]
fn uses_name_other_servers_permissions() {
    let mut declaration = authorization();
    declaration.uses = ["map.layers.read", "map.layers.write"]
        .into_iter()
        .map(|name| PermissionName::parse(name).unwrap())
        .collect();
    declaration.check(&slug(), &scheme()).unwrap();
    let wire = serde_json::to_value(&declaration).unwrap();
    assert_eq!(wire["uses"], json!(["map.layers.read", "map.layers.write"]));
    let map = ServerSlug::parse("map").unwrap();
    assert_eq!(
        declaration
            .uses_of(&map)
            .map(PermissionName::as_str)
            .collect::<Vec<_>>(),
        ["map.layers.read", "map.layers.write"]
    );
    assert_eq!(
        declaration
            .uses_of(&ServerSlug::parse("time").unwrap())
            .count(),
        0
    );
    // A declaration without uses omits the member, so existing documents stay as
    // they are.
    assert!(
        serde_json::to_value(authorization())
            .unwrap()
            .get("uses")
            .is_none()
    );

    let mut own = wire.clone();
    own["uses"] = json!(["clock.read"]);
    let own: AuthorizationDeclaration = serde_json::from_value(own).unwrap();
    assert!(matches!(
        own.check(&slug(), &scheme()),
        Err(DeclarationError::UseInsideNamespace { .. })
    ));
    let mut repeated = wire.clone();
    repeated["uses"] = json!(["map.layers.read", "map.layers.read"]);
    let repeated: AuthorizationDeclaration = serde_json::from_value(repeated).unwrap();
    assert!(matches!(
        repeated.check(&slug(), &scheme()),
        Err(DeclarationError::DuplicateUse { .. })
    ));
    let mut malformed = wire;
    malformed["uses"] = json!(["map"]);
    assert!(serde_json::from_value::<AuthorizationDeclaration>(malformed).is_err());

    // RFC 7591 §2: a server that uses others registers the token exchange grant, and
    // one that does not registers none.
    let consumer = ClientMetadata::new(rfc8037_key(), registration(), declaration).unwrap();
    assert_eq!(consumer.grant_types, [GrantType::TokenExchange]);
    assert_eq!(
        serde_json::to_value(&consumer).unwrap()["grant_types"],
        json!(["urn:ietf:params:oauth:grant-type:token-exchange"])
    );
    assert!(metadata().grant_types.is_empty());
    let mut ungranted = consumer.clone();
    ungranted.grant_types.clear();
    assert!(ungranted.check().is_err());
}

#[test]
fn a_server_that_calls_the_hub_registers_the_token_exchange_grant() {
    let mut declaration = authorization();
    declaration.calls_hub = true;
    declaration.check(&slug(), &scheme()).unwrap();
    assert_eq!(
        serde_json::to_value(&declaration).unwrap()["callsHub"],
        json!(true)
    );
    // A declaration that does not call the Hub omits the member.
    assert!(
        serde_json::to_value(authorization())
            .unwrap()
            .get("callsHub")
            .is_none()
    );

    // Without uses, calling the Hub alone registers the grant, in both spellings.
    let caller = ClientMetadata::new(rfc8037_key(), registration(), declaration).unwrap();
    assert_eq!(caller.grant_types, [GrantType::TokenExchange]);
    let wire = serde_json::to_value(&caller).unwrap();
    assert_eq!(wire["veoveo_authorization"]["calls_hub"], json!(true));
    assert_eq!(
        wire["grant_types"],
        json!(["urn:ietf:params:oauth:grant-type:token-exchange"])
    );
    let decoded: ClientMetadata = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, caller);
    decoded.check().unwrap();
    let mut ungranted = caller;
    ungranted.grant_types.clear();
    assert!(ungranted.check().is_err());
}

fn registration() -> RegistrationDeclaration {
    RegistrationDeclaration {
        deployment: veoveo_hub_sdk::hosting::testing::deployment_id(),
        slug: slug(),
        uri_scheme: scheme(),
        version: ServerVersion::new("0.1.0").unwrap(),
        endpoint: ServerEndpoint::new(url::Url::parse("https://clock.example/clock/mcp").unwrap())
            .unwrap(),
        surfaces: None,
    }
}

#[test]
fn registration_declaration_wire_format() {
    let wire = serde_json::to_value(registration()).unwrap();
    assert_eq!(
        wire,
        json!({
            "slug": "clock",
            "deployment": veoveo_hub_sdk::hosting::testing::deployment_id(),
            "uriScheme": "clock",
            "version": "0.1.0",
            "endpoint": "https://clock.example/clock/mcp"
        })
    );
    assert_eq!(
        serde_json::from_value::<RegistrationDeclaration>(wire).unwrap(),
        registration()
    );
}

#[test]
fn registration_values_are_admitted_once() {
    let mut wire = serde_json::to_value(registration()).unwrap();
    for (field, value) in [
        ("endpoint", json!("ftp://clock/mcp")),
        ("endpoint", json!("https://user:secret@clock.example/mcp")),
        ("endpoint", json!("http://clock.example/mcp")),
        ("endpoint", json!("https://clock.example/mcp?token=x")),
        ("version", json!("")),
        ("version", json!("1 0")),
        ("slug", json!("Clock")),
    ] {
        let original = wire[field].clone();
        wire[field] = value;
        assert!(
            serde_json::from_value::<RegistrationDeclaration>(wire.clone()).is_err(),
            "{field}"
        );
        wire[field] = original;
    }
    wire["publicKey"] = json!({});
    assert!(serde_json::from_value::<RegistrationDeclaration>(wire).is_err());
}

/// RFC 8037 Appendix A.3: the thumbprint of the example Ed25519 key.
const RFC8037_X: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
const RFC8037_THUMBPRINT: &str = "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

fn rfc8037_key() -> ServerPublicKey {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(RFC8037_X)
        .unwrap();
    ServerPublicKey::from_bytes(bytes.try_into().unwrap())
}

#[test]
fn server_keys_are_jwks_identified_by_their_thumbprint() {
    let key = rfc8037_key();
    assert_eq!(key.thumbprint().as_str(), RFC8037_THUMBPRINT);
    let wire = serde_json::to_value(ServerKeySet::new(key)).unwrap();
    assert_eq!(
        wire,
        json!({"keys": [{
            "kty": "OKP", "crv": "Ed25519", "x": RFC8037_X,
            "kid": RFC8037_THUMBPRINT, "alg": "EdDSA", "use": "sig"
        }]})
    );
    assert_eq!(
        serde_json::from_value::<ServerKeySet>(wire.clone())
            .unwrap()
            .key(),
        &key
    );
    let jwk = wire["keys"][0].clone();
    let with = |member: &str, value: serde_json::Value| {
        let mut jwk = jwk.clone();
        jwk[member] = value;
        json!({"keys": [jwk]})
    };
    for rejected in [
        json!({"keys": []}),
        json!({"keys": [jwk.clone(), jwk.clone()]}),
        with("kid", json!("not-the-thumbprint")),
        with("kty", json!("RSA")),
        with("crv", json!("X25519")),
        with("alg", json!("ES256")),
        with("use", json!("enc")),
        with("x", json!("Bwc")),
        with("d", json!("private")),
    ] {
        assert!(
            serde_json::from_value::<ServerKeySet>(rejected.clone()).is_err(),
            "{rejected}"
        );
    }
    // The Hub's key set rotates several keys, each named by its thumbprint.
    let next = ServerPublicKey::from_bytes([7; 32]);
    let rotation = json!({"keys": [
        serde_json::to_value(Ed25519Jwk::new(key)).unwrap(),
        serde_json::to_value(Ed25519Jwk::new(next)).unwrap()
    ]});
    let keys: Ed25519JwkSet = serde_json::from_value(rotation).unwrap();
    assert_eq!(keys.get(&next.thumbprint()), Some(&next));
    let duplicate = json!({"keys": [jwk.clone(), jwk.clone()]});
    assert!(serde_json::from_value::<Ed25519JwkSet>(duplicate).is_err());
    let mut named = jwk.clone();
    named["kid"] = json!("hub-1");
    assert!(serde_json::from_value::<Ed25519JwkSet>(json!({"keys": [named]})).is_err());
}

fn metadata() -> ClientMetadata {
    ClientMetadata::new(rfc8037_key(), registration(), authorization()).unwrap()
}

const STATEMENT: &str = "eyJhbGciOiJFZERTQSJ9.eyJpc3MiOiJjbG9jayJ9.c2lnbmF0dXJl";

fn registration_request() -> ClientRegistrationRequest {
    ClientRegistrationRequest {
        metadata: metadata(),
        software_statement: SoftwareStatement::new(STATEMENT).unwrap(),
    }
}

#[test]
fn registration_requests_are_rfc7591_client_metadata() {
    let wire = serde_json::to_value(registration_request()).unwrap();
    assert_eq!(
        wire["veoveo_deployment_id"],
        serde_json::to_value(registration().deployment).unwrap()
    );
    let mut missing_deployment = wire.clone();
    missing_deployment
        .as_object_mut()
        .unwrap()
        .remove("veoveo_deployment_id");
    assert!(serde_json::from_value::<ClientRegistrationRequest>(missing_deployment).is_err());
    assert_eq!(wire["token_endpoint_auth_method"], "private_key_jwt");
    assert_eq!(wire["grant_types"], json!([]));
    assert_eq!(wire["response_types"], json!([]));
    assert_eq!(wire["jwks"]["keys"][0]["kid"], RFC8037_THUMBPRINT);
    assert_eq!(wire["software_statement"], STATEMENT);
    assert_eq!(wire["software_id"], "clock");
    assert_eq!(wire["software_version"], "0.1.0");
    assert_eq!(wire["veoveo_uri_scheme"], "clock");
    assert_eq!(
        wire["veoveo_mcp_endpoint"],
        "https://clock.example/clock/mcp"
    );
    // OAuth documents spell members in snake_case; MCP keeps camelCase.
    let mut snake = serde_json::to_value(authorization()).unwrap();
    let templates = snake["requirements"]
        .as_object_mut()
        .unwrap()
        .remove("resourceTemplates")
        .unwrap();
    snake["requirements"]["resource_templates"] = templates;
    assert_eq!(wire["veoveo_authorization"], snake);
    for member in ["ai.veoveo/registration", "ai.veoveo/authorization"] {
        assert!(wire.get(member).is_none(), "{member}");
    }
    assert_eq!(
        serde_json::from_value::<ClientRegistrationRequest>(wire.clone()).unwrap(),
        registration_request()
    );

    // RFC 7591 §2: unknown client metadata is ignored.
    let mut extended = wire.clone();
    extended["client_name"] = json!("Clock");
    assert_eq!(
        serde_json::from_value::<ClientRegistrationRequest>(extended).unwrap(),
        registration_request()
    );
    // Veoveo members admit only their own fields.
    let mut extended = wire.clone();
    extended["veoveo_authorization"]["owner"] = json!("someone");
    assert!(serde_json::from_value::<ClientRegistrationRequest>(extended).is_err());
    let mut unsigned = wire;
    unsigned["software_statement"] = json!("not a jwt");
    assert!(serde_json::from_value::<ClientRegistrationRequest>(unsigned).is_err());
}

#[test]
fn client_metadata_checks_every_declaration() {
    metadata().check().unwrap();
    let mut granted = metadata();
    granted.grant_types = vec![GrantType::ClientCredentials];
    assert!(granted.check().is_err());

    let mut elsewhere = registration();
    elsewhere.slug = ServerSlug::parse("map").unwrap();
    assert!(ClientMetadata::new(rfc8037_key(), elsewhere, authorization()).is_err());

    // Kernel and PostgreSQL schemas are not server namespaces.
    for reserved in ["kernel", "identity", "public", "pg-stat"] {
        let mut registration = registration();
        registration.slug = ServerSlug::parse(reserved).unwrap();
        registration.uri_scheme = ResourceScheme::parse(reserved).unwrap();
        let mut metadata = metadata();
        metadata.registration = registration;
        assert!(
            matches!(
                metadata.check(),
                Err(DeclarationError::Invalid {
                    field: "software_id",
                    ..
                })
            ),
            "{reserved}"
        );
    }
}

fn information() -> serde_json::Value {
    let mut wire = serde_json::to_value(registration_request()).unwrap();
    let object = wire.as_object_mut().unwrap();
    for (member, value) in [
        ("client_id", json!("clock-0b6f")),
        ("client_id_issued_at", json!(1_791_763_200)),
        ("registration_access_token", json!("rat-secret")),
        (
            "registration_client_uri",
            json!("https://hub.example/oauth/register/clock-0b6f"),
        ),
        (
            "veoveo_access_tokens",
            json!({
                "issuer": "https://hub.example",
                "deployment": registration().deployment,
                "jwks": {"keys": [{
                    "kty": "OKP", "crv": "Ed25519", "x": RFC8037_X,
                    "kid": RFC8037_THUMBPRINT, "alg": "EdDSA", "use": "sig"
                }]},
                "resource": "https://clock.example/clock/mcp"
            }),
        ),
        (
            "veoveo_database",
            json!({
                "binding": {"kind":"shared_logical_schema", "server":"clock"},
                "url": "postgres://postgres.internal:5432/veoveo?sslmode=verify-full",
                "username": "clock_0b6f",
                "password": "hunter2"
            }),
        ),
    ] {
        object.insert(member.to_owned(), value);
    }
    wire
}

#[test]
fn client_information_carries_token_trust_and_database_login() {
    let received: ClientInformation = serde_json::from_value(information()).unwrap();
    received.check().unwrap();
    assert_eq!(received.client_id.as_str(), "clock-0b6f");
    assert_eq!(
        received.access_tokens.issuer.as_str(),
        "https://hub.example"
    );
    assert_eq!(received.metadata, metadata());
    let debug = format!("{received:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains("rat-secret"));
    assert_eq!(
        serde_json::to_value(&received).unwrap(),
        information(),
        "the response round-trips, secrets included"
    );
    for issuer in ["veoveo-internal", "http://hub.example"] {
        let mut opaque_issuer = information();
        opaque_issuer["veoveo_access_tokens"]["issuer"] = json!(issuer);
        let received: ClientInformation = serde_json::from_value(opaque_issuer).unwrap();
        assert!(received.check().is_err(), "the issuer must be an https URL");
    }
    let mut elsewhere = information();
    elsewhere["veoveo_access_tokens"]["resource"] = json!("https://other.example/mcp");
    let received: ClientInformation = serde_json::from_value(elsewhere).unwrap();
    assert!(
        received.check().is_err(),
        "the audience is the registered endpoint"
    );
    let mut without_database = information();
    without_database
        .as_object_mut()
        .unwrap()
        .remove("veoveo_database");
    let received: ClientInformation = serde_json::from_value(without_database).unwrap();
    assert!(received.database.is_none());
}

#[test]
fn explicit_storage_binding_must_match_the_registered_logical_server() {
    let mut wire = information();
    wire["veoveo_database"]["binding"]["server"] = json!("another-server");
    let information: ClientInformation = serde_json::from_value(wire).unwrap();
    assert!(information.check().is_err());
    let mut wire = self::information();
    wire["veoveo_database"]
        .as_object_mut()
        .unwrap()
        .remove("binding");
    assert!(serde_json::from_value::<ClientInformation>(wire).is_err());
    let mut wire = self::information();
    wire["veoveo_database"]["binding"] = json!({"kind":"isolated", "server":"clock"});
    assert!(serde_json::from_value::<ClientInformation>(wire).is_err());
}

#[test]
fn token_trust_cannot_name_another_registered_deployment() {
    let mut wire = information();
    wire["veoveo_access_tokens"]["deployment"] = json!(veoveo_hub_sdk::DeploymentId::new());
    let information: ClientInformation = serde_json::from_value(wire).unwrap();
    assert!(information.check().is_err());
}

#[test]
fn database_grants_name_one_postgres_database_and_role() {
    let received: ClientInformation = serde_json::from_value(information()).unwrap();
    let database = received.database.unwrap();
    assert_eq!(database.username.as_str(), "clock_0b6f");
    assert_eq!(database.url.as_url().path(), "/veoveo");
    for (member, value) in [
        ("url", json!("mysql://database.internal:3306/veoveo")),
        ("url", json!("postgres://postgres.internal:5432/")),
        ("url", json!("postgres://postgres.internal/veoveo/extra")),
        (
            "url",
            json!("postgres://clock:secret@postgres.internal/veoveo"),
        ),
        ("url", json!("postgres://postgres.internal/veoveo#fragment")),
        ("username", json!("Clock")),
        ("username", json!("0clock")),
        ("username", json!("clock-0b6f")),
        ("username", json!("c".repeat(64))),
        ("namespace", json!("veoveo")),
    ] {
        let mut wire = information();
        wire["veoveo_database"][member] = value;
        assert!(
            serde_json::from_value::<ClientInformation>(wire).is_err(),
            "{member}"
        );
    }
    assert!(DatabaseIdentifier::new("_".repeat(63)).is_ok());
    assert!(DatabaseUrl::try_from("postgresql://[::1]:5432/veoveo".to_owned()).is_ok());
}

#[test]
fn registration_errors_use_rfc7591_codes() {
    let error = RegistrationErrorResponse {
        error: RegistrationErrorCode::UnapprovedSoftwareStatement,
        error_description: Some("namespace clock is pinned to another key".into()),
    };
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({
            "error": "unapproved_software_statement",
            "error_description": "namespace clock is pinned to another key"
        })
    );
    assert!(
        serde_json::from_value::<RegistrationErrorResponse>(json!({"error": "teapot"})).is_err()
    );
}

#[test]
fn authorization_server_metadata_urls_follow_rfc8414() {
    let root = url::Url::parse("https://hub.example").unwrap();
    assert_eq!(
        AuthorizationServerMetadata::url_for(&root).as_str(),
        "https://hub.example/.well-known/oauth-authorization-server"
    );
    let tenant = url::Url::parse("https://hub.example/tenant/").unwrap();
    assert_eq!(
        AuthorizationServerMetadata::url_for(&tenant).as_str(),
        "https://hub.example/.well-known/oauth-authorization-server/tenant"
    );
}

#[test]
fn authorization_server_metadata_names_its_key_set() {
    let metadata = |jwks_uri: Option<&str>| {
        let mut wire = json!({
            "issuer": "https://hub.example",
            "registration_endpoint": "https://hub.example/oauth/register",
            "token_endpoint": "https://hub.example/oauth/token"
        });
        if let Some(uri) = jwks_uri {
            wire["jwks_uri"] = json!(uri);
        }
        serde_json::from_value::<AuthorizationServerMetadata>(wire).unwrap()
    };
    let published = metadata(Some("https://hub.example/oauth/jwks.json"));
    assert_eq!(
        published.key_set_url().unwrap().as_str(),
        "https://hub.example/oauth/jwks.json"
    );
    assert_eq!(
        serde_json::to_value(&published).unwrap()["jwks_uri"],
        "https://hub.example/oauth/jwks.json"
    );
    assert!(
        metadata(Some("http://127.0.0.1:8080/oauth/jwks.json"))
            .key_set_url()
            .is_ok()
    );
    for rejected in [
        None,
        Some("http://hub.example/oauth/jwks.json"),
        Some("https://hub.example/oauth/jwks.json?kid=1"),
        Some("ftp://hub.example/jwks.json"),
    ] {
        assert!(metadata(rejected).key_set_url().is_err(), "{rejected:?}");
    }
}

#[test]
fn granted_permissions_are_an_rfc9396_detail() {
    let detail = PermissionsDetail::new(
        [
            PermissionName::parse("clock.read").unwrap(),
            PermissionName::parse("clock.events.write").unwrap(),
        ]
        .into(),
    );
    let wire = serde_json::to_value(&detail).unwrap();
    assert_eq!(
        wire,
        json!({
            "type": PERMISSIONS_DETAIL_TYPE,
            "actions": ["clock.events.write", "clock.read"]
        })
    );
    detail.check(&slug()).unwrap();
    assert!(detail.check(&ServerSlug::parse("map").unwrap()).is_err());
    for rejected in [
        json!({"type": "https://example.com/payment", "actions": ["clock.read"]}),
        json!({"type": PERMISSIONS_DETAIL_TYPE, "actions": ["clock.read"], "locations": []}),
        json!({"type": PERMISSIONS_DETAIL_TYPE, "actions": ["read"]}),
    ] {
        assert!(serde_json::from_value::<PermissionsDetail>(rejected).is_err());
    }
}

#[test]
fn published_schemas_admit_the_wire_examples() {
    fn validator<T: schemars::JsonSchema>() -> jsonschema::Validator {
        let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        assert_eq!(
            schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "schemas use JSON Schema 2020-12"
        );
        jsonschema::validator_for(&schema).unwrap()
    }
    for (validator, example) in [
        (
            validator::<AuthorizationDeclaration>(),
            serde_json::to_value(authorization()).unwrap(),
        ),
        (
            validator::<RegistrationDeclaration>(),
            serde_json::to_value(registration()).unwrap(),
        ),
        (
            validator::<ClientRegistrationRequest>(),
            serde_json::to_value(registration_request()).unwrap(),
        ),
        (validator::<ClientInformation>(), information()),
        (
            validator::<PermissionsDetail>(),
            serde_json::to_value(PermissionsDetail::new(
                [PermissionName::parse("clock.read").unwrap()].into(),
            ))
            .unwrap(),
        ),
        (
            validator::<RegistrationErrorResponse>(),
            json!({"error": "invalid_client_metadata", "error_description": "tool undeclared"}),
        ),
    ] {
        assert!(validator.is_valid(&example), "{example}");
    }
    let validator = validator::<ClientRegistrationRequest>();
    for (pointer, value) in [
        (
            "/veoveo_authorization/permissions/0/access",
            json!("execute"),
        ),
        ("/jwks/keys/0/kty", json!("RSA")),
        ("/token_endpoint_auth_method", json!("client_secret_basic")),
    ] {
        let mut invalid = serde_json::to_value(registration_request()).unwrap();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(!validator.is_valid(&invalid), "{pointer}");
    }
}

#[test]
fn advertised_declarations_round_trip_through_capabilities() {
    let mut capabilities = rmcp::model::ServerCapabilities::default();
    mcp::advertise(&mut capabilities, &registration(), &authorization());
    let advertised = mcp::advertised(&capabilities).unwrap().unwrap();
    assert_eq!(advertised.registration, registration());
    assert_eq!(advertised.authorization, authorization());
    assert_eq!(
        mcp::advertised(&rmcp::model::ServerCapabilities::default()).unwrap(),
        None
    );
}

#[test]
fn plain_http_is_allowed_only_for_local_hosts() {
    for local in [
        "http://localhost:8800/mcp",
        "http://time.localhost:8800/mcp",
        "http://127.0.0.1:8800/mcp",
        "http://[::1]:8800/mcp",
        "http://vv-mcp-time.internal:8800/mcp",
    ] {
        assert!(
            ServerEndpoint::new(url::Url::parse(local).unwrap()).is_ok(),
            "{local}"
        );
        assert!(ResourceIndicator::parse(local).is_ok(), "{local}");
    }
    for remote in [
        "http://time.example/mcp",
        "http://10.0.0.5:8800/mcp",
        "ftp://localhost/mcp",
    ] {
        assert!(
            ServerEndpoint::new(url::Url::parse(remote).unwrap()).is_err(),
            "{remote}"
        );
        assert!(ResourceIndicator::parse(remote).is_err(), "{remote}");
    }
    let local_issuer = veoveo_types::TokenIssuer::parse("http://vv-hub.internal:8080").unwrap();
    assert!(issuer_url(&local_issuer).is_ok());
    let remote_issuer = veoveo_types::TokenIssuer::parse("http://hub.example").unwrap();
    assert!(issuer_url(&remote_issuer).is_err());
}

#[test]
fn exact_resource_access_overrides_full_template_conflicts_and_keeps_its_borrow() {
    let mut declaration = authorization();
    declaration.requirements.resources.clear();
    declaration.requirements.resource_templates = [
        (
            template("clock://events/{event}"),
            RouteAccess::Authenticated,
        ),
        (
            template("clock://events/{+tail}"),
            policy(ClockPermission::Read),
        ),
    ]
    .into();
    declaration.check(&slug(), &scheme()).unwrap();
    let uri = ResourceUri::new("clock://events/one").unwrap();
    assert!(
        declaration
            .access(RequirementTarget::Resource(&uri))
            .is_none()
    );
    declaration
        .requirements
        .resources
        .insert(uri.clone(), RouteAccess::Authenticated);
    let exact = declaration.requirements.resources.get(&uri).unwrap();
    let output = declaration
        .access(RequirementTarget::Resource(&uri))
        .unwrap();
    assert_eq!(output, &RouteAccess::Authenticated);
    assert!(std::ptr::eq(output, exact));
}

#[test]
fn agreeing_templates_return_the_original_first_complete_access_value() {
    let mut declaration = authorization();
    declaration.requirements.resources.clear();
    declaration.requirements.resource_templates = [
        (
            template("clock://events/{event}"),
            RouteAccess::Authenticated,
        ),
        (
            template("clock://events/{+tail}"),
            RouteAccess::Authenticated,
        ),
    ]
    .into();
    declaration.check(&slug(), &scheme()).unwrap();
    let uri = ResourceUri::new("clock://events/one").unwrap();
    let first = declaration
        .requirements
        .resource_templates
        .first_key_value()
        .unwrap()
        .1;
    let output = declaration
        .access(RequirementTarget::Resource(&uri))
        .unwrap();
    assert_eq!(output, &RouteAccess::Authenticated);
    assert!(std::ptr::eq(output, first));
}

#[test]
fn native_template_matching_preserves_empty_scalar_zero_tail_and_trailing_slash() {
    let mut declaration = authorization();
    declaration.requirements.resources.clear();
    declaration.requirements.resource_templates = [
        (
            template("clock://scalar/{value}"),
            RouteAccess::Authenticated,
        ),
        (template("clock://tail/{+rest}"), RouteAccess::Authenticated),
        (
            template("clock://slash/{value}/"),
            RouteAccess::Authenticated,
        ),
    ]
    .into();
    declaration.check(&slug(), &scheme()).unwrap();
    for wire in [
        "clock://scalar/",
        "clock://tail",
        "clock://tail/a/b",
        "clock://slash/a/",
    ] {
        let uri = ResourceUri::new(wire).unwrap();
        assert_eq!(
            declaration.access(RequirementTarget::Resource(&uri)),
            Some(&RouteAccess::Authenticated),
            "{wire}"
        );
    }
    for wire in ["clock://scalar", "clock://slash/a", "clock://elsewhere/a/"] {
        let uri = ResourceUri::new(wire).unwrap();
        assert!(
            declaration
                .access(RequirementTarget::Resource(&uri))
                .is_none(),
            "{wire}"
        );
    }
}

/// A server with a webhook route, the SDK's operation callbacks and an app origin that
/// `clock.admin` admits a person to.
fn surfaces() -> SurfacesDeclaration {
    let route = |prefix: &str, bytes, rate| PublicRouteDeclaration {
        prefix: PublicRoutePrefix::new(prefix).unwrap(),
        max_body_bytes: BodyLimit::new(bytes).unwrap(),
        requests_per_minute: RequestRate::per_minute(rate).unwrap(),
    };
    SurfacesDeclaration {
        public_routes: vec![
            route("/clock/hooks/", 1_048_576, 600),
            route("/clock/operations/", 1_048_576, 600),
        ],
        app_origin: true,
    }
}
fn surfaced_registration() -> RegistrationDeclaration {
    RegistrationDeclaration {
        surfaces: Some(surfaces()),
        ..registration()
    }
}
fn surfaced_authorization() -> AuthorizationDeclaration {
    let mut declaration = authorization();
    declaration.requirements.app_origin = Some(policy(ClockPermission::Admin));
    declaration
}
fn surfaced_metadata() -> ClientMetadata {
    ClientMetadata::new(
        rfc8037_key(),
        surfaced_registration(),
        surfaced_authorization(),
    )
    .unwrap()
}

#[test]
fn surfaces_wire_format_in_discover_and_registration() {
    let discover = json!({
        "publicRoutes": [
            {"prefix": "/clock/hooks/", "maxBodyBytes": 1_048_576, "requestsPerMinute": 600},
            {"prefix": "/clock/operations/", "maxBodyBytes": 1_048_576, "requestsPerMinute": 600}
        ],
        "appOrigin": true
    });
    let advertised = serde_json::to_value(surfaced_registration()).unwrap();
    assert_eq!(advertised["surfaces"], discover);
    assert_eq!(
        serde_json::from_value::<RegistrationDeclaration>(advertised).unwrap(),
        surfaced_registration()
    );
    let wire = serde_json::to_value(surfaced_metadata()).unwrap();
    assert_eq!(
        wire["veoveo_surfaces"],
        json!({
            "public_routes": [
                {"prefix": "/clock/hooks/", "max_body_bytes": 1_048_576, "requests_per_minute": 600},
                {"prefix": "/clock/operations/", "max_body_bytes": 1_048_576, "requests_per_minute": 600}
            ],
            "app_origin": true
        })
    );
    assert_eq!(
        wire["veoveo_authorization"]["requirements"]["app_origin"],
        json!({"kind": "permission", "permission": "clock.admin"})
    );
    assert_eq!(
        serde_json::to_value(surfaced_authorization()).unwrap()["requirements"]["appOrigin"],
        json!({"kind": "permission", "permission": "clock.admin"})
    );
    let decoded: ClientMetadata = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded, surfaced_metadata());
    decoded.check().unwrap();

    // Only an app origin, or only public routes, omits the other member.
    let origin_only = SurfacesDeclaration {
        public_routes: Vec::new(),
        app_origin: true,
    };
    assert_eq!(
        serde_json::to_value(&origin_only).unwrap(),
        json!({"appOrigin": true})
    );
    let routes_only = SurfacesDeclaration {
        app_origin: false,
        ..surfaces()
    };
    assert!(
        serde_json::to_value(&routes_only)
            .unwrap()
            .get("appOrigin")
            .is_none()
    );
    // A server serving neither omits the member in both spellings.
    assert!(
        serde_json::to_value(registration())
            .unwrap()
            .get("surfaces")
            .is_none()
    );
    assert!(
        serde_json::to_value(metadata())
            .unwrap()
            .get("veoveo_surfaces")
            .is_none()
    );
    // The member admits only its own fields.
    for (pointer, member) in [
        ("/veoveo_surfaces", "owner"),
        ("/veoveo_surfaces/public_routes/0", "methods"),
    ] {
        let mut extended = wire.clone();
        extended.pointer_mut(pointer).unwrap()[member] = json!("x");
        assert!(
            serde_json::from_value::<ClientMetadata>(extended).is_err(),
            "{pointer}/{member}"
        );
    }
}

#[test]
fn surfaces_are_checked_against_the_mount_limits_and_requirement() {
    let check = |surfaces: SurfacesDeclaration, authorization: AuthorizationDeclaration| {
        ClientMetadata::new(
            rfc8037_key(),
            RegistrationDeclaration {
                surfaces: Some(surfaces),
                ..registration()
            },
            authorization,
        )
    };
    check(surfaces(), surfaced_authorization()).unwrap();
    let with_prefixes = |prefixes: &[&str]| SurfacesDeclaration {
        public_routes: prefixes
            .iter()
            .map(|prefix| PublicRouteDeclaration {
                prefix: PublicRoutePrefix::new(*prefix).unwrap(),
                ..surfaces().public_routes[0].clone()
            })
            .collect(),
        app_origin: false,
    };
    for (prefixes, expected) in [
        (
            &["/time/hooks/"][..],
            "does not lie under the server's mount",
        ),
        (
            &["/clockwork/hooks/"][..],
            "does not lie under the server's mount",
        ),
        (&["/clock/"][..], "does not lie under the server's mount"),
        (
            &["/clock/mcp/"][..],
            "covers the server's MCP or administrative routes",
        ),
        (
            &["/clock/mcp/hooks/"][..],
            "covers the server's MCP or administrative routes",
        ),
        (
            &["/clock/admin/"][..],
            "covers the server's MCP or administrative routes",
        ),
        (&["/clock/hooks/", "/clock/hooks/github/"][..], "overlaps"),
        (&["/clock/hooks/github/", "/clock/hooks/"][..], "overlaps"),
        (&["/clock/hooks/", "/clock/hooks/"][..], "overlaps"),
    ] {
        let error = check(with_prefixes(prefixes), authorization()).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "{prefixes:?}: {error}"
        );
    }
    // Neighbouring prefixes and names that merely start like a reserved route pass.
    check(
        with_prefixes(&[
            "/clock/hooks/",
            "/clock/hooksmith/",
            "/clock/mcpx/",
            "/clock/administer/",
        ]),
        authorization(),
    )
    .unwrap();
    // An app origin is declared exactly with its requirement.
    assert!(check(surfaces(), authorization()).is_err());
    let routes_only = SurfacesDeclaration {
        app_origin: false,
        ..surfaces()
    };
    assert!(check(routes_only, surfaced_authorization()).is_err());
    assert!(ClientMetadata::new(rfc8037_key(), registration(), surfaced_authorization()).is_err());
    // An empty declaration is omitted, never sent.
    assert!(check(SurfacesDeclaration::default(), authorization()).is_err());
    // The app origin's requirement names a declared permission.
    let mut undeclared = surfaced_authorization();
    undeclared.requirements.app_origin = Some(RouteAccess::Permission(
        PermissionName::parse("clock.editor").unwrap(),
    ));
    assert!(matches!(
        check(surfaces(), undeclared),
        Err(DeclarationError::UndeclaredPermission { .. })
    ));
    let mut authenticated = surfaced_authorization();
    authenticated.requirements.app_origin = Some(RouteAccess::Authenticated);
    check(surfaces(), authenticated).unwrap();
}

#[test]
fn public_route_values_are_admitted_once() {
    for prefix in ["/clock/hooks/", "/clock/hooks/v1.0/", "/clock/a-b_c~d/"] {
        assert!(PublicRoutePrefix::new(prefix).is_ok(), "{prefix}");
    }
    for prefix in [
        "/clock/hooks",
        "clock/hooks/",
        "/",
        "//",
        "/clock//hooks/",
        "/clock/../hooks/",
        "/clock/./hooks/",
        "/clock/hooks%20/",
        "/clock/hooks?x/",
        "/clock/{hook}/",
    ] {
        assert!(PublicRoutePrefix::new(prefix).is_err(), "{prefix}");
    }
    assert_eq!(
        PublicRoutePrefix::under(&slug(), "hooks/")
            .unwrap()
            .as_str(),
        "/clock/hooks/"
    );
    for bytes in [1, MAX_PUBLIC_BODY_BYTES] {
        assert!(BodyLimit::new(bytes).is_ok(), "{bytes}");
    }
    for bytes in [0, MAX_PUBLIC_BODY_BYTES + 1] {
        assert!(BodyLimit::new(bytes).is_err(), "{bytes}");
    }
    for rate in [1, MAX_PUBLIC_REQUESTS_PER_MINUTE] {
        assert!(RequestRate::per_minute(rate).is_ok(), "{rate}");
    }
    for rate in [0, MAX_PUBLIC_REQUESTS_PER_MINUTE + 1] {
        assert!(RequestRate::per_minute(rate).is_err(), "{rate}");
    }
    let mut wire = serde_json::to_value(surfaced_metadata()).unwrap();
    for (member, value) in [
        ("max_body_bytes", json!(0)),
        ("max_body_bytes", json!(16_777_217)),
        ("requests_per_minute", json!(0)),
        ("requests_per_minute", json!(6001)),
        ("prefix", json!("/clock/hooks")),
    ] {
        let original = wire["veoveo_surfaces"]["public_routes"][0][member].clone();
        wire["veoveo_surfaces"]["public_routes"][0][member] = value;
        assert!(
            serde_json::from_value::<ClientMetadata>(wire.clone()).is_err(),
            "{member}"
        );
        wire["veoveo_surfaces"]["public_routes"][0][member] = original;
    }
}

#[test]
fn app_origin_access_is_looked_up_like_a_tools() {
    let declaration = surfaced_authorization();
    assert_eq!(
        declaration
            .access(RequirementTarget::AppOrigin)
            .and_then(RouteAccess::permission)
            .map(PermissionName::as_str),
        Some("clock.admin")
    );
    assert_eq!(authorization().access(RequirementTarget::AppOrigin), None);
}

/// The Hub's answer to a server that declared its surfaces.
fn surfaced_information() -> serde_json::Value {
    let mut wire = information();
    let registered = serde_json::to_value(surfaced_metadata()).unwrap();
    for member in ["veoveo_surfaces", "veoveo_authorization"] {
        wire[member] = registered[member].clone();
    }
    wire["veoveo_public_base"] = json!("https://veoveo.example.com/b/0199d4a1/clock");
    wire["veoveo_app_origin"] = json!("https://clock.apps.veoveo.example.com");
    wire
}

#[test]
fn the_response_names_where_the_hub_serves_the_surfaces() {
    let received: ClientInformation = serde_json::from_value(surfaced_information()).unwrap();
    received.check().unwrap();
    let base = received.public_base.as_ref().unwrap();
    assert_eq!(base.as_str(), "https://veoveo.example.com/b/0199d4a1/clock");
    assert_eq!(base.host_authority(), "veoveo.example.com");
    assert_eq!(
        base.join(["hooks", "a b"]).as_str(),
        "https://veoveo.example.com/b/0199d4a1/clock/hooks/a%20b"
    );
    let origin = received.app_origin.as_ref().unwrap();
    assert_eq!(origin.as_str(), "https://clock.apps.veoveo.example.com");
    assert_eq!(origin.host_authority(), "clock.apps.veoveo.example.com");
    assert_eq!(
        serde_json::to_value(&received).unwrap(),
        surfaced_information(),
        "the response round-trips"
    );
    // Each member is present exactly when its surface is declared.
    for member in ["veoveo_public_base", "veoveo_app_origin"] {
        let mut missing = surfaced_information();
        missing.as_object_mut().unwrap().remove(member);
        let received: ClientInformation = serde_json::from_value(missing).unwrap();
        assert!(received.check().is_err(), "without {member}");
        let mut undeclared = information();
        undeclared[member] = surfaced_information()[member].clone();
        let received: ClientInformation = serde_json::from_value(undeclared).unwrap();
        assert!(received.check().is_err(), "{member} without its surface");
    }
    // A server without surfaces gets neither.
    let received: ClientInformation = serde_json::from_value(information()).unwrap();
    assert!(received.public_base.is_none() && received.app_origin.is_none());
}

#[test]
fn public_bases_and_app_origins_are_admitted_once() {
    for base in [
        "https://veoveo.example.com/clock",
        "https://veoveo.example.com:8443/b/1/clock",
        "http://localhost:8080/clock",
        "http://vv-hub.internal:8080/clock",
    ] {
        assert!(PublicBase::parse(base).is_ok(), "{base}");
    }
    for base in [
        "https://veoveo.example.com",
        "https://veoveo.example.com/",
        "https://veoveo.example.com/clock/",
        "https://veoveo.example.com//clock",
        "https://veoveo.example.com/clock?x=1",
        "https://veoveo.example.com/clock#top",
        "https://user@veoveo.example.com/clock",
        "https://Veoveo.example.com/clock",
        "http://veoveo.example.com/clock",
        "veoveo.example.com/clock",
    ] {
        assert!(PublicBase::parse(base).is_err(), "{base}");
    }
    for origin in [
        "https://clock.apps.veoveo.example.com",
        "https://clock.apps.veoveo.example.com:8443",
        "http://clock.apps.localhost:8080",
    ] {
        let parsed = AppOrigin::parse(origin).unwrap();
        assert_eq!(parsed.to_string(), origin);
    }
    for origin in [
        "https://clock.apps.veoveo.example.com/",
        "https://clock.apps.veoveo.example.com/app",
        "https://clock.apps.veoveo.example.com:443",
        "https://clock.apps.veoveo.example.com?x=1",
        "https://user@clock.apps.veoveo.example.com",
        "http://clock.apps.veoveo.example.com",
        "clock.apps.veoveo.example.com",
    ] {
        assert!(AppOrigin::parse(origin).is_err(), "{origin}");
    }
    let mut wire = surfaced_information();
    wire["veoveo_public_base"] = json!("https://veoveo.example.com/clock?x=1");
    assert!(serde_json::from_value::<ClientInformation>(wire).is_err());
}

#[test]
fn published_schemas_admit_the_surfaces_examples() {
    for (schema, example) in [
        (
            serde_json::to_value(schemars::schema_for!(RegistrationDeclaration)).unwrap(),
            serde_json::to_value(surfaced_registration()).unwrap(),
        ),
        (
            serde_json::to_value(schemars::schema_for!(ClientRegistrationRequest)).unwrap(),
            serde_json::to_value(ClientRegistrationRequest {
                metadata: surfaced_metadata(),
                software_statement: SoftwareStatement::new(STATEMENT).unwrap(),
            })
            .unwrap(),
        ),
        (
            serde_json::to_value(schemars::schema_for!(ClientInformation)).unwrap(),
            surfaced_information(),
        ),
    ] {
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(validator.is_valid(&example), "{example}");
    }
    let validator = jsonschema::validator_for(
        &serde_json::to_value(schemars::schema_for!(ClientRegistrationRequest)).unwrap(),
    )
    .unwrap();
    for (pointer, value) in [
        ("/veoveo_surfaces/public_routes/0/max_body_bytes", json!(0)),
        (
            "/veoveo_surfaces/public_routes/0/requests_per_minute",
            json!(6001),
        ),
        (
            "/veoveo_surfaces/public_routes/0/prefix",
            json!("/clock/hooks"),
        ),
    ] {
        let mut invalid = serde_json::to_value(ClientRegistrationRequest {
            metadata: surfaced_metadata(),
            software_statement: SoftwareStatement::new(STATEMENT).unwrap(),
        })
        .unwrap();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(!validator.is_valid(&invalid), "{pointer}");
    }
}
