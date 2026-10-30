import MatcherProof

open Aeneas Aeneas.Std
open sdk_authorization_proof ResolutionProof TemplateProof PathProof MatcherProof

namespace AuthorizationProof
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/- The native parser/Core projection produces this view. It is NOT a list of
pre-authorized candidates: every raw template still passes the generated matcher. -/
structure DecodedView (K L S Q C : Type) where
  actualRoot : K
  expectedRoot : K
  template : decision.TemplateView L Q
  resource : decision.ResourceView S C

/- A composed extracted-function trace, not a second Rust implementation.
Native parse failure and the native call/borrow wiring remain adapter obligations. -/
def decodedTrace {K L S Q N C : Type}
    (rootEq : core.cmp.PartialEq K K) (segments : decision.Segment S L)
    (keys : decision.Cursor C N) (query : decision.AllowedQuery Q N)
    (decoded : Option (DecodedView K L S Q C)) : Result Bool := do
  match decoded with
  | none => .ok false
  | some view =>
    let same ← decision.same_root rootEq view.actualRoot view.expectedRoot
    if same then decision.matches_view segments keys query view.template view.resource
    else .ok false

def DecodedAllowed {K L S Q N C : Type} (text : TextModel S L)
    (query : QueryModel Q N) (keys : Enumeration C N)
    (decoded : Option (DecodedView K L S Q C)) : Prop :=
  match decoded with
  | none => False
  | some view => view.actualRoot = view.expectedRoot ∧
      RawViewAllowed text query keys view.template view.resource

/- Total Result specifications exclude failure/divergence and determine a Bool
result uniquely. This bridge uses the library's Result eliminator, not an axiom. -/
theorem bool_result_exact (value : Result Bool) (predicate : Prop) [Decidable predicate]
    (total : value ⦃ result => result = true ↔ predicate ⦄) :
    value = .ok (decide predicate) := by
  cases value using Result.cases with
  | ret result =>
    have correct := (WP.spec_ok result).mp total
    cases result <;> simp_all
  | vis effect continuation => exact False.elim ((WP.spec_vis effect continuation).mp total)
  | div => exact False.elim (WP.spec_div.mp total)

section Composition
variable {K L S Q N C : Type} [DecidableEq K]
variable (rootEq : core.cmp.PartialEq K K) (rootLaw : LawfulEq rootEq)
variable (segments : decision.Segment S L) (text : TextModel S L)
variable (segmentLaw : LawfulSegment segments text)
variable (keys : decision.Cursor C N) (keyModel : Enumeration C N)
variable (keyLaw : LawfulCursor keys keyModel)
variable (query : decision.AllowedQuery Q N) (queryModel : QueryModel Q N)
variable (queryLaw : LawfulQuery query queryModel)

include rootLaw segmentLaw keyLaw queryLaw in
theorem decoded_trace_exact (decoded : Option (DecodedView K L S Q C)) :
    decodedTrace rootEq segments keys query decoded =
      .ok (decide (DecodedAllowed text queryModel keyModel decoded)) := by
  classical
  apply bool_result_exact
  cases decoded with
  | none => simp [decodedTrace, DecodedAllowed]
  | some view =>
    simp only [decodedTrace, same_root_correct rootEq rootLaw, bind_ok]
    by_cases same : view.actualRoot = view.expectedRoot
    · simp only [same, decide_true, Bool.true_eq, if_true]
      apply WP.spec_mono (matches_view_correct segments text segmentLaw keys keyModel keyLaw
        query queryModel queryLaw view.template view.resource)
      intro result correct
      simpa [DecodedAllowed, same] using correct
    · simp [same, DecodedAllowed]

/- This is the remaining native parent adapter correspondence, not a desired
matcher result assumption: its RHS calls the ACTUAL generated root/path/query
functions. Parsing, decoded text and borrowing into their arguments stay native. -/
def NativeMatcherTrace {T M : Type} (ops : decision.Matcher M T) (matcher : M)
    (decode : T → Option (DecodedView K L S Q C)) : Prop :=
  ∀ template, ops.«matches» matcher template =
    decodedTrace rootEq segments keys query (decode template)

include rootLaw segmentLaw keyLaw queryLaw in
theorem derived_matcher {T M : Type} (ops : decision.Matcher M T) (matcher : M)
    (decode : T → Option (DecodedView K L S Q C))
    (native : NativeMatcherTrace rootEq segments keys query ops matcher decode) :
    LawfulMatcher ops matcher (fun template => decide (DecodedAllowed text queryModel keyModel (decode template))) := by
  intro template
  rw [native template]
  exact decoded_trace_exact rootEq rootLaw segments text segmentLaw keys keyModel keyLaw
    query queryModel queryLaw (decode template)

include rootLaw segmentLaw keyLaw queryLaw in
/- Success and refusal for the resolver composed with actual root/path/query
semantics. Exact hits still take precedence; template agreement uses FULL access. -/
theorem authorization_success_iff {T A E M : Type} [DecidableEq A]
    (eq : core.cmp.PartialEq A A) (eqLaw : LawfulEq eq) (copy : core.marker.Copy A)
    (entries : decision.Cursor E (T × A)) (entryModel : Enumeration E (T × A))
    (entryLaw : LawfulCursor entries entryModel)
    (ops : decision.Matcher M T) (matcher : M)
    (decode : T → Option (DecodedView K L S Q C))
    (native : NativeMatcherTrace rootEq segments keys query ops matcher decode)
    (exact : Option A) (candidates : E) :
    decision.resolve eq copy entries ops exact candidates matcher ⦃ result =>
      ∀ output, result.1 = some output ↔ exact = some output ∨
        (exact = none ∧ ∃ tail,
          matching (fun template => decide (DecodedAllowed text queryModel keyModel (decode template)))
            (entryModel.remaining candidates) = output :: tail ∧
          ∀ other ∈ tail, other = output) ⦄ := by
  exact resolve_success_iff eq eqLaw entries entryModel entryLaw ops matcher _
    (derived_matcher rootEq rootLaw segments text segmentLaw keys keyModel keyLaw
      query queryModel queryLaw ops matcher decode native) copy exact candidates

/- Only native-consistent projections identify flags with query presence/keys.
Both laws are required; template grammar and terminal Tail remain parser-owned. -/
theorem native_decoded_equivalence (view : DecodedView K L S Q C)
    (native : NativeQueryView keyModel view.resource) :
    DecodedAllowed text queryModel keyModel (some view) ↔
      view.actualRoot = view.expectedRoot ∧
      PathAllowed text view.template.path view.resource.segments view.template.trailing_slash ∧
      NativeQueryAllowed queryModel keyModel view.template view.resource := by
  simp only [DecodedAllowed, RawViewAllowed,
    native_query_equivalence (queryModel := queryModel) (enumeration := keyModel)
      view.template view.resource native]
end Composition

#print axioms bool_result_exact
#print axioms decoded_trace_exact
#print axioms derived_matcher
#print axioms authorization_success_iff
#print axioms native_decoded_equivalence

end
end AuthorizationProof
