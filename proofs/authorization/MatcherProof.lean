import PathProof

open Aeneas Aeneas.Std
open sdk_authorization_proof ResolutionProof TemplateProof PathProof

namespace MatcherProof

def EveryQuery {Q N C : Type} (model : QueryModel Q N) (enumeration : Enumeration C N)
    (allowed : Q) (keys : C) : Prop :=
  ∀ key ∈ enumeration.remaining keys, key ∈ model.allowed allowed

/- Raw behavior intentionally includes arbitrary flags: no consistency of the
flag snapshot and decoded key cursor is silently built into this theorem. -/
def RawQueryAllowed {L S Q N C : Type} (model : QueryModel Q N) (enumeration : Enumeration C N)
    (template : decision.TemplateView L Q) (resource : decision.ResourceView S C) : Prop :=
  (resource.has_query = true → model.allowed template.query ≠ ∅ ∧ resource.query_empty = false) ∧
    EveryQuery model enumeration template.query resource.query

def RawViewAllowed {L S Q N C : Type} (text : TextModel S L) (model : QueryModel Q N)
    (enumeration : Enumeration C N) (template : decision.TemplateView L Q)
    (resource : decision.ResourceView S C) : Prop :=
  PathAllowed text template.path resource.segments template.trailing_slash ∧
    RawQueryAllowed model enumeration template resource

/- These are the precise native Core query projection laws. Core also owns
canonical parsing/percent decoding and rejection of duplicate decoded names. -/
def NativeQueryView {S C N : Type} (enumeration : Enumeration C N)
    (resource : decision.ResourceView S C) : Prop :=
  (resource.query_empty = true ↔ enumeration.remaining resource.query = []) ∧
    (resource.has_query = false → enumeration.remaining resource.query = [])

def NativeQueryAllowed {L S Q N C : Type} (model : QueryModel Q N) (enumeration : Enumeration C N)
    (template : decision.TemplateView L Q) (resource : decision.ResourceView S C) : Prop :=
  resource.has_query = true → model.allowed template.query ≠ ∅ ∧
    enumeration.remaining resource.query ≠ [] ∧ EveryQuery model enumeration template.query resource.query

section View
variable {L S Q N C : Type}
variable (segmentOps : decision.Segment S L) (text : TextModel S L)
variable (segmentLaw : LawfulSegment segmentOps text)
variable (cursor : decision.Cursor C N) (enumeration : Enumeration C N)
variable (cursorLaw : LawfulCursor cursor enumeration)
variable (queryOps : decision.AllowedQuery Q N) (queryModel : QueryModel Q N)
variable (queryLaw : LawfulQuery queryOps queryModel)

include segmentLaw cursorLaw queryLaw in
/- Complete acceptance AND refusal over the actual generated path and query
functions. Scalar contents, optional key order and empty-tail behavior stay native. -/
theorem matches_view_correct (template : decision.TemplateView L Q)
    (resource : decision.ResourceView S C) :
    decision.matches_view segmentOps cursor queryOps template resource ⦃ result =>
      result = true ↔ RawViewAllowed text queryModel enumeration template resource ⦄ := by
  classical
  unfold decision.matches_view
  step with (path_matches_correct segmentOps text segmentLaw template.path resource.segments template.trailing_slash)
    as ⟨path, pathSpec⟩
  cases path with
  | false =>
    have denied : ¬ PathAllowed text template.path resource.segments template.trailing_slash := by
      intro allowed
      simpa using pathSpec.mpr allowed
    simp [RawViewAllowed, denied]
  | true =>
    have allowed : PathAllowed text template.path resource.segments template.trailing_slash := pathSpec.mp rfl
    cases present : resource.has_query with
    | false =>
      have query := query_loop_correct cursor enumeration cursorLaw queryOps queryModel queryLaw resource.query template.query
      simpa [query_loop1_eq, RawViewAllowed, RawQueryAllowed, EveryQuery, allowed, present] using query
    | true =>
      simp only [queryLaw.1, bind_ok]
      by_cases empty : queryModel.allowed template.query = ∅
      · simp [empty, RawViewAllowed, RawQueryAllowed, allowed, present]
      · simp only [empty, decide_false, Bool.false_eq_true, if_false]
        cases snapshot : resource.query_empty with
        | true => simp [RawViewAllowed, RawQueryAllowed, allowed, present, snapshot]
        | false =>
          have query := query_loop_correct cursor enumeration cursorLaw queryOps queryModel queryLaw resource.query template.query
          simpa [RawViewAllowed, RawQueryAllowed, EveryQuery, allowed, present, snapshot, empty] using query

/- Only the native-consistent corollary identifies flags with the decoded key
list and makes absence of a query imply an empty list. -/
theorem native_query_equivalence (template : decision.TemplateView L Q)
    (resource : decision.ResourceView S C) (native : NativeQueryView enumeration resource) :
    RawQueryAllowed queryModel enumeration template resource ↔
      NativeQueryAllowed queryModel enumeration template resource := by
  classical
  cases present : resource.has_query with
  | false =>
    have absent := native.2 present
    simp [RawQueryAllowed, NativeQueryAllowed, EveryQuery, present, absent]
  | true =>
    have emptyIff := native.1
    cases empty : resource.query_empty <;>
      simp [RawQueryAllowed, NativeQueryAllowed, present, empty] at emptyIff ⊢
    all_goals aesop

include segmentLaw cursorLaw queryLaw in
theorem native_matches_view_correct (template : decision.TemplateView L Q)
    (resource : decision.ResourceView S C) (native : NativeQueryView enumeration resource) :
    decision.matches_view segmentOps cursor queryOps template resource ⦃ result =>
      result = true ↔ PathAllowed text template.path resource.segments template.trailing_slash ∧
        NativeQueryAllowed queryModel enumeration template resource ⦄ := by
  apply WP.spec_mono (matches_view_correct segmentOps text segmentLaw cursor enumeration cursorLaw
    queryOps queryModel queryLaw template resource)
  intro result spec
  simpa only [RawViewAllowed, native_query_equivalence (queryModel := queryModel) (enumeration := enumeration) template resource native] using spec
end View

/- Root comparison is extracted too. Core's actual URI/text representation and
its wiring to these operands remain native adapter obligations. -/
theorem same_root_correct {K : Type} [DecidableEq K]
    (eq : core.cmp.PartialEq K K) (law : LawfulEq eq) (actual expected : K) :
    decision.same_root eq actual expected = .ok (decide (actual = expected)) := by
  simpa [decision.same_root, core.cmp.impls.PartialEqShared.eq] using law.1 actual expected

#print axioms matches_view_correct
#print axioms native_query_equivalence
#print axioms native_matches_view_correct
#print axioms same_root_correct

end MatcherProof
