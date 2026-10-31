import InternalAuthClaimsExamples
import Lean.Util.CollectAxioms

-- Test-only: raw permission-detail inversion graph, fresh unaffected Components
-- and independent Examples. Never import failing ClaimsProof or its OLean.
open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthValueLaws InternalAuthContextProof
open InternalAuthClaimsComponents InternalAuthClaimsExamples

namespace PermissionDetailInversionWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate
attribute [local simp] principal_clone context_clone delegation_clone
  alloc.string.String.Insts.CoreCloneClone.clone
  identity.HubProfileId.Insts.CoreCloneClone.clone identity.OpaqueTaskId.Insts.CoreCloneClone.clone
  veoveo_types.platform_names.ServerSlug.Insts.CoreCloneClone.clone
  veoveo_types.authority.InvocationAuthority.Insts.CoreCloneClone.clone

/-- Independent complete wrong value: every normative field is retained, except
that the deliberate source fault reverses presence of the permission detail. -/
def wrongClaimsValue (token : Identity) : internal_auth.AccessTokenClaims := {
  claimsValue token with
  authorization_details := alloc.vec.Vec.from (if token.permissions.values = [] then
    [{kind := .Permissions, actions := token.permissions}] else []) (by
      split <;> simp only [List.length_nil, List.length_singleton] <;> scalar_tac) }

/-- Actual mutated complete result, derived from primitive operations and fresh
unaffected component proofs; no own-function-success premise. -/
theorem mutant_claims_exact (token : Identity) (domain : ClaimsTextDomain token) :
    internal_auth.AccessTokenClaims.from_identity token = .ok (wrongClaimsValue token) := by
  unfold internal_auth.AccessTokenClaims.from_identity
  simp only [registration.declaration.ResourceIndicator.as_str, url.Url.as_str]
  rw [encode_exact _ domain.2.1]
  simp only [bind_ok, bind_tc_ok, core.option.Option.as_ref]
  rw [standard_claims_exact token domain]
  simp only [bind_ok, bind_tc_ok, alloc.collections.btree.set.BTreeSet.is_empty]
  have permissionsClone : alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone
      veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone token.permissions = .ok token.permissions := by
    apply set_clone_exact
    intro value
    rfl
  have taskClone : core.option.Option.Insts.CoreCloneClone.clone
      identity.OpaqueTaskId.Insts.CoreCloneClone token.acting_task = .ok token.acting_task := by
    apply option_clone_exact
    intro taskValue
    simp [identity.OpaqueTaskId.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
  simp only [core.bool.Bool.then,
    internal_auth.AccessTokenClaims.from_identity.closure.Insts.CoreOpsFunctionFnOnceTuplePermissionsDetail.call_once,
    permissionsClone, registration.grants.PermissionsDetail.new, bind_ok, bind_tc_ok]
  cases empty : token.permissions.values with
  | nil =>
    cases acting : token.acting_server <;>
      simp [acting, empty, taskClone, core.option.Option.Insts.CoreIterTraitsCollectIntoIteratorTIntoIter.into_iter,
      option_vec_collect, option_clone_exact, core.option.Option.map,
      internal_auth.AccessTokenClaims.from_identity.closure_1.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerServerSlug.call_once,
      internal_auth.AccessTokenClaims.from_identity.closure_2.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerDeploymentId.call_once,
      wrongClaimsValue, claimsValue, expectedInvocation, bind_ok, bind_tc_ok]
  | cons head tail =>
    cases acting : token.acting_server <;>
      simp [acting, empty, taskClone, core.option.Option.Insts.CoreIterTraitsCollectIntoIteratorTIntoIter.into_iter,
      option_vec_collect, option_clone_exact, core.option.Option.map,
      internal_auth.AccessTokenClaims.from_identity.closure_1.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerServerSlug.call_once,
      internal_auth.AccessTokenClaims.from_identity.closure_2.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerDeploymentId.call_once,
      wrongClaimsValue, claimsValue, expectedInvocation, bind_ok, bind_tc_ok]

private theorem wrong_rich_refused_by_spec :
    ¬ ClaimsExact richIdentity (wrongClaimsValue richIdentity) := by
  intro accepted
  rcases accepted with ⟨_, _, _, _, _, _, _, _, _, _, _, _, details, _⟩
  simpa [wrongClaimsValue, claimsValue, richIdentity] using details

/-- Populated closed input loses both supplied actions, although all other complete
claims fields remain the independent normative values. This is a deliberate fault,
not a claim of a production defect or current-policy grant. -/
theorem rich_two_actions_dropped :
    internal_auth.AccessTokenClaims.from_identity richIdentity = .ok (wrongClaimsValue richIdentity) ∧
    (wrongClaimsValue richIdentity).authorization_details.val = [] ∧
    richIdentity.permissions.values = [⟨"operator.inspect"⟩, ⟨"operator.operate"⟩] ∧
    ¬ ClaimsExact richIdentity (wrongClaimsValue richIdentity) := by
  exact ⟨mutant_claims_exact richIdentity rich_domain,
    by simp [wrongClaimsValue, richIdentity], rfl, wrong_rich_refused_by_spec⟩

/-- The same inverted native guard also emits a spurious empty detail for the
independently valid empty-permission input. No native parser/signature is assumed. -/
theorem empty_spurious_detail :
    internal_auth.AccessTokenClaims.from_identity emptyDirectIdentity =
      .ok (wrongClaimsValue emptyDirectIdentity) ∧
    (wrongClaimsValue emptyDirectIdentity).authorization_details.val =
      [{kind := .Permissions, actions := emptyDirectIdentity.permissions}] ∧
    ¬ ClaimsExact emptyDirectIdentity (wrongClaimsValue emptyDirectIdentity) := by
  refine ⟨mutant_claims_exact emptyDirectIdentity empty_direct_domain,
    by simp [wrongClaimsValue, emptyDirectIdentity], ?_⟩
  intro accepted
  rcases accepted with ⟨_, _, _, _, _, _, _, _, _, _, _, _, details, _⟩
  simpa [wrongClaimsValue, claimsValue, emptyDirectIdentity] using details

#print axioms mutant_claims_exact
#print axioms rich_two_actions_dropped
#print axioms empty_spurious_detail
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``PermissionDetailInversionWitness.mutant_claims_exact,
      ``PermissionDetailInversionWitness.rich_two_actions_dropped,
      ``PermissionDetailInversionWitness.empty_spurious_detail] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed permission-detail witness boundary in {declaration}: {found}"
end
end PermissionDetailInversionWitness
