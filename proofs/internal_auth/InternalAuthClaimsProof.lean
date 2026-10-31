import InternalAuthClaimsExamples
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthValueLaws InternalAuthContextProof
open InternalAuthClaimsComponents InternalAuthClaimsExamples

namespace InternalAuthClaimsProof
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate
attribute [local simp] principal_clone context_clone delegation_clone
  alloc.string.String.Insts.CoreCloneClone.clone
  identity.HubProfileId.Insts.CoreCloneClone.clone identity.OpaqueTaskId.Insts.CoreCloneClone.clone
  veoveo_types.platform_names.ServerSlug.Insts.CoreCloneClone.clone
  veoveo_types.authority.InvocationAuthority.Insts.CoreCloneClone.clone

/-- Complete transparent actual claims constructor. NumericDates explicitly project
seconds; no whole-second lifetime, signature, Task liveness or identity authenticity
is inferred. Native set-order/UTF8/allocation refinement remains explicit. -/
theorem from_identity_exact (token : Identity) (domain : ClaimsTextDomain token) :
    internal_auth.AccessTokenClaims.from_identity token = .ok (claimsValue token) := by
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
      claimsValue, expectedInvocation, bind_ok, bind_tc_ok]
  | cons head tail =>
    cases acting : token.acting_server <;>
      simp [acting, empty, taskClone, core.option.Option.Insts.CoreIterTraitsCollectIntoIteratorTIntoIter.into_iter,
      option_vec_collect, option_clone_exact, core.option.Option.map,
      internal_auth.AccessTokenClaims.from_identity.closure_1.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerServerSlug.call_once,
      internal_auth.AccessTokenClaims.from_identity.closure_2.Insts.CoreOpsFunctionFnOnceTupleShared0ActingServerDeploymentId.call_once,
      claimsValue, expectedInvocation, bind_ok, bind_tc_ok]

theorem actual_claims_complete (token : Identity) (domain : ClaimsTextDomain token) :
    internal_auth.AccessTokenClaims.from_identity token ⦃ claims => ClaimsExact token claims ⦄ := by
  rw [from_identity_exact token domain]
  exact (WP.spec_ok _).mpr (claims_value_exact token)

/-- Complete actual populated result: two lexical scopes, two inherited actors
then the current actor, inherited Task, nonempty permissions and supplied context
payload fields. The context relation is proved separately in populated_inputs. -/
theorem rich_claims_constructed :
    internal_auth.AccessTokenClaims.from_identity richIdentity = .ok (claimsValue richIdentity) ∧
    ClaimsExact richIdentity (claimsValue richIdentity) ∧
    (claimsValue richIdentity).standard.scope = some "operator:read operator:use" ∧
    (claimsValue richIdentity).standard.act.act =
      some (.mk currentActor.client (some (.mk secondActor.client
        (some (.mk firstActor.client none))))) ∧
    (claimsValue richIdentity).authorization_details.val =
      [{kind := .Permissions, actions := richIdentity.permissions}] := by
  exact ⟨from_identity_exact richIdentity rich_domain, claims_value_exact richIdentity,
    rfl, rfl, by simp [claimsValue, richIdentity]⟩

/-- Empty permissions remain valid and emit no authorization detail, while the
Direct context, full actor metadata and nonempty ordered scope text are retained. -/
theorem empty_direct_claims_constructed :
    internal_auth.AccessTokenClaims.from_identity emptyDirectIdentity =
      .ok (claimsValue emptyDirectIdentity) ∧
    ClaimsExact emptyDirectIdentity (claimsValue emptyDirectIdentity) ∧
    (claimsValue emptyDirectIdentity).authorization_details.val = [] ∧
    (claimsValue emptyDirectIdentity).standard.scope = some "operator:read operator:use" ∧
    (claimsValue emptyDirectIdentity).standard.act.act = none := by
  exact ⟨from_identity_exact emptyDirectIdentity empty_direct_domain,
    claims_value_exact emptyDirectIdentity, by simp [claimsValue, emptyDirectIdentity], rfl, rfl⟩

#print axioms from_identity_exact
#print axioms actual_claims_complete
#print axioms rich_claims_constructed
#print axioms empty_direct_claims_constructed
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthClaimsProof.from_identity_exact,
      ``InternalAuthClaimsProof.actual_claims_complete, ``InternalAuthClaimsProof.rich_claims_constructed,
      ``InternalAuthClaimsProof.empty_direct_claims_constructed] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed complete claims boundary in {declaration}: {found}"
end
end InternalAuthClaimsProof
