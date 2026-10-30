import SdkAuthorizationProof

open Aeneas Aeneas.Std
open sdk_authorization_proof

namespace ResolutionProof

/- Both methods used by production must terminate and implement full equality;
this is not equality of permission() projections that erase Authenticated. -/
def LawfulEq {A : Type} [DecidableEq A] (eq : core.cmp.PartialEq A A) : Prop :=
  (∀ left right, eq.eq left right = .ok (decide (left = right))) ∧
  (∀ left right, eq.ne left right = .ok (decide (left ≠ right)))

structure Enumeration (C I : Type) where
  remaining : C → List I

/- Native next enumerates RAW entries in their original order. No matcher result
or authorized-candidate projection is included in this finite progress law. -/
def LawfulCursor {C I : Type} (ops : decision.Cursor C I) (model : Enumeration C I) : Prop :=
  ∀ cursor, ops.next cursor ⦃ result =>
    match model.remaining cursor with
    | [] => result.1 = none ∧ model.remaining result.2 = []
    | entry :: tail => result.1 = some entry ∧ model.remaining result.2 = tail ⦄

/- The operational matcher result is composed with the extracted parsed-view
algorithm later. No desired authorization semantics is assumed in this lemma. -/
def LawfulMatcher {M T : Type} (ops : decision.Matcher M T) (matcher : M)
    (evaluate : T → Bool) : Prop :=
  ∀ template, ops.«matches» matcher template = .ok (evaluate template)

def matching {T A : Type} (evaluate : T → Bool) (entries : List (T × A)) : List A :=
  entries.filterMap fun (template, access) => if evaluate template then some access else none

@[simp] theorem matching_nil {T A : Type} (evaluate : T → Bool) : matching evaluate ([] : List (T × A)) = [] := rfl

@[simp] theorem matching_cons {T A : Type} (evaluate : T → Bool)
    (template : T) (access : A) (tail : List (T × A)) :
    matching evaluate ((template, access) :: tail) =
      if evaluate template then access :: matching evaluate tail else matching evaluate tail := by
  cases matched : evaluate template <;> simp [matching, matched]

noncomputable section

/- Mathematical agreement, not a second Rust resolver. Multiple equal complete
access values agree; zero matches or any disagreement gives no access. -/
def finish {A : Type} (first : Option A) (hits : List A) : Option A := by
  classical
  exact match first with
  | some access => if ∀ other ∈ hits, other = access then some access else none
  | none => match hits with
    | [] => none
    | access :: rest => if ∀ other ∈ rest, other = access then some access else none

@[simp] theorem finish_nil {A : Type} (first : Option A) : finish first [] = first := by
  cases first <;> simp [finish]

@[simp] theorem finish_none_cons {A : Type} (access : A) (rest : List A) :
    finish none (access :: rest) = finish (some access) rest := rfl

@[simp] theorem finish_some_cons {A : Type} [DecidableEq A] (first access : A) (rest : List A) :
    finish (some first) (access :: rest) =
      if access = first then finish (some first) rest else none := by
  classical
  by_cases same : access = first <;> simp [finish, same]

theorem finish_success_iff {A : Type} (hits : List A) (output : A) :
    finish none hits = some output ↔ ∃ tail, hits = output :: tail ∧
      ∀ other ∈ tail, other = output := by
  classical
  cases hits with
  | nil => simp [finish]
  | cons access tail =>
    constructor
    · intro accepted
      change (if ∀ other ∈ tail, other = access then some access else none) = some output at accepted
      by_cases agreed : ∀ other ∈ tail, other = access
      · rw [if_pos agreed] at accepted
        cases accepted
        exact ⟨tail, rfl, agreed⟩
      · rw [if_neg agreed] at accepted
        cases accepted
    · rintro ⟨rest, same, agreed⟩
      cases same
      change (if ∀ other ∈ tail, other = output then some output else none) = some output
      rw [if_pos agreed]

section Resolver
variable {T A C M : Type} [DecidableEq A]
variable (eq : core.cmp.PartialEq A A) (eqLaw : LawfulEq eq)
variable (cursor : decision.Cursor C (T × A)) (model : Enumeration C (T × A))
variable (cursorLaw : LawfulCursor cursor model)
variable (matcherOps : decision.Matcher M T) (matcher : M) (evaluate : T → Bool)
variable (matcherLaw : LawfulMatcher matcherOps matcher evaluate)

include eqLaw cursorLaw matcherLaw in
/- Total resolution from an arbitrary recorded first match, over all raw entries.
The cursor's remaining suffix can only shrink; termination uses its finite length. -/
theorem resolve_loop_correct (candidates : C) (first : Option A) :
    decision.resolve_loop eq cursor matcherOps candidates matcher first ⦃ result =>
      result.1 = finish first (matching evaluate (model.remaining candidates)) ∧
      (model.remaining result.2).length ≤ (model.remaining candidates).length ⦄ := by
  classical
  unfold decision.resolve_loop
  rw [loop]
  unfold decision.resolve_loop.body
  dsimp only
  step with (cursorLaw candidates) as ⟨pair, nextSpec⟩
  rcases pair with ⟨entry, next⟩
  cases remaining : model.remaining candidates with
  | nil =>
    simp only [remaining] at nextSpec
    rcases nextSpec with ⟨rfl, suffix⟩
    simp [remaining, suffix]
  | cons head tail =>
    simp only [remaining] at nextSpec
    rcases nextSpec with ⟨rfl, suffix⟩
    rcases head with ⟨template, access⟩
    unfold LawfulMatcher at matcherLaw
    simp only [matcherLaw, bind_ok]
    cases matched : evaluate template with
    | false =>
      have recursive := resolve_loop_correct next first
      apply WP.spec_mono (by simpa [decision.resolve_loop, decision.resolve_loop.body, matcherLaw, eqLaw.2, matched] using recursive)
      intro result resultSpec
      refine ⟨?_, ?_⟩
      · simpa [remaining, matched, suffix] using resultSpec.1
      · have bound := resultSpec.2
        rw [suffix] at bound
        simpa [remaining] using Nat.le_trans bound (Nat.le_succ tail.length)
    | true =>
      cases first with
      | none =>
        have recursive := resolve_loop_correct next (some access)
        apply WP.spec_mono (by simpa [decision.resolve_loop, decision.resolve_loop.body, matcherLaw, eqLaw.2, matched] using recursive)
        intro result resultSpec
        refine ⟨?_, ?_⟩
        · simpa [remaining, matched, suffix] using resultSpec.1
        · have bound := resultSpec.2
          rw [suffix] at bound
          simpa [remaining] using Nat.le_trans bound (Nat.le_succ tail.length)
      | some previous =>
        by_cases same : access = previous
        · simp [eqLaw.2, same]
          have recursive := resolve_loop_correct next (some previous)
          apply WP.spec_mono (by simpa [decision.resolve_loop, decision.resolve_loop.body, matcherLaw, eqLaw.2, matched] using recursive)
          intro result resultSpec
          refine ⟨?_, ?_⟩
          · simpa [remaining, matched, suffix, same] using resultSpec.1
          · have bound := resultSpec.2
            rw [suffix] at bound
            simpa [remaining] using Nat.le_trans bound (Nat.le_succ tail.length)
        · simp [eqLaw.2, same, remaining, matched, suffix]
termination_by (model.remaining candidates).length
decreasing_by all_goals simp_all

/- The exact fastpath is independent of all enumeration/matcher behavior,
including implementations which would fail or diverge if invoked. -/
theorem exact_without_visits (copy : core.marker.Copy A) (access : A) (candidates : C) :
    decision.resolve eq copy cursor matcherOps (some access) candidates matcher =
      .ok (some access, candidates) := rfl

include eqLaw cursorLaw matcherLaw in
theorem resolve_correct (copy : core.marker.Copy A) (exact : Option A) (candidates : C) :
    decision.resolve eq copy cursor matcherOps exact candidates matcher ⦃ result =>
      result.1 = (match exact with
        | some access => some access
        | none => finish none (matching evaluate (model.remaining candidates))) ∧
      (model.remaining result.2).length ≤ (model.remaining candidates).length ∧
      (exact.isSome = true → result.2 = candidates) ⦄ := by
  classical
  cases exact with
  | some access => simp [decision.resolve]
  | none =>
    have recursive := resolve_loop_correct eq eqLaw cursor model cursorLaw
      matcherOps matcher evaluate matcherLaw candidates none
    apply WP.spec_mono (by simpa [decision.resolve] using recursive)
    intro result spec
    exact ⟨spec.1, spec.2, by simp⟩

include eqLaw in
/- A matching conflicting candidate returns its ACTUAL immediate next cursor.
No suffix enumeration or later matching is evaluated, regardless of those ops. -/
theorem first_matching_conflict_stops (candidates next : C) (template : T)
    (access previous : A)
    (take : cursor.next candidates = .ok (some (template, access), next))
    (hit : matcherOps.«matches» matcher template = .ok true)
    (different : access ≠ previous) :
    decision.resolve_loop eq cursor matcherOps candidates matcher (some previous) =
      .ok (none, next) := by
  unfold decision.resolve_loop
  rw [loop]
  simp [decision.resolve_loop.body, take, hit, eqLaw.2, different]

include eqLaw cursorLaw matcherLaw in
/- Successful resolution is exactly an exact hit or a nonempty list whose full
access values all equal its first value. It is not an always-deny safety theorem. -/
theorem resolve_success_iff (copy : core.marker.Copy A) (exact : Option A) (candidates : C) :
    decision.resolve eq copy cursor matcherOps exact candidates matcher ⦃ result =>
      ∀ output, result.1 = some output ↔ exact = some output ∨
        (exact = none ∧ ∃ tail,
          matching evaluate (model.remaining candidates) = output :: tail ∧
          ∀ other ∈ tail, other = output) ⦄ := by
  classical
  apply WP.spec_mono (resolve_correct eq eqLaw cursor model cursorLaw matcherOps
    matcher evaluate matcherLaw copy exact candidates)
  intro result resultSpec output
  rw [resultSpec.1]
  cases exact with
  | some access => simp [eq_comm]
  | none => simpa using finish_success_iff (matching evaluate (model.remaining candidates)) output

end Resolver

#print axioms resolve_loop_correct
#print axioms exact_without_visits
#print axioms resolve_correct
#print axioms first_matching_conflict_stops
#print axioms resolve_success_iff

end
end ResolutionProof
