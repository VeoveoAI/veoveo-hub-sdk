import InternalAuthContextExamples
import InternalAuthValueLaws
import Lean.Util.CollectAxioms

-- Deliberately no import of InternalAuthContextProof or ContextWitnesses.
-- Run only against this control's untouched raw mutant outputs in a fresh graph.
open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples

namespace DirectActorBypassWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/-- Same complete user identity except one additional role. Native set traversal
is unique and lexical; this is not a claim about downstream granted permissions. -/
def addedRoleActor : Principal := {
  userPrincipal none with roles := ⟨[⟨"operator"⟩, ⟨"superuser"⟩]⟩ }

theorem only_roles_differ :
    { addedRoleActor with roles := (userPrincipal none).roles } = userPrincipal none ∧
    addedRoleActor.roles.values = [⟨"operator"⟩, ⟨"superuser"⟩] := by
  exact ⟨rfl, rfl⟩

theorem independent_refusal :
    ¬ ContextAccepted (directContext seedContext none) addedRoleActor
      (directAuthority seedAuthority) := by
  intro accepted
  have actorEquality : addedRoleActor = (directContext seedContext none).principal :=
    accepted.2.2.2.2.2.2.2.1.1
  have rolesEquality := congrArg (fun principal : Principal => principal.roles.values) actorEquality
  simp [addedRoleActor, directContext, inputContext, userPrincipal] at rolesEquality

/-- Actual wrong acceptance from the extracted Rust with only Direct actor
comparison removed. No validator-success or desired-output premise is used. -/
theorem mutant_accepts_added_role :
    internal_auth.request.HubRequestContext.validate_for
      (directContext seedContext none) addedRoleActor (directAuthority seedAuthority) =
      Result.ok (.Ok ()) := by
  simp [addedRoleActor,
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
      (directContext seedContext none) addedRoleActor (directAuthority seedAuthority) ≠
      Result.ok (if ContextAccepted (directContext seedContext none) addedRoleActor
        (directAuthority seedAuthority) then core.result.Result.Ok ()
        else .Err internal_auth.InternalTokenError.InvalidRequestContext) := by
  rw [mutant_accepts_added_role, if_neg independent_refusal]
  intro impossible
  simp only [Result.ok.injEq] at impossible
  cases impossible

#print axioms only_roles_differ
#print axioms independent_refusal
#print axioms mutant_accepts_added_role
#print axioms violates_exact_contract

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``DirectActorBypassWitness.only_roles_differ,
      ``DirectActorBypassWitness.independent_refusal,
      ``DirectActorBypassWitness.mutant_accepts_added_role,
      ``DirectActorBypassWitness.violates_exact_contract] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed direct-actor fault witness ({declaration}): {found}"

end
end DirectActorBypassWitness
