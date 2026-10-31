import InternalAuthSpecification
import InternalAuthValueLaws
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthValueLaws

namespace InternalAuthContextProof
noncomputable section
local instance (p : Prop) : Decidable p := Classical.propDecidable p

/-- Representation restriction of the native text adapter, not an admission
condition added to the Rust API. All native allocation availability and physical
usize/string correspondence remains outside this value-level contract. -/
def TextRepresentable (text : String) : Prop := text.toByteArray.size ≤ Usize.max

private def utf8Byte (value : UInt8) : U8 :=
  ⟨value.toNat, by cases value; simp only [UInt8.toNat_ofBitVec,
    UScalarTy.U8_numBits_eq, Nat.reducePow]; omega⟩

private def utf8Bytes (text : String) : List U8 := text.toByteArray.toList.map utf8Byte

private def encoded (text : String) (bound : TextRepresentable text) : Str :=
  Slice.from (utf8Bytes text) (by
    simpa only [TextRepresentable, utf8Bytes, List.length_map, ByteArray.length_toList] using bound)

private theorem encode_exact (text : String) (bound : TextRepresentable text) :
    Native.stringToStr text = .ok (encoded text bound) := by
  have lengthBound : (utf8Bytes text).length ≤ Usize.max := by
    simpa only [TextRepresentable, utf8Bytes, List.length_map, ByteArray.length_toList] using bound
  unfold Native.stringToStr
  with_unfolding_all change (if sizeBound : (utf8Bytes text).length ≤ Usize.max
    then Result.ok (Slice.from (utf8Bytes text) sizeBound)
    else Result.fail .maximumSizeExceeded) = Result.ok (encoded text bound)
  rw [dif_pos lengthBound]
  rfl

private theorem byte_roundtrip (value : UInt8) :
    UInt8.ofNat (utf8Byte value).val = value := by
  have converted : (utf8Byte value).val = value.toNat := by
    with_unfolding_all rfl
  rw [converted]
  exact UInt8.ofNat_toNat

/-- The pinned native ByteArray conversion uses an accumulator loop. Its finite
suffix equation establishes contents, not merely the available length theorem. -/
private theorem byte_array_to_list (bytes : ByteArray) : bytes.toList = bytes.data.toList := by
  have loopContents : ∀ index reversed,
      ByteArray.toList.loop bytes index reversed =
        reversed.reverse ++ bytes.data.toList.drop index := by
    intro index reversed
    fun_induction ByteArray.toList.loop bytes index reversed with
    | case1 index reversed active ih =>
      rw [ih]
      have inList : index < bytes.data.toList.length := by
        simpa only [Array.length_toList, ByteArray.size_data] using active
      have current : bytes.get! index = bytes.data.toList[index] := by
        simp [ByteArray.get!, getElem!_pos, active, inList]
      rw [List.drop_eq_getElem_cons inList]
      simp only [List.reverse_cons, List.append_assoc, List.singleton_append, current]
    | case2 index reversed inactive =>
      have exhausted : bytes.data.toList.length ≤ index := by
        simpa only [Array.length_toList, ByteArray.size_data] using Nat.le_of_not_gt inactive
      simp only [List.drop_eq_nil_of_le exhausted, List.append_nil]
  unfold ByteArray.toList
  simpa using loopContents 0 []

private theorem decode_exact (text : String) (bound : TextRepresentable text) :
    Native.strToString (encoded text bound) = .ok text := by
  have bytesBack : ByteArray.mk ((utf8Bytes text).toArray.map
      (fun byte => UInt8.ofNat byte.val)) = text.toByteArray := by
    change ByteArray.mk _ = ByteArray.mk text.toByteArray.data
    congr 1
    simp only [utf8Bytes, List.map_toArray, List.map_map,
      Function.comp_def, byte_roundtrip, byte_array_to_list]
    simpa only [List.map_id'] using
      (Array.toArray_toList (xs := text.toByteArray.data))
  unfold Native.strToString
  simp only [encoded, Slice.from_val]
  rw [bytesBack]
  simp only [String.fromUTF8?, dif_pos text.isValidUTF8]
  rfl

private theorem encoded_eq_iff (left right : String)
    (leftBound : TextRepresentable left) (rightBound : TextRepresentable right) :
    encoded left leftBound = encoded right rightBound ↔ left = right := by
  constructor
  · intro same
    have decoded := congrArg Native.strToString same
    simpa only [decode_exact, Result.ok.injEq] using decoded
  · intro same
    subst right
    rfl

private theorem encoded_str_eq (left right : String)
    (leftBound : TextRepresentable left) (rightBound : TextRepresentable right) :
    Str.Insts.CoreCmpPartialEqStr.eq (encoded left leftBound) (encoded right rightBound) =
      .ok (decide (left = right)) := by
  simp only [Str.Insts.CoreCmpPartialEqStr.eq, encoded_eq_iff]

private theorem group_role_eq (left right : identity.GroupRole) :
    identity.GroupRole.Insts.CoreCmpPartialEqGroupRole.eq left right =
      .ok (decide (left = right)) := by
  cases left <;> cases right <;>
    simp [identity.GroupRole.Insts.CoreCmpPartialEqGroupRole.eq,
      read_discriminant, identity.GroupRole.read_discriminant]

private theorem assurance_eq (left right : identity.PrincipalAssurance) :
    identity.PrincipalAssurance.Insts.CoreCmpPartialEqPrincipalAssurance.eq left right =
      .ok (decide (left = right)) := by
  cases left <;> cases right <;>
    simp [identity.PrincipalAssurance.Insts.CoreCmpPartialEqPrincipalAssurance.eq]

private theorem kind_eq (left right : identity.PrincipalKind) :
    identity.PrincipalKind.Insts.CoreCmpPartialEqPrincipalKind.eq left right =
      .ok (decide (left = right)) := by
  cases left <;> cases right <;>
    simp [identity.PrincipalKind.Insts.CoreCmpPartialEqPrincipalKind.eq,
      read_discriminant, identity.PrincipalKind.read_discriminant]

attribute [local simp]
  veoveo_types.identity.PrincipalId.Insts.CoreCmpPartialEqPrincipalId.eq
  veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq
  veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq
  veoveo_types.identity.WorkContextId.Insts.CoreCmpPartialEqWorkContextId.eq
  veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId.eq
  veoveo_types.identity.DelegationId.Insts.CoreCmpPartialEqDelegationId.eq
  veoveo_types.identity.GroupId.Insts.CoreCmpPartialEqGroupId.eq
  veoveo_types.identity.RoleId.Insts.CoreCmpPartialEqRoleId.eq
  veoveo_types.identity.DataLabelId.Insts.CoreCmpPartialEqDataLabelId.eq
  veoveo_types.names.ScopeName.Insts.CoreCmpPartialEqScopeName.eq
  veoveo_types.provenance.InvocationMode.Insts.CoreCmpPartialEqInvocationMode.eq
  chrono.datetime.DateTime.Insts.CoreCmpPartialEqDateTime.eq
  group_role_eq assurance_eq kind_eq

private theorem group_membership_eq (left right : identity.GroupMembership) :
    identity.GroupMembership.Insts.CoreCmpPartialEqGroupMembership.eq left right =
      .ok (decide (left = right)) := by
  cases left with
  | mk leftGroup leftRole =>
    cases right with
    | mk rightGroup rightRole =>
      simp only [identity.GroupMembership.Insts.CoreCmpPartialEqGroupMembership.eq,
        veoveo_types.identity.GroupId.Insts.CoreCmpPartialEqGroupId.eq,
        group_role_eq, bind_ok]
      by_cases same : leftGroup = rightGroup <;>
        simp [same, identity.GroupMembership.mk.injEq]

private theorem group_set_eq (left right : OrderedSet veoveo_types.identity.GroupId) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      veoveo_types.identity.GroupId.Insts.CoreCmpPartialEqGroupId
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; rfl) left right

private theorem membership_set_eq (left right : OrderedSet identity.GroupMembership) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      identity.GroupMembership.Insts.CoreCmpPartialEqGroupMembership
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; exact group_membership_eq left right) left right

private theorem role_set_eq (left right : OrderedSet veoveo_types.identity.RoleId) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      veoveo_types.identity.RoleId.Insts.CoreCmpPartialEqRoleId
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; rfl) left right

private theorem scope_set_eq (left right : OrderedSet veoveo_types.names.ScopeName) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      veoveo_types.names.ScopeName.Insts.CoreCmpPartialEqScopeName
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; rfl) left right

private theorem label_set_eq (left right : OrderedSet veoveo_types.identity.DataLabelId) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      veoveo_types.identity.DataLabelId.Insts.CoreCmpPartialEqDataLabelId
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; rfl) left right

private theorem assurance_set_eq (left right : OrderedSet identity.PrincipalAssurance) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
      identity.PrincipalAssurance.Insts.CoreCmpPartialEqPrincipalAssurance
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone left right =
      .ok (decide (left = right)) := by
  exact set_eq_exact _ _ (by intro left right; exact assurance_eq left right) left right

private theorem tenant_option_eq (left right : Option veoveo_types.identity.TenantId) :
    core.option.Option.Insts.CoreCmpPartialEqOption.eq
      veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId left right =
      .ok (decide (left = right)) := by
  exact option_eq_exact _ (by intro left right; rfl) left right

private theorem shared_tenant_option_eq (left right : Option veoveo_types.identity.TenantId) :
    core.option.Option.Insts.CoreCmpPartialEqOption.eq
      (core.cmp.PartialEqShared veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId)
      left right = .ok (decide (left = right)) := by
  exact option_eq_exact _ (by intro left right; rfl) left right

private theorem timestamp_option_eq (left right : Option NativeTimestamp) :
    core.option.Option.Insts.CoreCmpPartialEqOption.eq
      (chrono.datetime.DateTime.Insts.CoreCmpPartialEqDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      .ok (decide (left = right)) := by
  exact option_eq_exact _ (by intro left right; rfl) left right

/-- The decision dictionary is quantified: generated Boolean guards use their
computable Bool equality dictionary rather than this file's classical Prop one. -/
private theorem guard_bool (first second : Bool) [test : Decidable (first = true)] :
    (if first = true then (Result.ok second : Result Bool) else Result.ok false) =
      Result.ok (first && second) := by
  cases first <;> simp

private theorem guard_pair {A : Type} (value : A) (first second : Bool)
    [test : Decidable (first = true)] :
    (if first = true then (Result.ok (value, second) : Result (A × Bool))
      else Result.ok (value, false)) = Result.ok (value, first && second) := by
  cases first <;> simp

private theorem guard_triple {A B : Type} (left : A) (right : B) (first second : Bool)
    [test : Decidable (first = true)] :
    (if first = true then (Result.ok (left, right, second) : Result (A × B × Bool))
      else Result.ok (left, right, false)) = Result.ok (left, right, first && second) := by
  cases first <;> simp

private theorem provenance_mode (provenance : veoveo_types.provenance.InvocationProvenance) :
    veoveo_types.provenance.InvocationProvenance.mode provenance =
      Result.ok (invocationMode provenance) := by
  cases provenance <;> rfl

/-- Each actual comparator is reduced once. Collapsing one two-proposition guard
at a time avoids enumerating combinations of unrelated Principal field values. -/
private theorem principal_eq (left right : identity.Principal) :
    identity.Principal.Insts.CoreCmpPartialEqPrincipal.eq left right =
      .ok (decide (left = right)) := by
  cases left
  cases right
  simp only [identity.Principal.Insts.CoreCmpPartialEqPrincipal.eq,
    veoveo_types.identity.PrincipalId.Insts.CoreCmpPartialEqPrincipalId.eq,
    kind_eq, veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq,
    veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq,
    tenant_option_eq, group_set_eq, membership_set_eq, role_set_eq,
    scope_set_eq, label_set_eq, assurance_set_eq, timestamp_option_eq,
    bind_ok, guard_bool, identity.Principal.mk.injEq, Bool.decide_and]

attribute [local simp] principal_eq group_set_eq membership_set_eq role_set_eq
  scope_set_eq label_set_eq assurance_set_eq tenant_option_eq shared_tenant_option_eq
  timestamp_option_eq

private theorem option_id_matches {tag : Nat}
    (actual : Option (NativeId tag)) (expected : NativeId tag) :
    (match actual with
      | none => Result.ok false
      | some value => Result.ok (decide (value = expected))) =
      Result.ok (decide (actual = some expected)) := by
  cases actual <;> simp

private theorem option_is_some {T : Type} (value : Option T) :
    value.isSome = decide (value ≠ none) := by
  cases value <;> simp

private theorem option_none_ne_some {T : Type} (value : T) :
    (none : Option T) ≠ some value := by
  intro impossible
  cases impossible

private theorem set_is_empty {T : Type} {A : Type}
    (allocator : core.alloc.AllocatorClone A)
    (value : alloc.collections.btree.set.BTreeSet T A) :
    alloc.collections.btree.set.BTreeSet.is_empty allocator value =
      Result.ok (decide (value.values = [])) := by
  cases remaining : value.values <;>
    simp [alloc.collections.btree.set.BTreeSet.is_empty, remaining]

private theorem negative_decide (condition : Prop) :
    (!decide condition) = decide (¬condition) := by
  by_cases holds : condition <;> simp [holds]

/-- Every text read by any provenance branch is physically representable. Direct
admission itself does not require these extra text operations; the uniform domain
lets this theorem characterize all three branches with one native refinement. -/
structure ContextTextDomain (context : Context) (actor : Principal) : Prop where
  sourceSubject : TextRepresentable context.principal.subject.value
  actorId : TextRepresentable actor.id.value
  actorSubject : TextRepresentable actor.subject.value
  tokenIssuer : TextRepresentable context.access_token.issuer.value
  client : TextRepresentable context.access_token.oauth_client_id.value

private theorem singleton_hash : String.singleton '#' = "#" := rfl

-- Keep the larger normalization allowance local to this finite all-mode proof.
set_option maxHeartbeats 800000 in
/-- Both acceptance and refusal of the actual transparent validator, preserving
complete direct/automated Principal equality and all delegated inherited fields.
This does not verify the authentication or acquisition of the supplied context. -/
theorem validate_for_exact (context : Context) (actor : Principal) (authority : Authority)
    (textDomain : ContextTextDomain context actor) :
    internal_auth.request.HubRequestContext.validate_for context actor authority =
      .ok (if ContextAccepted context actor authority then .Ok () else
        .Err internal_auth.InternalTokenError.InvalidRequestContext) := by
  have encodeSource := encode_exact _ textDomain.sourceSubject
  have encodeActor := encode_exact _ textDomain.actorId
  have encodeActorSubject := encode_exact _ textDomain.actorSubject
  have encodeIssuer := encode_exact _ textDomain.tokenIssuer
  have encodeClient := encode_exact _ textDomain.client
  unfold internal_auth.request.HubRequestContext.validate_for
  rw [veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq]
  simp only [bind_ok]
  -- Normalize only the input of the first remaining bind. The actual source
  -- preserves actor and authority in every common-guard branch; exposing this
  -- before provenance analysis prevents all three continuations being duplicated.
  conv =>
    lhs
    arg 1
    simp only [veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq,
      veoveo_types.identity.WorkContextId.Insts.CoreCmpPartialEqWorkContextId.eq,
      scope_set_eq, tenant_option_eq, shared_tenant_option_eq,
      core.option.Option.as_ref, provenance_mode,
      veoveo_types.provenance.InvocationMode.Insts.CoreCmpPartialEqInvocationMode.eq,
      bind_ok, uncurry_apply_pair, guard_bool, guard_pair, guard_triple, ← Bool.decide_and]
  -- The scoped conversion can already normalize this outer bind. Either form
  -- continues to the same branch obligations; no residual goal is discharged here.
  try simp only [bind_ok, uncurry_apply_pair]
  cases provenance : authority.provenance <;>
    simp only [provenance,
      core.cmp.impls.PartialEqShared.ne,
      core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
      veoveo_types.provenance.InvocationProvenance.mode,
      veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq,
      veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq,
      veoveo_types.identity.WorkContextId.Insts.CoreCmpPartialEqWorkContextId.eq,
      veoveo_types.identity.PrincipalId.Insts.CoreCmpPartialEqPrincipalId.eq,
      veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId.eq,
      veoveo_types.identity.DelegationId.Insts.CoreCmpPartialEqDelegationId.eq,
      veoveo_types.provenance.InvocationMode.Insts.CoreCmpPartialEqInvocationMode.eq,
      principal_eq, kind_eq, scope_set_eq, label_set_eq, assurance_set_eq,
      tenant_option_eq, shared_tenant_option_eq, option_id_matches,
      core.option.Option.as_ref, core.option.Option.is_some, option_is_some, set_is_empty,
      veoveo_types.identity.PrincipalId.as_str,
      veoveo_types.identity.TokenSubject.as_str,
      veoveo_types.identity.TokenIssuer.as_str,
      veoveo_types.platform_names.OAuthClientId.as_str,
      encodeSource, encodeActor, encodeActorSubject, encodeIssuer, encodeClient,
      alloc.string.String.new, alloc.string.String.push_str, alloc.string.String.push,
      Shared0Str.Insts.CoreCmpPartialEqString.ne,
      encoded_str_eq, decode_exact,
      String.push_eq_append, singleton_hash, negative_decide,
      String.empty_append, bind_ok, bind_tc_ok, decide_eq_true_eq]
  all_goals
    clear encodeSource encodeActor encodeActorSubject encodeIssuer encodeClient
    simp only [ContextAccepted, invocationMode, provenance, option_is_some]
    simp only [ite_true, ite_false, ite_self, decide_eq_true_eq, Bool.decide_coe,
      Bool.true_eq_false, Bool.false_eq_true, and_true, true_and, and_false, false_and,
      and_self, or_true, true_or, or_false, false_or, true_implies, false_implies,
      implies_true, not_true_eq_false, not_false_eq_true, not_not, eq_self_iff_true,
      ne_eq, decide_true, decide_false, bind_ok, bind_tc_ok, option_id_matches,
      Option.some.injEq, Option.some_ne_none, option_none_ne_some]
    -- Target one conditional; Lean's legacy split reduction also revisits nested guards.
    set_option backward.split false in
      repeat' (split <;>
        (try simp only [not_not] at *) <;>
        try simp only [*, ite_true, ite_false, ite_self, decide_eq_true_eq, Bool.decide_coe,
          Bool.true_eq_false, Bool.false_eq_true, and_true, true_and, and_false, false_and,
          and_self, or_true, true_or, or_false, false_or, true_implies, false_implies,
          implies_true, not_true_eq_false, not_false_eq_true, not_not, eq_self_iff_true,
          ne_eq, decide_true, decide_false, bind_ok, bind_tc_ok, option_id_matches,
          Option.some.injEq, Option.some_ne_none, option_none_ne_some])
  all_goals
    exfalso
    grind

#print axioms validate_for_exact

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let found ← Lean.collectAxioms ``InternalAuthContextProof.validate_for_exact
  unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
    throwError "unreviewed SDK context-admission dependency: {found}"

end
end InternalAuthContextProof
