import RequestCompositionSdk.Funs
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk

namespace InternalAuthValueLaws
noncomputable section
local instance (p : Prop) : Decidable p := Classical.propDecidable p
universe u v

/-- Finite callback laws for the operational primitives, not an alternative
implementation of SDK claims or context validation. -/
theorem clone_list_exact {T : Type u} (clone : T → Result T)
    (law : ∀ value, clone value = .ok value) (values : List T) :
    Native.cloneList clone values = .ok values := by
  induction values with
  | nil => rfl
  | cons head tail ih =>
    simp [Native.cloneList, law, ih, bind_ok, bind_tc_ok]

theorem list_eq_exact {T : Type u} [DecidableEq T]
    (equal : T → T → Result Bool)
    (law : ∀ left right, equal left right = .ok (decide (left = right)))
    (left right : List T) :
    Native.listEq equal left right = .ok (decide (left = right)) := by
  induction left generalizing right with
  | nil => cases right <;> simp [Native.listEq]
  | cons head tail ih =>
    cases right with
    | nil => simp [Native.listEq]
    | cons other rest =>
      rw [Native.listEq, law]
      by_cases same : head = other
      · subst other
        simp [ih, bind_ok, bind_tc_ok]
      · simp [same, bind_ok, bind_tc_ok]

theorem set_clone_exact {T : Type u} {A : Type v}
    (clone : core.clone.Clone T) (allocator : core.alloc.AllocatorClone A)
    (law : ∀ value, clone.clone value = .ok value)
    (values : alloc.collections.btree.set.BTreeSet T A) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone clone allocator values =
      .ok values := by
  unfold alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone
  rw [clone_list_exact clone.clone law]
  cases values
  simp only [bind_ok, bind_tc_ok]

theorem set_eq_exact {T : Type u} {A : Type v} [DecidableEq T]
    (equal : core.cmp.PartialEq T T) (allocator : core.alloc.AllocatorClone A)
    (law : ∀ left right, equal.eq left right = .ok (decide (left = right)))
    (left right : alloc.collections.btree.set.BTreeSet T A) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      equal allocator left right = .ok (decide (left = right)) := by
  cases left with
  | mk left =>
    cases right with
    | mk right =>
      unfold alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      by_cases lengths : left.length = right.length
      · simp [lengths, list_eq_exact equal.eq law]
      · have different : left ≠ right := by
          intro same
          exact lengths (congrArg List.length same)
        simp [lengths, different]

theorem option_clone_exact {T : Type u} (clone : core.clone.Clone T)
    (law : ∀ value, clone.clone value = .ok value) (value : Option T) :
    core.option.Option.Insts.CoreCloneClone.clone clone value = .ok value := by
  cases value <;>
    simp [core.option.Option.Insts.CoreCloneClone.clone, law, bind_ok, bind_tc_ok]

theorem option_eq_exact {T : Type u} [DecidableEq T]
    (equal : core.cmp.PartialEq T T)
    (law : ∀ left right, equal.eq left right = .ok (decide (left = right)))
    (left right : Option T) :
    core.option.Option.Insts.CoreCmpPartialEqOption.eq equal left right =
      .ok (decide (left = right)) := by
  cases left <;> cases right <;>
    simp [core.option.Option.Insts.CoreCmpPartialEqOption.eq, law, bind_ok, bind_tc_ok]

#print axioms clone_list_exact
#print axioms list_eq_exact
#print axioms set_clone_exact
#print axioms set_eq_exact
#print axioms option_clone_exact
#print axioms option_eq_exact

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[
      ``InternalAuthValueLaws.clone_list_exact,
      ``InternalAuthValueLaws.list_eq_exact,
      ``InternalAuthValueLaws.set_clone_exact,
      ``InternalAuthValueLaws.set_eq_exact,
      ``InternalAuthValueLaws.option_clone_exact,
      ``InternalAuthValueLaws.option_eq_exact] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed finite native value law in {declaration}: {found}"

end
end InternalAuthValueLaws
