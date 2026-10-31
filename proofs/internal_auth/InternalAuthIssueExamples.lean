import InternalAuthIssueSpecification
import InternalAuthClaimsExamples
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof InternalAuthContextExamples
open InternalAuthIdentityExamples InternalAuthIdentityProof InternalAuthClaimsComponents
open InternalAuthClaimsExamples InternalAuthIssueSpecification

namespace InternalAuthIssueExamples
noncomputable section

/-- Independent populated issuer inputs, reusable without the public-issue
correctness theorem. Context metadata is a typed input, not authenticity evidence. -/
def ordinaryRequest : Request := {
  profile := emptyDirectIdentity.profile
  server := emptyDirectIdentity.server
  deployment := emptyDirectIdentity.deployment
  audience := emptyDirectIdentity.audience
  client := emptyDirectIdentity.client
  actor := emptyDirectIdentity.actor
  authority := emptyDirectIdentity.authority
  permissions := richIdentity.permissions
  request_context := emptyDirectIdentity.request_context
  acting_server := none
  acting_task := none
  delegation_context := none
  expires_at := requestedDeadline }

def firstForeignPermission : veoveo_types.permission_names.PermissionName := ⟨"other.inspect"⟩

/-- The client mismatch would invalidate context if issuance reached that stage.
The foreign permission must win before any provider/time/identifier observation. -/
def priorityRequest : Request := { ordinaryRequest with
  client := ⟨"other-client"⟩
  permissions := ⟨[⟨"operator.inspect"⟩, firstForeignPermission, ⟨"third.inspect"⟩]⟩ }

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem context_domain : ContextTextDomain
    (directContext seedContext (some seedAuthenticatedAt))
    (userPrincipal (some seedAuthenticatedAt)) := by
  constructor <;> apply short_text <;>
    dsimp only [directContext, inputContext, userPrincipal] <;> decide +kernel

private theorem byte_guard :
    (fun byte : U8 => decide (byte.val ≠ 46)) =
      (fun byte : U8 => !(byte.val == 46)) := by
  funext byte
  by_cases same : byte.val = 46 <;> simp [same]

private theorem owned_iff (permission : veoveo_types.permission_names.PermissionName)
    (server : veoveo_types.platform_names.ServerSlug) :
    Owned permission server ↔
      (utf8Bytes permission.value).takeWhile (fun byte => !(byte.val == 46)) =
        utf8Bytes server.value := by
  unfold Owned
  rw [byte_guard]

private theorem inspect_owned : Owned ⟨"operator.inspect"⟩ ⟨"operator"⟩ := by
  apply (owned_iff _ _).mpr
  decide +kernel

private theorem operate_owned : Owned ⟨"operator.operate"⟩ ⟨"operator"⟩ := by
  apply (owned_iff _ _).mpr
  decide +kernel

private theorem other_foreign : ¬ Owned firstForeignPermission ⟨"operator"⟩ := by
  rw [owned_iff]
  decide +kernel

/-- Physical text bounds only; none is an admission or own-output premise. -/
theorem ordinary_domain : IssueTextDomain ordinaryRequest := by
  refine ⟨⟨?_, ?_⟩, ?_, empty_direct_domain.1,
    empty_direct_domain.2.1, empty_direct_domain.2.2.1,
    empty_direct_domain.2.2.2⟩
  · apply short_text; decide +kernel
  · intro permission member
    change permission ∈ ([⟨"operator.inspect"⟩, ⟨"operator.operate"⟩] : List _) at member
    simp only [List.mem_cons, List.not_mem_nil, or_false] at member
    rcases member with rfl | rfl <;> apply short_text <;> decide +kernel
  · intro context equality
    change some (directContext seedContext (some seedAuthenticatedAt)) = some context at equality
    cases Option.some.inj equality
    exact context_domain

theorem priority_permission_domain : PermissionTextDomain priorityRequest := by
  refine ⟨ordinary_domain.permission_text.server, ?_⟩
  intro permission member
  change permission ∈ ([⟨"operator.inspect"⟩, ⟨"other.inspect"⟩, ⟨"third.inspect"⟩] : List _) at member
  simp only [List.mem_cons, List.not_mem_nil, or_false] at member
  rcases member with rfl | rfl | rfl <;> apply short_text <;> decide +kernel

theorem ordinary_constraints :
    firstForeign ordinaryRequest.permissions.values ordinaryRequest.server = none ∧
    delegationError ordinaryRequest = none ∧
    requestContextAccepted ordinaryRequest ∧
    effectiveDeadline ordinaryRequest = seedExpiresAt ∧
    Native.compareTimestamp requestedDeadline seedExpiresAt = .gt ∧
    Native.compareTimestamp seedExpiresAt seedIssuedAt = .gt := by
  refine ⟨?_, rfl, ?_, ?_, deadline_order.1, deadline_order.2.2.1⟩
  · change firstForeign [⟨"operator.inspect"⟩, ⟨"operator.operate"⟩] ⟨"operator"⟩ = none
    simp [firstForeign, inspect_owned, operate_owned]
  · change ContextAccepted (directContext seedContext (some seedAuthenticatedAt))
      (userPrincipal (some seedAuthenticatedAt)) (directAuthority seedAuthority) ∧ _
    exact ⟨direct_accepted seedContext seedAuthority (some seedAuthenticatedAt), rfl⟩
  · simp [effectiveDeadline, delegationDeadline, ordinaryRequest, emptyDirectIdentity,
      directContext, inputContext, seedContext, earlier, deadline_order.1]

theorem priority_constraints :
    firstForeign priorityRequest.permissions.values priorityRequest.server = some firstForeignPermission ∧
    ¬ requestContextAccepted priorityRequest := by
  constructor
  · change firstForeign [⟨"operator.inspect"⟩, firstForeignPermission, ⟨"third.inspect"⟩]
      ⟨"operator"⟩ = some firstForeignPermission
    simp [firstForeign, inspect_owned, other_foreign]
  · intro accepted
    have clients := accepted.2
    change (⟨"console"⟩ : veoveo_types.platform_names.OAuthClientId) = ⟨"other-client"⟩ at clients
    have different : ("console" : String) ≠ "other-client" := by decide +kernel
    exact different (congrArg (fun value : veoveo_types.platform_names.OAuthClientId => value.value) clients)

#print axioms ordinary_domain
#print axioms priority_permission_domain
#print axioms ordinary_constraints
#print axioms priority_constraints
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthIssueExamples.ordinary_domain,
      ``InternalAuthIssueExamples.priority_permission_domain,
      ``InternalAuthIssueExamples.ordinary_constraints,
      ``InternalAuthIssueExamples.priority_constraints] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed independent issuer fixture boundary in {declaration}: {found}"
end
end InternalAuthIssueExamples
