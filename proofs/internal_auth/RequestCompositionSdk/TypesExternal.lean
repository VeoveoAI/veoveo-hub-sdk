-- Handwritten native value models; generated own-code bodies are untouched.
module
public import Aeneas
@[expose] public section
open Aeneas Aeneas.Std
namespace RequestCompositionSdk

structure NativeId (tag : Nat) where
  value : _root_.String
  deriving DecidableEq

/-- Traversal is retained explicitly. Native correspondence additionally requires
unique entries in the native key order; canonical scope rendering uses that law. -/
structure OrderedSet (T : Type _) where
  values : List T

structure NativeTimestamp where
  seconds : I64
  nanoseconds : U32
  deriving DecidableEq

/-- [core::net::ip_addr::Ipv6Addr]
    Source: '/rustc/library/core/src/net/ip_addr.rs', lines 168:0-168:19
    Name pattern: [core::net::ip_addr::Ipv6Addr]
    Visibility: public -/
@[rust_type "core::net::ip_addr::Ipv6Addr"]
abbrev core.net.ip_addr.Ipv6Addr := NativeId 0

/-- [core::net::ip_addr::Ipv4Addr]
    Source: '/rustc/library/core/src/net/ip_addr.rs', lines 74:0-74:19
    Name pattern: [core::net::ip_addr::Ipv4Addr]
    Visibility: public -/
@[rust_type "core::net::ip_addr::Ipv4Addr"]
abbrev core.net.ip_addr.Ipv4Addr := NativeId 1

/-- [core::option::IntoIter]
    Source: '/rustc/library/core/src/option.rs', lines 2625:0-2625:22
    Name pattern: [core::option::IntoIter]
    Visibility: public -/
@[rust_type "core::option::IntoIter"]
abbrev core.option.IntoIter (A : Type _) := Option A

/-- [alloc::collections::btree::set::BTreeSet]
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 78:0-81:1
    Name pattern: [alloc::collections::btree::set::BTreeSet]
    Visibility: public -/
@[rust_type "alloc::collections::btree::set::BTreeSet"]
abbrev alloc.collections.btree.set.BTreeSet (T : Type _) (_A : Type _) := OrderedSet T

/-- [alloc::collections::btree::set::Iter]
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 135:0-135:26
    Name pattern: [alloc::collections::btree::set::Iter]
    Visibility: public -/
@[rust_type "alloc::collections::btree::set::Iter"]
abbrev alloc.collections.btree.set.Iter (T : Type _) := List T

/-- [chrono::offset::fixed::FixedOffset]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/fixed.rs', lines 30:0-30:22
    Name pattern: [chrono::offset::fixed::FixedOffset]
    Visibility: public -/
@[rust_type "chrono::offset::fixed::FixedOffset"]
abbrev chrono.offset.fixed.FixedOffset := Unit

/-- [chrono::naive::datetime::NaiveDateTime]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/naive/datetime/mod.rs', lines 76:0-76:24
    Name pattern: [chrono::naive::datetime::NaiveDateTime]
    Visibility: public -/
@[rust_type "chrono::naive::datetime::NaiveDateTime"]
abbrev chrono.naive.datetime.NaiveDateTime := NativeTimestamp

/-- [chrono::naive::date::NaiveDate]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/naive/date/mod.rs', lines 102:0-102:20
    Name pattern: [chrono::naive::date::NaiveDate]
    Visibility: public -/
@[rust_type "chrono::naive::date::NaiveDate"]
abbrev chrono.naive.date.NaiveDate := Int

/-- [chrono::datetime::DateTime]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 56:0-56:33
    Name pattern: [chrono::datetime::DateTime]
    Visibility: public -/
@[rust_type "chrono::datetime::DateTime"]
abbrev chrono.datetime.DateTime (_Tz : Type _) (_Offset : Type _) := NativeTimestamp

/-- [jsonwebtoken::errors::Error]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/jsonwebtoken-11.0.0/src/errors.rs', lines 17:0-17:16
    Name pattern: [jsonwebtoken::errors::Error]
    Visibility: public -/
@[rust_type "jsonwebtoken::errors::Error"]
abbrev jsonwebtoken.errors.Error := NativeId 2

/-- [serde_json::error::Error]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde_json-1.0.151/src/error.rs', lines 17:0-17:16
    Name pattern: [serde_json::error::Error]
    Visibility: public -/
@[rust_type "serde_json::error::Error"]
abbrev serde_json.error.Error := NativeId 3

/-- [url::Url]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/lib.rs', lines 228:0-228:14
    Name pattern: [url::Url]
    Visibility: public -/
@[rust_type "url::Url"]
abbrev url.Url := NativeId 4

/-- [uuid::Uuid]
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/uuid-1.25.0/src/lib.rs', lines 467:0-467:15
    Name pattern: [uuid::Uuid]
    Visibility: public -/
@[rust_type "uuid::Uuid"]
abbrev uuid.Uuid := NativeId 5

/-- [veoveo_audit_contract::ids::AuditRequestId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/audit/contract/src/ids.rs', lines 17:0-17:25
    Name pattern: [veoveo_audit_contract::ids::AuditRequestId]
    Visibility: public -/
@[rust_type "veoveo_audit_contract::ids::AuditRequestId"]
abbrev veoveo_audit_contract.ids.AuditRequestId := NativeId 6

/-- [veoveo_audit_contract::ids::AuditTraceId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/audit/contract/src/ids.rs', lines 23:0-23:23
    Name pattern: [veoveo_audit_contract::ids::AuditTraceId]
    Visibility: public -/
@[rust_type "veoveo_audit_contract::ids::AuditTraceId"]
abbrev veoveo_audit_contract.ids.AuditTraceId := NativeId 7

/-- [veoveo_audit_contract::ids::AuditSpanId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/audit/contract/src/ids.rs', lines 26:0-26:22
    Name pattern: [veoveo_audit_contract::ids::AuditSpanId]
    Visibility: public -/
@[rust_type "veoveo_audit_contract::ids::AuditSpanId"]
abbrev veoveo_audit_contract.ids.AuditSpanId := NativeId 8

/-- [veoveo_types::task::TaskId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/task.rs', lines 17:0-17:17
    Name pattern: [veoveo_types::task::TaskId]
    Visibility: public -/
@[rust_type "veoveo_types::task::TaskId"]
abbrev veoveo_types.task.TaskId := NativeId 9

/-- [veoveo_types::identity::GroupId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 26:0-26:18
    Name pattern: [veoveo_types::identity::GroupId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::GroupId"]
abbrev veoveo_types.identity.GroupId := NativeId 10

/-- [veoveo_types::identity::PrincipalId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 10:0-10:22
    Name pattern: [veoveo_types::identity::PrincipalId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::PrincipalId"]
abbrev veoveo_types.identity.PrincipalId := NativeId 11

/-- [veoveo_types::identity::DataLabelId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 6:0-6:22
    Name pattern: [veoveo_types::identity::DataLabelId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::DataLabelId"]
abbrev veoveo_types.identity.DataLabelId := NativeId 12

/-- [veoveo_types::identity::DelegationId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 22:0-22:23
    Name pattern: [veoveo_types::identity::DelegationId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::DelegationId"]
abbrev veoveo_types.identity.DelegationId := NativeId 13

/-- [veoveo_types::identity::PolicyVersion]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 34:0-34:24
    Name pattern: [veoveo_types::identity::PolicyVersion]
    Visibility: public -/
@[rust_type "veoveo_types::identity::PolicyVersion"]
abbrev veoveo_types.identity.PolicyVersion := NativeId 14

/-- [veoveo_types::identity::WorkContextId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 18:0-18:24
    Name pattern: [veoveo_types::identity::WorkContextId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::WorkContextId"]
abbrev veoveo_types.identity.WorkContextId := NativeId 15

/-- [veoveo_types::identity::TenantId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 14:0-14:19
    Name pattern: [veoveo_types::identity::TenantId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::TenantId"]
abbrev veoveo_types.identity.TenantId := NativeId 16

/-- [veoveo_types::digest::Sha256Digest]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/digest.rs', lines 10:0-10:23
    Name pattern: [veoveo_types::digest::Sha256Digest]
    Visibility: public -/
@[rust_type "veoveo_types::digest::Sha256Digest"]
abbrev veoveo_types.digest.Sha256Digest := NativeId 17

/-- [veoveo_types::error::IdentifierError]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/error.rs', lines 5:0-5:26
    Name pattern: [veoveo_types::error::IdentifierError]
    Visibility: public -/
@[rust_type "veoveo_types::error::IdentifierError"]
abbrev veoveo_types.error.IdentifierError := NativeId 18

/-- [veoveo_types::execution::DeploymentId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/execution.rs', lines 19:0-19:23
    Name pattern: [veoveo_types::execution::DeploymentId]
    Visibility: public -/
@[rust_type "veoveo_types::execution::DeploymentId"]
abbrev veoveo_types.execution.DeploymentId := NativeId 19

/-- [veoveo_types::identity::RoleId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 30:0-30:17
    Name pattern: [veoveo_types::identity::RoleId]
    Visibility: public -/
@[rust_type "veoveo_types::identity::RoleId"]
abbrev veoveo_types.identity.RoleId := NativeId 20

/-- [veoveo_types::identity::TokenIssuer]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 115:0-115:22
    Name pattern: [veoveo_types::identity::TokenIssuer]
    Visibility: public -/
@[rust_type "veoveo_types::identity::TokenIssuer"]
abbrev veoveo_types.identity.TokenIssuer := NativeId 21

/-- [veoveo_types::identity::TokenSubject]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 118:0-118:23
    Name pattern: [veoveo_types::identity::TokenSubject]
    Visibility: public -/
@[rust_type "veoveo_types::identity::TokenSubject"]
abbrev veoveo_types.identity.TokenSubject := NativeId 22

/-- [veoveo_types::names::ScopeName]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/names.rs', lines 5:0-5:20
    Name pattern: [veoveo_types::names::ScopeName]
    Visibility: public -/
@[rust_type "veoveo_types::names::ScopeName"]
abbrev veoveo_types.names.ScopeName := NativeId 23

/-- [veoveo_types::permission_names::PermissionName]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/permission_names.rs', lines 7:0-7:25
    Name pattern: [veoveo_types::permission_names::PermissionName]
    Visibility: public -/
@[rust_type "veoveo_types::permission_names::PermissionName"]
abbrev veoveo_types.permission_names.PermissionName := NativeId 24

/-- [veoveo_types::platform_names::ServerSlug]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 6:0-6:21
    Name pattern: [veoveo_types::platform_names::ServerSlug]
    Visibility: public -/
@[rust_type "veoveo_types::platform_names::ServerSlug"]
abbrev veoveo_types.platform_names.ServerSlug := NativeId 25

/-- [veoveo_types::platform_names::OAuthClientId]
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 40:0-40:24
    Name pattern: [veoveo_types::platform_names::OAuthClientId]
    Visibility: public -/
@[rust_type "veoveo_types::platform_names::OAuthClientId"]
abbrev veoveo_types.platform_names.OAuthClientId := NativeId 26

/-- [veoveo_hub_sdk::internal_auth::HubInternalSigningKey]
    Source: 'crates/veoveo-hub-sdk/src/internal_auth.rs', lines 64:0-68:1
    Visibility: public -/
abbrev internal_auth.HubInternalSigningKey := NativeId 27

end RequestCompositionSdk
