import SdkGrantDetailModel.Funs
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std SdkGrantDetailModel

namespace RepositoryGrantProof
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

abbrev Grant := registration.grants.RepositoryGrant
abbrev Detail := registration.grants.RepositoryDetail
abbrev Action := registration.grants.RepositoryAction

/-- The set clone law follows the actual element-clone traversal, rather than
assuming the desired result of a whole native set operation. -/
theorem clone_list_exact {T : Type} (clone : T → Result T)
    (law : ∀ value, clone value = .ok value) (values : List T) :
    Native.cloneList clone values = .ok values := by
  induction values with
  | nil => rfl
  | cons head tail recursive => simp [Native.cloneList, law, recursive]

theorem set_clone_exact {T A : Type} (clone : Aeneas.Std.core.clone.Clone T)
    (law : ∀ value, clone.clone value = .ok value)
    (allocator : core.alloc.AllocatorClone A)
    (values : alloc.collections.btree.set.BTreeSet T A) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone clone allocator values =
      .ok values := by
  simp [alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone,
    clone_list_exact clone.clone law]

theorem repository_id_clone_exact (value : registration.grants.RepositoryId) :
    registration.grants.RepositoryId.Insts.CoreCloneClone.clone value = .ok value := by
  simp [registration.grants.RepositoryId.Insts.CoreCloneClone.clone,
    alloc.string.String.Insts.CoreCloneClone.clone]

theorem action_clone_exact (value : Action) :
    registration.grants.RepositoryAction.Insts.CoreCloneClone.clone value = .ok value := by
  simp [registration.grants.RepositoryAction.Insts.CoreCloneClone.clone,
    alloc.string.String.Insts.CoreCloneClone.clone]

theorem permissions_new_exact
    (actions : alloc.collections.btree.set.BTreeSet
      veoveo_types.permission_names.PermissionName Aeneas.Std.alloc.alloc.Global) :
    registration.grants.PermissionsDetail.new actions =
      .ok { kind := registration.grants.PermissionsDetailType.Permissions, actions := actions } := by
  rfl

theorem repository_new_exact (grant : Grant) :
    registration.grants.RepositoryDetail.new grant = .ok {
      kind := registration.grants.RepositoryDetailType.Repository,
      identifier := grant.repository, actions := grant.actions } := by
  rfl

theorem projection_exact (detail : Detail) :
    registration.grants.RepositoryDetail.grant detail = .ok {
      repository := detail.identifier, actions := detail.actions } := by
  have cloned (actions : alloc.collections.btree.set.BTreeSet Action
      Aeneas.Std.alloc.alloc.Global) :
      alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone
        registration.grants.RepositoryAction.Insts.CoreCloneClone
        alloc.alloc.Global.Insts.CoreAllocAllocatorClone actions = .ok actions :=
    set_clone_exact registration.grants.RepositoryAction.Insts.CoreCloneClone
      action_clone_exact alloc.alloc.Global.Insts.CoreAllocAllocatorClone actions
  simp only [registration.grants.RepositoryDetail.grant, repository_id_clone_exact,
    cloned, alloc.collections.btree.set.BTreeSetTGlobal.new, bind_tc_eq, bind_ok]

/-- Complete actual new→grant call chain, including an empty read grant. -/
theorem roundtrip_exact (grant : Grant) :
    (do let detail ← registration.grants.RepositoryDetail.new grant; registration.grants.RepositoryDetail.grant detail) = .ok grant := by
  rw [repository_new_exact]
  simp only [bind_ok, bind_tc_eq]
  simpa only [] using projection_exact
    { kind := registration.grants.RepositoryDetailType.Repository,
      identifier := grant.repository, actions := grant.actions }

/-- An explicit legal empty-action specialization supplies no nonempty premise. -/
theorem empty_read_roundtrip (repository : registration.grants.RepositoryId) :
    (do let detail ← registration.grants.RepositoryDetail.new { repository := repository, actions := ∅ }
        registration.grants.RepositoryDetail.grant detail) = .ok { repository := repository, actions := ∅ } := by
  exact roundtrip_exact { repository := repository, actions := ∅ }

/-- Distinct complete grants cannot collapse under the actual constructor. -/
theorem new_injective (left right : Grant)
    (same : registration.grants.RepositoryDetail.new left = registration.grants.RepositoryDetail.new right) : left = right := by
  have observed := congrArg (fun operation : Result Detail => do
    let detail ← operation
    registration.grants.RepositoryDetail.grant detail) same
  rw [roundtrip_exact, roundtrip_exact] at observed
  exact Result.ok_injective observed

#print axioms clone_list_exact
#print axioms set_clone_exact
#print axioms repository_id_clone_exact
#print axioms action_clone_exact
#print axioms permissions_new_exact
#print axioms repository_new_exact
#print axioms projection_exact
#print axioms roundtrip_exact
#print axioms empty_read_roundtrip
#print axioms new_injective

run_cmd do
  let allowed := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``registration.grants.PermissionsDetail.new,
      ``registration.grants.RepositoryDetail.new, ``registration.grants.RepositoryDetail.grant,
      ``clone_list_exact, ``set_clone_exact,
      ``repository_id_clone_exact, ``action_clone_exact, ``permissions_new_exact,
      ``repository_new_exact, ``projection_exact, ``roundtrip_exact,
      ``empty_read_roundtrip, ``new_injective] do
    let found ← Lean.collectAxioms declaration
    let unexpected := found.filter (fun axiomName => !allowed.contains axiomName)
    unless unexpected.isEmpty do
      throwError "unexpected repository-grant axiom in {declaration}: {unexpected}"

end
end RepositoryGrantProof
