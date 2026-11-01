import SdkGrantDetailModel.Funs
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std SdkGrantDetailModel

namespace RepositoryGrantDropActionsWitness
noncomputable section

abbrev Grant := registration.grants.RepositoryGrant
abbrev Detail := registration.grants.RepositoryDetail

def repository : registration.grants.RepositoryId := "0123456789abcdef0123456789abcdef"
def actions : Finset registration.grants.RepositoryAction := {"write", "push-protected"}
def supplied : Grant := { repository := repository, actions := actions }
def detail : Detail := {
  kind := registration.grants.RepositoryDetailType.Repository,
  identifier := repository, actions := actions }
def emptyRead : Grant := { repository := repository, actions := ∅ }
def emptyDetail : Detail := { detail with actions := ∅ }

/-- Actual mutated projection, independent of the failed generic proof. -/
theorem mutant_drops_actions :
    registration.grants.RepositoryDetail.grant detail = .ok emptyRead := by
  simp [registration.grants.RepositoryDetail.grant,
    registration.grants.RepositoryId.Insts.CoreCloneClone.clone,
    alloc.string.String.Insts.CoreCloneClone.clone,
    alloc.collections.btree.set.BTreeSetTGlobal.new,
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone,
    Native.cloneList, detail, emptyRead]

/-- The source fault leaves the legitimate empty read projection accepted. -/
theorem empty_read_preserved :
    registration.grants.RepositoryDetail.grant emptyDetail = .ok emptyRead := by
  simp [registration.grants.RepositoryDetail.grant,
    registration.grants.RepositoryId.Insts.CoreCloneClone.clone,
    alloc.string.String.Insts.CoreCloneClone.clone,
    alloc.collections.btree.set.BTreeSetTGlobal.new,
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone,
    Native.cloneList, emptyDetail, detail, emptyRead]

theorem wrong_full_grant :
    registration.grants.RepositoryDetail.grant detail ≠ .ok supplied := by
  rw [mutant_drops_actions]
  intro same
  have whole := Result.ok_injective same
  have sets := congrArg (fun grant : Grant => grant.actions) whole
  have held : ("write" : registration.grants.RepositoryAction) ∈ supplied.actions := by
    simp [supplied, actions]
  have missing : ("write" : registration.grants.RepositoryAction) ∈ emptyRead.actions := by
    rw [sets]
    exact held
  simpa [emptyRead] using missing

#print axioms mutant_drops_actions
#print axioms empty_read_preserved
#print axioms wrong_full_grant
run_cmd do
  let allowed := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``mutant_drops_actions, ``empty_read_preserved, ``wrong_full_grant] do
    let found ← Lean.collectAxioms declaration
    let unexpected := found.filter (fun axiomName => !allowed.contains axiomName)
    unless unexpected.isEmpty do
      throwError "unexpected grant source-control axiom in {declaration}: {unexpected}"

end
end RepositoryGrantDropActionsWitness
