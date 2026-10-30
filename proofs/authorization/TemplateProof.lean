import ResolutionProof

open Aeneas Aeneas.Std
open sdk_authorization_proof ResolutionProof

namespace TemplateProof

structure TextModel (S L : Type) where
  segment : S → _root_.String
  literal : L → _root_.String

def LawfulSegment {S L : Type} (ops : decision.Segment S L) (text : TextModel S L) : Prop :=
  (∀ value, ops.is_empty value = .ok (decide (text.segment value = ""))) ∧
  (∀ value literal, ops.literal_eq value literal = .ok (decide (text.segment value = text.literal literal)))

def PartMatches {S L : Type} (text : TextModel S L) (part : decision.Path L) (segment : S) : Prop :=
  match part with
  | .Literal literal => text.segment segment = text.literal literal
  | .Scalar | .Tail => True

def LiteralSuffix {S L : Type} (text : TextModel S L) (path : Slice (decision.Path L))
    (segments : Slice S) (active start : Nat) : Prop :=
  ∀ index (pathBound : index < path.val.length) (segmentBound : index < segments.val.length),
    start ≤ index → index < active → PartMatches text path.val[index] segments.val[index]

theorem literal_suffix_step {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S) (active index : Nat)
    (pathBound : index < path.val.length) (segmentBound : index < segments.val.length)
    (activeBound : index < active) :
    LiteralSuffix text path segments active index ↔
      PartMatches text path.val[index] segments.val[index] ∧
      LiteralSuffix text path segments active (index + 1) := by
  constructor
  · intro suffix
    refine ⟨suffix index pathBound segmentBound (by omega) activeBound, ?_⟩
    intro other otherPath otherSegment after before
    exact suffix other otherPath otherSegment (by omega) before
  · rintro ⟨here, rest⟩ other otherPath otherSegment after before
    by_cases same : other = index
    · subst other
      exact here
    · exact rest other otherPath otherSegment (by omega) before

theorem literal_suffix_empty {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S) (active index : Nat)
    (finished : path.val.length ≤ index ∨ active ≤ index) :
    LiteralSuffix text path segments active index := by
  intro other pathBound segmentBound after before
  omega

section PathLoop
variable {S L : Type}
variable (ops : decision.Segment S L) (text : TextModel S L)
variable (law : LawfulSegment ops text)

include law in
/- Total correctness of the actual literal scan, from any index. The active
prefix is bounded by the original segment slice; indexing and increments are safe. -/
theorem path_loop_correct (path : Slice (decision.Path L)) (segments : Slice S)
    (active pathLen index : Usize) (activeBound : active.val ≤ segments.val.length)
    (pathLength : pathLen.val = path.val.length) :
    decision.path_matches_loop0 ops path segments active pathLen index ⦃ result =>
      result = true ↔ LiteralSuffix text path segments active.val index.val ⦄ := by
  classical
  unfold decision.path_matches_loop0
  rw [loop]
  unfold decision.path_matches_loop0.body
  split
  next beforePath =>
    have pathBound : index.val < path.val.length := by scalar_tac
    split
    next beforeActive =>
      have before : index.val < active.val := by scalar_tac
      have segmentBound : index.val < segments.val.length := by omega
      have suffix := literal_suffix_step text path segments active.val index.val pathBound segmentBound before
      step as ⟨part, partEq⟩
      have suffix' : LiteralSuffix text path segments active.val index.val ↔
          PartMatches text part segments.val[index.val] ∧
          LiteralSuffix text path segments active.val (index.val + 1) := by
        simpa only [← partEq] using suffix
      cases part with
      | Literal literal =>
        step as ⟨value, valueEq⟩
        simp only [law.2, bind_ok]
        by_cases same : text.segment value = text.literal literal
        · simp [same]
          step as ⟨next, nextEq⟩
          have nextValue : next.val = index.val + 1 := by simpa using nextEq
          have recursive := path_loop_correct path segments active pathLen next activeBound pathLength
          have refined := WP.spec_mono recursive (fun result spec => show
              result = true ↔ LiteralSuffix text path segments active.val index.val from by
            simpa [suffix', PartMatches, ← valueEq, same, nextValue] using spec)
          simpa [decision.path_matches_loop0, decision.path_matches_loop0.body, law.2, same, nextEq] using refined
        · simp [same, suffix', PartMatches, ← valueEq]
      | Scalar =>
        step as ⟨next, nextEq⟩
        have nextValue : next.val = index.val + 1 := by simpa using nextEq
        have recursive := path_loop_correct path segments active pathLen next activeBound pathLength
        have refined := WP.spec_mono recursive (fun result spec => show
            result = true ↔ LiteralSuffix text path segments active.val index.val from by
          simpa [suffix', PartMatches, nextValue] using spec)
        simpa [decision.path_matches_loop0, decision.path_matches_loop0.body, nextEq] using refined
      | Tail =>
        step as ⟨next, nextEq⟩
        have nextValue : next.val = index.val + 1 := by simpa using nextEq
        have recursive := path_loop_correct path segments active pathLen next activeBound pathLength
        have refined := WP.spec_mono recursive (fun result spec => show
            result = true ↔ LiteralSuffix text path segments active.val index.val from by
          simpa [suffix', PartMatches, nextValue] using spec)
        simpa [decision.path_matches_loop0, decision.path_matches_loop0.body, nextEq] using refined
    next afterActive =>
      have finished : active.val ≤ index.val := by scalar_tac
      have suffix := literal_suffix_empty text path segments active.val index.val (Or.inr finished)
      simp [suffix]
  next afterPath =>
    have finished : path.val.length ≤ index.val := by scalar_tac
    have suffix := literal_suffix_empty text path segments active.val index.val (Or.inl finished)
    simp [suffix]
termination_by min path.val.length active.val - index.val
decreasing_by all_goals omega

/- These are generated copies of the SAME source loop. Definitional equality
reuses its proof; no Rust or generated model was edited to remove the copies. -/
theorem path_loop1_eq (path : Slice (decision.Path L)) (segments : Slice S)
    (active pathLen index : Usize) :
    decision.path_matches_loop1 ops path segments active pathLen index =
      decision.path_matches_loop0 ops path segments active pathLen index := rfl

theorem path_loop2_eq (path : Slice (decision.Path L)) (segments : Slice S)
    (active pathLen index : Usize) :
    decision.path_matches_loop2 ops path segments active pathLen index =
      decision.path_matches_loop0 ops path segments active pathLen index := rfl

theorem path_loop3_eq (path : Slice (decision.Path L)) (segments : Slice S)
    (active pathLen index : Usize) :
    decision.path_matches_loop3 ops path segments active pathLen index =
      decision.path_matches_loop0 ops path segments active pathLen index := rfl
end PathLoop

structure QueryModel (Q N : Type) where
  allowed : Q → Set N

def LawfulQuery {Q N : Type} (ops : decision.AllowedQuery Q N) (model : QueryModel Q N) : Prop := by
  classical
  exact (∀ allowed, ops.is_empty allowed = .ok (decide (model.allowed allowed = ∅))) ∧
    (∀ allowed queryName, ops.contains allowed queryName = .ok (decide (queryName ∈ model.allowed allowed)))

section Queries
variable {Q N C : Type}
variable (cursor : decision.Cursor C N) (enumeration : Enumeration C N)
variable (cursorLaw : LawfulCursor cursor enumeration)
variable (ops : decision.AllowedQuery Q N) (model : QueryModel Q N)
variable (law : LawfulQuery ops model)

include cursorLaw law in
/- Total native-key traversal. Ordering and duplicate values are immaterial to
membership; the native decoder owns duplicate decoded-queryName rejection. -/
theorem query_loop_correct (candidates : C) (allowed : Q) :
    decision.matches_view_loop0 cursor ops candidates allowed ⦃ result =>
      result = true ↔ ∀ queryName ∈ enumeration.remaining candidates, queryName ∈ model.allowed allowed ⦄ := by
  classical
  unfold decision.matches_view_loop0
  rw [loop]
  unfold decision.matches_view_loop0.body
  step with (cursorLaw candidates) as ⟨pair, nextSpec⟩
  rcases pair with ⟨entry, next⟩
  cases remaining : enumeration.remaining candidates with
  | nil =>
    simp only [remaining] at nextSpec
    rcases nextSpec with ⟨rfl, suffix⟩
    simp [remaining]
  | cons queryName tail =>
    simp only [remaining] at nextSpec
    rcases nextSpec with ⟨rfl, suffix⟩
    simp only [law.2, bind_ok]
    by_cases known : queryName ∈ model.allowed allowed
    · have recursive := query_loop_correct next allowed
      have refined := WP.spec_mono recursive (fun result spec => show
          result = true ↔ ∀ queryName ∈ enumeration.remaining candidates, queryName ∈ model.allowed allowed from by
        simpa [remaining, suffix, known] using spec)
      simpa [decision.matches_view_loop0, decision.matches_view_loop0.body, law.2, known, remaining, suffix] using refined
    · simp [known, remaining]
termination_by (enumeration.remaining candidates).length
decreasing_by all_goals simp_all

theorem query_loop1_eq (candidates : C) (allowed : Q) :
    decision.matches_view_loop1 cursor ops candidates allowed =
      decision.matches_view_loop0 cursor ops candidates allowed := rfl
end Queries

#print axioms path_loop_correct
#print axioms path_loop1_eq
#print axioms path_loop2_eq
#print axioms path_loop3_eq
#print axioms query_loop_correct
#print axioms query_loop1_eq

end TemplateProof
