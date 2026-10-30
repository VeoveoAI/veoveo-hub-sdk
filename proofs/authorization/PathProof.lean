import TemplateProof

open Aeneas Aeneas.Std
open sdk_authorization_proof ResolutionProof TemplateProof

namespace PathProof

def IsTail {L : Type} (part : decision.Path L) : Prop :=
  match part with
  | .Tail => True
  | .Literal _ | .Scalar => False

/- This reads the final tag only when a final element exists. It does not assert
that any earlier Tail is a valid template; native parsing owns that invariant. -/
def TerminalTail {L : Type} (path : Slice (decision.Path L)) : Prop :=
  ∃ nonempty : 0 < path.val.length,
    IsTail path.val[path.val.length - 1]

theorem terminal_tail_nonempty {L : Type} (path : Slice (decision.Path L))
    (nonempty : 0 < path.val.length) :
    TerminalTail path ↔ IsTail path.val[path.val.length - 1] := by
  constructor
  · rintro ⟨_, tail⟩
    exact tail
  · intro tail
    exact ⟨nonempty, tail⟩

theorem terminal_tail_empty {L : Type} (path : Slice (decision.Path L))
    (empty : path.val.length = 0) : ¬ TerminalTail path := by
  simp [TerminalTail, empty]

/- Effective segment count: remove exactly one terminal empty segment when a
trailing slash is required. Scalar contents are never tested for nonemptiness. -/
def Trimmed {S L : Type} (text : TextModel S L) (segments : Slice S)
    (trailing : Bool) (active : Nat) : Prop :=
  if trailing then
    ∃ nonempty : 0 < segments.val.length,
      active = segments.val.length - 1 ∧ text.segment segments.val[segments.val.length - 1] = ""
  else active = segments.val.length

def Cardinality {L : Type} (path : Slice (decision.Path L)) (active : Nat) : Prop :=
  (TerminalTail path ∧ path.val.length - 1 ≤ active) ∨
    (¬ TerminalTail path ∧ active = path.val.length)

def PathAllowed {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S) (trailing : Bool) : Prop :=
  ∃ active, Trimmed text segments trailing active ∧ Cardinality path active ∧
    LiteralSuffix text path segments active 0

@[simp] theorem path_allowed_without_trim {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S) :
    PathAllowed text path segments false ↔ Cardinality path segments.val.length ∧
      LiteralSuffix text path segments segments.val.length 0 := by
  simp [PathAllowed, Trimmed]

theorem path_allowed_with_trim {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S)
    (nonempty : 0 < segments.val.length) :
    PathAllowed text path segments true ↔
      text.segment segments.val[segments.val.length - 1] = "" ∧
      Cardinality path (segments.val.length - 1) ∧
      LiteralSuffix text path segments (segments.val.length - 1) 0 := by
  constructor
  · rintro ⟨active, ⟨_, count, empty⟩, cardinality, literals⟩
    subst active
    exact ⟨empty, cardinality, literals⟩
  · rintro ⟨empty, cardinality, literals⟩
    exact ⟨segments.val.length - 1, ⟨nonempty, rfl, empty⟩, cardinality, literals⟩

theorem path_allowed_empty_trim {S L : Type} (text : TextModel S L)
    (path : Slice (decision.Path L)) (segments : Slice S)
    (empty : segments.val.length = 0) : ¬ PathAllowed text path segments true := by
  simp [PathAllowed, Trimmed, empty]

/- A lemma about the repeated compiler-generated subexpression in path_matches;
this is not a replacement model of the implementation. The public proof rewrites
that actual subexpression, retaining both generated scan and arithmetic bodies. -/
theorem tail_expression_correct {L : Type} (path : Slice (decision.Path L)) :
    (do
      if Slice.len path != 0#usize then
        let index ← Slice.len path - 1#usize
        let part ← Slice.index_usize path index
        match part with
        | .Literal _ => Result.ok false
        | .Scalar => Result.ok false
        | .Tail => Result.ok true
      else Result.ok false) ⦃ result => result = true ↔ TerminalTail path ⦄ := by
  split
  next present =>
    have nonempty : 0 < path.val.length := by scalar_tac
    step as ⟨index, indexEq⟩
    have indexValue : index.val = path.val.length - 1 := by simpa using indexEq
    step as ⟨part, partEq⟩
    have tail : TerminalTail path ↔ IsTail part := by
      rw [terminal_tail_nonempty path nonempty]
      simpa only [partEq, indexValue]
    cases part <;> simp [tail, IsTail]
  next absent =>
    have empty : path.val.length = 0 := by scalar_tac
    have noTail := terminal_tail_empty path empty
    simp [noTail]

section Geometry
variable {S L : Type}
variable (ops : decision.Segment S L) (text : TextModel S L)
variable (law : LawfulSegment ops text)

include law in
/- The actual post-tail-test generated subexpression: ordinary cardinality
branches plus the proved literal loop. No matcher result is assumed. -/
theorem geometry_expression_correct (path : Slice (decision.Path L)) (segments : Slice S)
    (active : Usize) (activeBound : active.val ≤ segments.val.length)
    (tail : Bool) (tailSpec : tail = true ↔ TerminalTail path) :
    (do
      if tail then
        let required ← Slice.len path - 1#usize
        if active < required then Result.ok false
        else decision.path_matches_loop0 ops path segments active (Slice.len path) 0#usize
      else
        if active != Slice.len path then Result.ok false
        else decision.path_matches_loop0 ops path segments active (Slice.len path) 0#usize)
      ⦃ result => result = true ↔ Cardinality path active.val ∧
        LiteralSuffix text path segments active.val 0 ⦄ := by
  classical
  cases tail with
  | false =>
    have noTail : ¬ TerminalTail path := by
      intro yes
      simpa using tailSpec.mpr yes
    rw [if_neg (by decide : (false : Bool) ≠ true)]
    split
    next wrong =>
      have different : active.val ≠ path.val.length := by scalar_tac
      simp [Cardinality, noTail, different]
    next same =>
      have equal : active.val = path.val.length := by scalar_tac
      have scanned := path_loop_correct ops text law path segments active (Slice.len path) 0#usize activeBound (by simp)
      simpa [Cardinality, noTail, equal] using scanned
  | true =>
    have yesTail : TerminalTail path := tailSpec.mp rfl
    have nonempty : 0 < path.val.length := yesTail.choose
    rw [if_pos rfl]
    step as ⟨required, requiredEq⟩
    have requiredValue : required.val = path.val.length - 1 := by simpa using requiredEq
    split
    next tooShort =>
      have short : active.val < path.val.length - 1 := by scalar_tac
      have notEnough : ¬ path.val.length - 1 ≤ active.val := by omega
      simp [Cardinality, yesTail, notEnough]
    next enough =>
      have lower : path.val.length - 1 ≤ active.val := by scalar_tac
      have scanned := path_loop_correct ops text law path segments active (Slice.len path) 0#usize activeBound (by simp)
      simpa [Cardinality, yesTail, lower] using scanned

include law in
/- Full success AND refusal equivalence for trimming, terminal-tail cardinality
and all literal positions, with no domain-ID/nonempty-scalar predicate added. -/
theorem path_matches_correct (path : Slice (decision.Path L)) (segments : Slice S)
    (trailing : Bool) :
    decision.path_matches ops path segments trailing ⦃ result =>
      result = true ↔ PathAllowed text path segments trailing ⦄ := by
  classical
  unfold decision.path_matches
  simp only [path_loop1_eq, path_loop2_eq, path_loop3_eq]
  cases trailing with
  | false =>
    rw [if_neg (by decide : (false : Bool) ≠ true)]
    step with (tail_expression_correct path) as ⟨tail, tailSpec⟩
    have geometry := geometry_expression_correct ops text law path segments
      (Slice.len segments) (by simp) tail tailSpec
    simpa using geometry
  | true =>
    rw [if_pos rfl]
    split
    next empty =>
      have emptyLength : segments.val.length = 0 := by scalar_tac
      have impossible := path_allowed_empty_trim text path segments emptyLength
      simp [impossible]
    next present =>
      have nonempty : 0 < segments.val.length := by scalar_tac
      step as ⟨active, activeEq⟩
      have activeValue : active.val = segments.val.length - 1 := by simpa using activeEq
      have activeBound : active.val ≤ segments.val.length := by omega
      step as ⟨last, lastEq⟩
      simp only [law.1, bind_ok]
      by_cases emptyLast : text.segment last = ""
      · rw [if_pos (by simpa using emptyLast)]
        step with (tail_expression_correct path) as ⟨tail, tailSpec⟩
        have actualEmpty : text.segment segments.val[segments.val.length - 1] = "" := by
          simpa only [lastEq, activeValue] using emptyLast
        have geometry := geometry_expression_correct ops text law path segments active activeBound tail tailSpec
        have refined := WP.spec_mono geometry (fun result spec => show
            result = true ↔ PathAllowed text path segments true from by
          rw [path_allowed_with_trim text path segments nonempty]
          simpa [actualEmpty, activeValue] using spec)
        simpa using refined
      · have actualNonempty : text.segment segments.val[segments.val.length - 1] ≠ "" := by
          simpa only [lastEq, activeValue] using emptyLast
        simp [emptyLast, path_allowed_with_trim text path segments nonempty, actualNonempty]
end Geometry

#print axioms tail_expression_correct
#print axioms path_allowed_with_trim
#print axioms geometry_expression_correct
#print axioms path_matches_correct

end PathProof
