import InternalAuthContextExamples
import InternalAuthValueLaws
import Lean.Util.CollectAxioms

-- Deliberately no import of InternalAuthContextProof or ContextWitnesses.
-- Run only against this control's untouched raw mutant outputs in a fresh graph.
open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples

namespace AutomatedKindRefusalWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/-- Independent admission remains true for the populated Service fixture. -/
theorem independent_admission :
    ContextAccepted (automatedContext seedContext none) (automatedPrincipal none)
      (automatedAuthority seedAuthority) := by
  exact automated_accepted seedContext seedAuthority none

/-- Actual wrong refusal from the extracted Rust with only Automated's required
kind changed to User. No validator-error premise or failed theorem is imported. -/
theorem mutant_refuses_service :
    internal_auth.request.HubRequestContext.validate_for
      (automatedContext seedContext none) (automatedPrincipal none)
      (automatedAuthority seedAuthority) =
      Result.ok (.Err internal_auth.InternalTokenError.InvalidRequestContext) := by
  simp [
    internal_auth.request.HubRequestContext.validate_for,
    directContext, directAuthority, automatedContext, automatedAuthority,
    inputContext, inputAuthority, userPrincipal, automatedPrincipal,
    veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq,
    veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq,
    veoveo_types.identity.WorkContextId.Insts.CoreCmpPartialEqWorkContextId.eq,
    veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId.eq,
    veoveo_types.identity.PrincipalId.Insts.CoreCmpPartialEqPrincipalId.eq,
    veoveo_types.identity.GroupId.Insts.CoreCmpPartialEqGroupId.eq,
    veoveo_types.identity.RoleId.Insts.CoreCmpPartialEqRoleId.eq,
    veoveo_types.identity.DataLabelId.Insts.CoreCmpPartialEqDataLabelId.eq,
    veoveo_types.names.ScopeName.Insts.CoreCmpPartialEqScopeName.eq,
    veoveo_types.provenance.InvocationMode.Insts.CoreCmpPartialEqInvocationMode.eq,
    veoveo_types.provenance.InvocationProvenance.mode,
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq,
    Native.listEq, core.option.Option.as_ref, core.option.Option.is_some,
    core.option.Option.Insts.CoreCmpPartialEqOption.eq, core.cmp.PartialEqShared,
    core.cmp.impls.PartialEqShared.ne, core.cmp.PartialEq.ne.trait_default,
    core.cmp.PartialEq.ne.default,
    identity.Principal.Insts.CoreCmpPartialEqPrincipal.eq,
    identity.PrincipalKind.Insts.CoreCmpPartialEqPrincipalKind.eq,
    read_discriminant, identity.PrincipalKind.read_discriminant,
    uncurry_apply_pair, bind_ok, bind_tc_ok]

theorem violates_exact_contract :
    internal_auth.request.HubRequestContext.validate_for
      (automatedContext seedContext none) (automatedPrincipal none)
      (automatedAuthority seedAuthority) ≠
      Result.ok (if ContextAccepted (automatedContext seedContext none)
        (automatedPrincipal none) (automatedAuthority seedAuthority)
        then core.result.Result.Ok ()
        else .Err internal_auth.InternalTokenError.InvalidRequestContext) := by
  rw [mutant_refuses_service, if_pos independent_admission]
  intro impossible
  simp only [Result.ok.injEq] at impossible
  cases impossible

#print axioms independent_admission
#print axioms mutant_refuses_service
#print axioms violates_exact_contract

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``AutomatedKindRefusalWitness.independent_admission,
      ``AutomatedKindRefusalWitness.mutant_refuses_service,
      ``AutomatedKindRefusalWitness.violates_exact_contract] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed automated-kind fault witness ({declaration}): {found}"

end
end AutomatedKindRefusalWitness
