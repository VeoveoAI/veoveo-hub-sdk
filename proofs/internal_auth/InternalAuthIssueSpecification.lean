import InternalAuthIdentityProof
import InternalAuthClaimsComponents

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof InternalAuthIdentityProof
open InternalAuthClaimsComponents

namespace InternalAuthIssueSpecification
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/-- Independent UTF-8 byte sequence used to state the modeled first-dot rule.
The existing handwritten Core-helper refinement boundary is still open: this
relation is not advertised as a proof of the untranslated native helper. -/
def utf8Bytes (text : String) : List U8 :=
  text.toByteArray.toList.map (fun byte =>
    ⟨byte.toNat, by cases byte; simp only [UInt8.toNat_ofBitVec,
      UScalarTy.U8_numBits_eq, Nat.reducePow]; omega⟩)

/-- The first ASCII-dot segment, compared as UTF-8 bytes. Empty/no-dot names are
included; no unproved parsed-name invariant narrows the typed model's domain. -/
def Owned (permission : veoveo_types.permission_names.PermissionName)
    (server : veoveo_types.platform_names.ServerSlug) : Prop :=
  (utf8Bytes permission.value).takeWhile (fun byte => decide (byte.val ≠ 46)) =
    utf8Bytes server.value

def firstForeign (permissions : List veoveo_types.permission_names.PermissionName)
    (server : veoveo_types.platform_names.ServerSlug) : Option veoveo_types.permission_names.PermissionName :=
  permissions.find? (fun permission => decide (¬ Owned permission server))

/-- Physical representation bounds, not additional native authorization rules.
The modeled traversal is exact for arbitrary finite lists; native first-error
ordering additionally needs the existing sorted/unique BTreeSet correspondence. -/
structure PermissionTextDomain (request : Request) : Prop where
  server : TextRepresentable request.server.value
  permissions : ∀ permission ∈ request.permissions.values, TextRepresentable permission.value

structure IssueTextDomain (request : Request) : Prop where
  permission_text : PermissionTextDomain request
  context_text : RequestTextDomain request
  actor_id : TextRepresentable request.actor.id.value
  audience : TextRepresentable request.audience.value
  scopes : ∀ scope ∈ request.actor.scopes.values, TextRepresentable scope.value
  rendered_scopes : TextRepresentable (String.intercalate " "
    (request.actor.scopes.values.map (·.value)))

/-- Complete independent public outcome. A first foreign permission precedes all
identity setup/validation. Otherwise use the already-proved independent identity
outcome, complete claims value and the actual precisely typed signer boundary.
No own-function-success or successful-signing assumption is introduced. -/
def issueOutcome (issuer : internal_auth.HubInternalTokenIssuer) (request : Request) :
    Result (core.result.Result internal_auth.IssuedHubInternalToken internal_auth.InternalTokenError) :=
  match firstForeign request.permissions.values request.server with
  | some foreign => .ok (.Err (.PermissionOutsideAudience foreign))
  | none => do
    let result ← identityOutcome issuer request
    match result with
    | .Err error => .ok (.Err error)
    | .Ok token =>
      let signed ← internal_auth.HubInternalTokenIssuer.sign_forwarded issuer (claimsValue token)
      match signed with
      | .Err error => .ok (.Err error)
      | .Ok bearer => .ok (.Ok { bearer_token := bearer, identity := token })

/- Intended complete theorem in InternalAuthIssueProof (not an axiom):
  (issuer) (request) (domain : IssueTextDomain request) :
    HubInternalTokenIssuer.issue issuer request = issueOutcome issuer request.

Also derive the first-foreign refusal using only PermissionTextDomain, and a
populated all-owned nonempty-permission success using typed provider/clock/ID and
signer observations. ClaimsTextDomain for constructed identities is derived from
request-field preservation, never assumed from_identity success. Existing Core
namespace/UTF8/allocation/sorted-traversal/native-signing refinement gaps remain
explicit; no authenticity, live-Task, policy-grant or crypto roundtrip assertion.
-/
end
end InternalAuthIssueSpecification
