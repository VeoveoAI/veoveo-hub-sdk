import InternalAuthIssueExamples
import InternalAuthIssueProof
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextExamples InternalAuthIdentityProof
open InternalAuthClaimsComponents InternalAuthIssueSpecification
open InternalAuthIssueExamples InternalAuthIdentityExamples InternalAuthClaimsExamples

namespace InternalAuthIssueWitnesses
noncomputable section

/-- Complete expected identity, independently assembled from the populated
request and the earlier source expiry. The issuer/ID remain arbitrary typed inputs. -/
def ordinaryIdentity (issuer : internal_auth.HubInternalTokenIssuer) (jwtId : identity.JwtId) : Identity :=
  InternalAuthCorrectness.identityValue issuer ordinaryRequest seedIssuedAt seedExpiresAt jwtId

/-- Actual public issuance with two owned permissions and populated Direct
context. All premises observe external primitives; no own-code success is assumed.
The exact signer argument contains the complete independently specified claims. -/
theorem ordinary_issued (issuer : internal_auth.HubInternalTokenIssuer)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt)
    (bearer : String)
    (signed : internal_auth.HubInternalTokenIssuer.sign_forwarded issuer
      (claimsValue (ordinaryIdentity issuer jwtId)) = .ok (.Ok bearer)) :
    internal_auth.HubInternalTokenIssuer.issue issuer ordinaryRequest =
      .ok (.Ok {bearer_token := bearer, identity := ordinaryIdentity issuer jwtId}) := by
  rw [InternalAuthIssueProof.issue_exact issuer ordinaryRequest ordinary_domain]
  unfold issueOutcome
  rw [ordinary_constraints.1]
  unfold identityOutcome
  rw [provider]
  simp only [bind_ok, bind_tc_ok, ordinary_constraints.2.1]
  rw [clock]
  simp only [bind_ok, bind_tc_ok, ordinary_constraints.2.2.1, ite_true,
    ordinary_constraints.2.2.2.1, ordinary_constraints.2.2.2.2.2, ite_true]
  rw [observation.uuid_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.text_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.parse_observed]
  simp only [bind_ok, bind_tc_ok, parsed]
  change Aeneas.Std.bind
    (internal_auth.HubInternalTokenIssuer.sign_forwarded issuer
      (claimsValue (ordinaryIdentity issuer jwtId))) _ = _
  rw [signed]
  simp only [bind_ok, bind_tc_ok, ordinaryIdentity]

/-- The signer observation in ordinary_issued names these complete claims:
ordered scopes, both permissions, complete invocation/context, and source-capped
full identity expiry. This is value/dependency evidence, not a signing trace. -/
theorem ordinary_complete_fields (issuer : internal_auth.HubInternalTokenIssuer) (jwtId : identity.JwtId) :
    ClaimsExact (ordinaryIdentity issuer jwtId) (claimsValue (ordinaryIdentity issuer jwtId)) ∧
    (ordinaryIdentity issuer jwtId).expires_at = seedExpiresAt ∧
    Native.compareTimestamp requestedDeadline (ordinaryIdentity issuer jwtId).expires_at = .gt ∧
    (claimsValue (ordinaryIdentity issuer jwtId)).standard.scope = some "operator:read operator:use" ∧
    (claimsValue (ordinaryIdentity issuer jwtId)).authorization_details.val =
      [{kind := .Permissions, actions := ordinaryRequest.permissions}] := by
  refine ⟨claims_value_exact _, rfl, ordinary_constraints.2.2.2.2.1, rfl, ?_⟩
  simp [claimsValue, ordinaryIdentity, InternalAuthCorrectness.identityValue,
    ordinaryRequest, richIdentity]

/-- An exact first-foreign refusal even though later client/context validation
would also refuse. No provider, clock, UUID, parse or signer observation is needed. -/
theorem foreign_precedes_invalid_context (issuer : internal_auth.HubInternalTokenIssuer) :
    ¬ requestContextAccepted priorityRequest ∧
    internal_auth.HubInternalTokenIssuer.issue issuer priorityRequest =
      .ok (.Err (.PermissionOutsideAudience firstForeignPermission)) := by
  exact ⟨priority_constraints.2,
    InternalAuthIssueProof.foreign_permission_refused issuer priorityRequest
      priority_permission_domain firstForeignPermission priority_constraints.1⟩

#print axioms ordinary_issued
#print axioms ordinary_complete_fields
#print axioms foreign_precedes_invalid_context
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let nativeIssue := #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse,
    ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.sign_forwarded]
  let fields ← Lean.collectAxioms ``InternalAuthIssueWitnesses.ordinary_complete_fields
  unless (fields.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
    throwError "unreviewed complete public-issue fixture claims: {fields}"
  for declaration in #[``InternalAuthIssueWitnesses.ordinary_issued,
      ``InternalAuthIssueWitnesses.foreign_precedes_invalid_context] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !(standard ++ nativeIssue).contains axiomName)).isEmpty do
      throwError "unreviewed actual populated public-issue boundary in {declaration}: {found}"
end
end InternalAuthIssueWitnesses
