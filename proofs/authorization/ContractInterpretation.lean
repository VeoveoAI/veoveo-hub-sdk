import AuthorizationProof

open Aeneas Aeneas.Std
open sdk_authorization_proof ResolutionProof TemplateProof PathProof MatcherProof AuthorizationProof

namespace ContractInterpretation
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/- A finite mathematical interpretation of FULL access values witnesses that the
premises are satisfiable. This is not a replacement Rust type or wire parser.
Native tests separately exercise SDK RouteAccess and original-reference identity. -/
inductive Access where
  | Authenticated
  | Permission (permission : Bool)
  deriving DecidableEq

def equality (A : Type) [DecidableEq A] : core.cmp.PartialEq A A where
  eq left right := .ok (decide (left = right))
  ne left right := .ok (decide (left ≠ right))

theorem equality_law (A : Type) [DecidableEq A] : LawfulEq (equality A) := by
  constructor <;> intros <;> rfl

def copyAccess : core.marker.Copy Access where
  cloneInst := { clone := fun access => .ok access }

def cursor (I : Type) : decision.Cursor (List I) I where
  next values := .ok (match values with | [] => (none, []) | head :: tail => (some head, tail))

def enumeration (I : Type) : Enumeration (List I) I where
  remaining := id

theorem cursor_law (I : Type) : LawfulCursor (cursor I) (enumeration I) := by
  intro values
  cases values <;> simp [cursor, enumeration]

def text : TextModel _root_.String _root_.String where
  segment := id
  literal := id

def segments : decision.Segment _root_.String _root_.String where
  is_empty value := .ok (decide (value = ""))
  literal_eq value literal := .ok (decide (value = literal))

theorem segment_law : LawfulSegment segments text := by
  constructor <;> intros <;> rfl

def queryModel : QueryModel (Finset Bool) Bool where
  allowed declared := { key | key ∈ declared }

def query : decision.AllowedQuery (Finset Bool) Bool where
  is_empty declared := .ok (decide (declared = ∅))
  contains declared key := .ok (decide (key ∈ declared))

theorem query_law : LawfulQuery query queryModel := by
  constructor
  · intro declared
    have empty : queryModel.allowed declared = ∅ ↔ declared = ∅ := by
      simp [queryModel, Set.eq_empty_iff_forall_notMem, Finset.eq_empty_iff_forall_notMem]
    simp [query, empty]
  · intro declared key
    simp [query, queryModel]

/- Nonempty decoded query, empty root path. The alternate template has a different
root; neither valid query membership nor permission projection can hide that. -/
def view (same : Bool) : DecodedView Bool _root_.String _root_.String (Finset Bool) (List Bool) where
  actualRoot := true
  expectedRoot := same
  template := { path := Slice.new _, trailing_slash := false, query := {false} }
  resource := { segments := Slice.new _, has_query := true, query_empty := false, query := [false] }

def decode (same : Bool) := some (view same)

def matcher : decision.Matcher Unit Bool where
  «matches» _ candidate := decodedTrace (equality Bool) segments (cursor Bool) query (decode candidate)

theorem matcher_trace : NativeMatcherTrace (equality Bool) segments (cursor Bool) query matcher () decode := by
  intro candidate
  rfl

theorem native_view (same : Bool) : NativeQueryView (enumeration Bool) (view same).resource := by
  simp [NativeQueryView, enumeration, view]

theorem empty_path_allowed : PathAllowed text (Slice.new (decision.Path _root_.String)) (Slice.new _root_.String) false := by
  refine ⟨0, ?_, ?_, ?_⟩
  · simp [Trimmed]
  · simp [Cardinality, TerminalTail]
  · intro index impossible
    simp at impossible

theorem decoded_membership (same : Bool) :
    DecodedAllowed text queryModel (enumeration Bool) (decode same) ↔ same = true := by
  simp [DecodedAllowed, decode, view, RawViewAllowed, empty_path_allowed, RawQueryAllowed,
    EveryQuery, enumeration, queryModel, eq_comm, PathAllowed, Trimmed, Cardinality, TerminalTail, LiteralSuffix]

/- The complete composed resolver theorem applies to this lawful, nonvacuous
interpretation, not just to a matcher whose desired result was postulated. -/
theorem composed_laws :
    LawfulMatcher matcher () (fun candidate => decide (DecodedAllowed text queryModel (enumeration Bool) (decode candidate))) := by
  exact derived_matcher (equality Bool) (equality_law Bool) segments text segment_law
    (cursor Bool) (enumeration Bool) (cursor_law Bool) query queryModel query_law matcher () decode matcher_trace

theorem nontrivial_match (candidate : Bool) : matcher.«matches» () candidate = .ok candidate := by
  rw [matcher_trace candidate,
    decoded_trace_exact (equality Bool) (equality_law Bool) segments text segment_law
      (cursor Bool) (enumeration Bool) (cursor_law Bool) query queryModel query_law]
  simp [decoded_membership]

/- Two equal Authenticated values agree, while a distinct Permission is a real
conflict. Authenticated here is a requirement, not final user authorization. -/
theorem authenticated_agreement :
    decision.resolve (equality Access) copyAccess (cursor (Bool × Access)) matcher none
      [(true, .Authenticated), (false, .Permission true), (true, .Authenticated)] ()
      ⦃ result => result.1 = some .Authenticated ⦄ := by
  have resolved := resolve_correct (equality Access) (equality_law Access)
    (cursor (Bool × Access)) (enumeration (Bool × Access)) (cursor_law (Bool × Access))
    matcher () _ composed_laws copyAccess none
    [(true, Access.Authenticated), (false, Access.Permission true), (true, Access.Authenticated)]
  apply WP.spec_mono resolved
  intro result specification
  have evaluate : (fun candidate => decide (DecodedAllowed text queryModel (enumeration Bool) (decode candidate))) = id := by
    funext candidate
    simp [decoded_membership]
  rw [evaluate] at specification
  simpa [enumeration, matching, finish, List.filterMap_cons, List.filterMap_nil] using specification.1

theorem full_access_conflict :
    decision.resolve (equality Access) copyAccess (cursor (Bool × Access)) matcher none
      [(true, .Authenticated), (true, .Permission false)] ()
      ⦃ result => result.1 = none ⦄ := by
  have resolved := resolve_correct (equality Access) (equality_law Access)
    (cursor (Bool × Access)) (enumeration (Bool × Access)) (cursor_law (Bool × Access))
    matcher () _ composed_laws copyAccess none
    [(true, Access.Authenticated), (true, Access.Permission false)]
  apply WP.spec_mono resolved
  intro result specification
  have evaluate : (fun candidate => decide (DecodedAllowed text queryModel (enumeration Bool) (decode candidate))) = id := by
    funext candidate
    simp [decoded_membership]
  rw [evaluate] at specification
  simpa [enumeration, matching, finish, List.filterMap_cons, List.filterMap_nil] using specification.1

theorem exact_overrides_conflict :
    decision.resolve (equality Access) copyAccess (cursor (Bool × Access)) matcher (some .Authenticated)
      [(true, .Permission false), (true, .Permission true)] () =
      .ok (some .Authenticated, [(true, .Permission false), (true, .Permission true)]) := by
  rfl

#print axioms equality_law
#print axioms cursor_law
#print axioms segment_law
#print axioms query_law
#print axioms native_view
#print axioms composed_laws
#print axioms nontrivial_match
#print axioms authenticated_agreement
#print axioms full_access_conflict
#print axioms exact_overrides_conflict
end
end ContractInterpretation
