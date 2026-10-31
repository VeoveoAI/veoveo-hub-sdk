import InternalAuthContextProof
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthValueLaws InternalAuthContextProof

namespace InternalAuthClaimsComponents
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

/-- Physical native String/slice bounds, not authentication or admission rules.
Native allocation availability and sorted/unique set traversal correspondence
remain separate from this exact modeled-traversal result. -/
def ClaimsTextDomain (token : Identity) : Prop :=
  TextRepresentable token.actor.id.value ∧
  TextRepresentable token.audience.value ∧
  (∀ scope ∈ token.actor.scopes.values, TextRepresentable scope.value) ∧
  TextRepresentable (String.intercalate " " (token.actor.scopes.values.map (·.value)))

/-- A total WP with a unique value excludes primitive failure/divergence. -/
private theorem exact_of_total {T : Type} (operation : Result T) (value : T)
    (total : operation ⦃ result => result = value ⦄) : operation = .ok value := by
  cases operation using Result.cases with
  | ret returned => exact congrArg Result.ok ((WP.spec_ok returned).mp total)
  | vis effect continuation => exact False.elim ((WP.spec_vis effect continuation).mp total)
  | div => exact False.elim (WP.spec_div.mp total)

-- Finite byte conversion laws are local because ContextProof's corresponding
-- helpers are private. No business validation or claims algorithm is modeled here.
private def utf8Byte (value : UInt8) : U8 :=
  ⟨value.toNat, by cases value; simp only [UInt8.toNat_ofBitVec,
    UScalarTy.U8_numBits_eq, Nat.reducePow]; omega⟩

private def utf8Bytes (text : String) : List U8 := text.toByteArray.toList.map utf8Byte

def encoded (text : String) (bound : TextRepresentable text) : Str :=
  Slice.from (utf8Bytes text) (by
    simpa only [TextRepresentable, utf8Bytes, List.length_map, ByteArray.length_toList] using bound)

theorem encode_exact (text : String) (bound : TextRepresentable text) :
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

theorem decode_exact (text : String) (bound : TextRepresentable text) :
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

private def rendered (scopes : List veoveo_types.names.ScopeName)
    (value : String) (first : Bool) : String :=
  match scopes with
  | [] => value
  | _ :: _ => value ++ (if first then "" else " ") ++
      String.intercalate " " (scopes.map (·.value))

private theorem rendered_step (head : veoveo_types.names.ScopeName)
    (tail : List veoveo_types.names.ScopeName) (value : String) (first : Bool) :
    rendered (head :: tail) value first =
      rendered tail ((if first then value else value.push ' ') ++ head.value) false := by
  cases tail <;> cases first <;>
    simp [rendered, String.push_eq_append, String.intercalate_cons_cons, String.append_assoc]

private theorem scope_body_cons (head : veoveo_types.names.ScopeName)
    (tail : List veoveo_types.names.ScopeName) (initial : String) (first : Bool)
    (bound : TextRepresentable head.value) :
    internal_auth.scope_claim_loop.body (head :: tail) initial first =
      .ok (.cont (tail, (if first then initial else initial.push ' ') ++ head.value, false)) := by
  unfold internal_auth.scope_claim_loop.body
  cases first <;>
    simp only [alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next,
      bind_ok, bind_tc_ok, Bool.false_eq_true, ite_false, ite_true,
      alloc.string.String.push, veoveo_types.names.ScopeName.as_str]
  all_goals
    with_unfolding_all
      change Aeneas.Std.bind (Native.stringToStr head.value) (fun fragment =>
        Aeneas.Std.bind (alloc.string.String.push_str _ fragment) (fun appended =>
          Result.ok (ControlFlow.cont (tail, appended, false)))) = _
    rw [encode_exact _ bound]
    simp only [bind_ok, bind_tc_ok, alloc.string.String.push_str, decode_exact]

/-- This is the extracted scope loop, with its actual finite traversal and space
handling. It does not assume an own-code loop result. -/
theorem scope_loop_exact (scopes : List veoveo_types.names.ScopeName)
    (value : String) (first : Bool)
    (bounds : ∀ scope ∈ scopes, TextRepresentable scope.value) :
    internal_auth.scope_claim_loop scopes value first = .ok (rendered scopes value first) := by
  apply exact_of_total
  unfold internal_auth.scope_claim_loop
  let target := rendered scopes value first
  refine loop.spec_decr_nat (fun state => state.1.length)
    (fun state => (∀ scope ∈ state.1, TextRepresentable scope.value) ∧
      rendered state.1 state.2.1 state.2.2 = target)
    (fun result => result = target) _ _ ?_ ⟨bounds, rfl⟩
  rintro ⟨current, initial, isFirst⟩ ⟨domain, invariant⟩
  cases current with
  | nil => simpa [internal_auth.scope_claim_loop.body,
      alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next,
      rendered, bind_ok, bind_tc_ok, WP.spec_ok] using invariant
  | cons head tail =>
    have tailBound : ∀ scope ∈ tail, TextRepresentable scope.value :=
      fun scope member => domain scope (List.mem_cons_of_mem head member)
    dsimp only
    rw [scope_body_cons head tail initial isFirst (domain head List.mem_cons_self)]
    simp only [WP.spec_ok]
    refine ⟨⟨tailBound, ?_⟩, by simp⟩
    simpa only [rendered_step] using invariant

theorem scope_claim_exact (actor : Principal)
    (bounds : ∀ scope ∈ actor.scopes.values, TextRepresentable scope.value) :
    internal_auth.scope_claim actor = .ok (expectedScope actor) := by
  unfold internal_auth.scope_claim
  simp only [alloc.collections.btree.set.BTreeSet.is_empty]
  cases entries : actor.scopes.values with
  | nil => simp [entries, expectedScope, bind_ok, bind_tc_ok]
  | cons head tail =>
    simp only [entries, List.isEmpty_cons, bind_ok, if_false,
      alloc.string.String.new,
      SharedABTreeSet.Insts.CoreIterTraitsCollectIntoIteratorSharedATIter.into_iter]
    rw [scope_loop_exact _ _ _ (by simpa only [entries] using bounds)]
    simp [expectedScope, entries, rendered, bind_ok, bind_tc_ok]

private def chainFold (actors : List internal_auth.ActingServer)
    (initial : Option internal_auth.PriorActor) : Option internal_auth.PriorActor :=
  actors.foldl (fun chain actor => some (.mk actor.client chain)) initial

private theorem prior_body_active
    (current : Aeneas.Std.core.slice.iter.Iter internal_auth.ActingServer)
    (prior : Option internal_auth.PriorActor) (active : current.i < current.slice.val.length) :
    internal_auth.exchange.prior_actors_loop.body current prior =
      .ok (.cont ({ current with i := current.i + 1 },
        some (.mk (current.slice.val[current.i]).client prior))) := by
  have nativeBound : current.i < current.slice.len := by simpa using active
  have nextStep : Aeneas.Std.core.slice.iter.IteratorSliceIter.next current =
      .ok (some current.slice.val[current.i], { current with i := current.i + 1 }) := by
    unfold Aeneas.Std.core.slice.iter.IteratorSliceIter.next
    rw [dif_pos nativeBound]
    apply congrArg Result.ok
    apply Prod.ext
    · exact congrArg some (Slice.getElem_Nat_eq current.slice current.i active)
    · rfl
  unfold internal_auth.exchange.prior_actors_loop.body
  rw [nextStep]
  simp only [bind_ok]
  with_unfolding_all
    change Aeneas.Std.bind
      (veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone
        (current.slice.val[current.i]).client)
      (fun client => Result.ok (ControlFlow.cont
        ({ current with i := current.i + 1 }, some (internal_auth.PriorActor.mk client prior)))) = _
  simp only [veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone, bind_ok]
  with_unfolding_all rfl

private theorem prior_body_done
    (current : Aeneas.Std.core.slice.iter.Iter internal_auth.ActingServer)
    (prior : Option internal_auth.PriorActor) (inactive : ¬ current.i < current.slice.val.length) :
    internal_auth.exchange.prior_actors_loop.body current prior = .ok (.done prior) := by
  have nativeBound : ¬ current.i < current.slice.len := by simpa using inactive
  have nextStep : Aeneas.Std.core.slice.iter.IteratorSliceIter.next current =
      .ok (none, current) := by
    unfold Aeneas.Std.core.slice.iter.IteratorSliceIter.next
    rw [dif_neg nativeBound]
  unfold internal_auth.exchange.prior_actors_loop.body
  rw [nextStep]
  simp only [bind_ok]
  with_unfolding_all rfl

/-- Exact slice cursor, including arbitrary consumed prefixes. A strictly
shrinking suffix measure proves the real generated loop terminates. -/
theorem prior_loop_exact (cursor : Aeneas.Std.core.slice.iter.Iter internal_auth.ActingServer)
    (initial : Option internal_auth.PriorActor) :
    internal_auth.exchange.prior_actors_loop cursor initial =
      .ok (chainFold (cursor.slice.val.drop cursor.i) initial) := by
  apply exact_of_total
  unfold internal_auth.exchange.prior_actors_loop
  let target := chainFold (cursor.slice.val.drop cursor.i) initial
  refine loop.spec_decr_nat (fun state => (state.1.slice.val.drop state.1.i).length)
    (fun state => chainFold (state.1.slice.val.drop state.1.i) state.2 = target)
    (fun result => result = target) _ _ ?_ rfl
  rintro ⟨current, prior⟩ invariant
  dsimp only
  by_cases active : current.i < current.slice.val.length
  · have suffix := List.drop_eq_getElem_cons active
    rw [prior_body_active current prior active]
    simp only [WP.spec_ok]
    constructor
    · simpa only [suffix, chainFold, List.foldl_cons] using invariant
    · simp only [List.length_drop]
      omega
  · have exhausted : current.slice.val.length ≤ current.i := Nat.le_of_not_gt active
    rw [prior_body_done current prior active]
    simpa [List.drop_eq_nil_of_le exhausted, chainFold, WP.spec_ok] using invariant

theorem forwarded_actors_exact (context : Option internal_auth.exchange.DelegationContext)
    (acting : Option internal_auth.ActingServer) :
    internal_auth.exchange.forwarded_actors context acting = .ok (expectedPrior context acting) := by
  unfold internal_auth.exchange.forwarded_actors
  cases acting with
  | none => rfl
  | some current =>
    unfold core.option.Option.map
    simp only [internal_auth.exchange.forwarded_actors.closure.Insts.CoreOpsFunctionFnOnceTupleShared2ActingServerPriorActor.call_once]
    unfold internal_auth.exchange.prior_actors
    cases context with
    | none => simp [expectedPrior,
        veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
    | some inherited =>
      simp only [SharedAVec.Insts.CoreIterTraitsCollectIntoIteratorSharedATIter.into_iter,
        core.slice.Slice.iter, bind_ok, bind_tc_ok]
      rw [prior_loop_exact]
      simp [chainFold, expectedPrior, alloc.vec.Vec.deref, veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone,
        bind_ok, bind_tc_ok]

attribute [local simp]
  alloc.string.String.Insts.CoreCloneClone.clone
  chrono.datetime.DateTime.Insts.CoreCloneClone.clone
  chrono.offset.utc.Utc.Insts.CoreCloneClone.clone
  veoveo_audit_contract.model.AuditRequest.Insts.CoreCloneClone.clone
  veoveo_types.authority.InvocationAuthority.Insts.CoreCloneClone.clone
  veoveo_types.digest.Sha256Digest.Insts.CoreCloneClone.clone
  veoveo_types.execution.DeploymentId.Insts.CoreCloneClone.clone
  veoveo_types.identity.DataLabelId.Insts.CoreCloneClone.clone
  veoveo_types.identity.PrincipalId.Insts.CoreCloneClone.clone
  veoveo_types.identity.TenantId.Insts.CoreCloneClone.clone
  veoveo_types.identity.WorkContextId.Insts.CoreCloneClone.clone
  veoveo_types.identity.DelegationId.Insts.CoreCloneClone.clone
  veoveo_types.identity.GroupId.Insts.CoreCloneClone.clone
  veoveo_types.identity.RoleId.Insts.CoreCloneClone.clone
  veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone
  veoveo_types.identity.TokenSubject.Insts.CoreCloneClone.clone
  veoveo_types.names.ScopeName.Insts.CoreCloneClone.clone
  veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone.clone
  veoveo_types.platform_names.ServerSlug.Insts.CoreCloneClone.clone
  veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone
  veoveo_types.provenance.InvocationMode.Insts.CoreCloneClone.clone
private theorem group_role_clone (value : identity.GroupRole) :
    identity.GroupRole.Insts.CoreCloneClone.clone value = .ok value := by cases value <;> rfl
private theorem assurance_clone (value : identity.PrincipalAssurance) :
    identity.PrincipalAssurance.Insts.CoreCloneClone.clone value = .ok value := by cases value <;> rfl
private theorem kind_clone (value : identity.PrincipalKind) :
    identity.PrincipalKind.Insts.CoreCloneClone.clone value = .ok value := by cases value <;> rfl
attribute [local simp] group_role_clone assurance_clone kind_clone
  identity.JwtId.Insts.CoreCloneClone.clone identity.ProtectedResourceId.Insts.CoreCloneClone.clone
  identity.HubRefreshFamilyId.Insts.CoreCloneClone.clone
  identity.HubProfileId.Insts.CoreCloneClone.clone identity.OpaqueTaskId.Insts.CoreCloneClone.clone

private theorem group_membership_clone (value : identity.GroupMembership) :
    identity.GroupMembership.Insts.CoreCloneClone.clone value = .ok value := by
  simp [identity.GroupMembership.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
attribute [local simp] group_membership_clone

private theorem groups_clone (value : alloc.collections.btree.set.BTreeSet veoveo_types.identity.GroupId alloc.alloc.Global) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone veoveo_types.identity.GroupId.Insts.CoreCloneClone
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone value = .ok value := by
  apply set_clone_exact
  intro item
  rfl
attribute [local simp] groups_clone
private theorem roles_clone (value : alloc.collections.btree.set.BTreeSet veoveo_types.identity.RoleId alloc.alloc.Global) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone veoveo_types.identity.RoleId.Insts.CoreCloneClone
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone value = .ok value := by
  apply set_clone_exact
  intro item
  rfl
attribute [local simp] roles_clone
private theorem scopes_clone (value : alloc.collections.btree.set.BTreeSet veoveo_types.names.ScopeName alloc.alloc.Global) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone veoveo_types.names.ScopeName.Insts.CoreCloneClone
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone value = .ok value := by
  apply set_clone_exact
  intro item
  rfl
attribute [local simp] scopes_clone
private theorem labels_clone (value : alloc.collections.btree.set.BTreeSet veoveo_types.identity.DataLabelId alloc.alloc.Global) :
    alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone veoveo_types.identity.DataLabelId.Insts.CoreCloneClone
      alloc.alloc.Global.Insts.CoreAllocAllocatorClone value = .ok value := by
  apply set_clone_exact
  intro item
  rfl
attribute [local simp] labels_clone
private theorem tenant_clone (value : Option veoveo_types.identity.TenantId) :
    core.option.Option.Insts.CoreCloneClone.clone veoveo_types.identity.TenantId.Insts.CoreCloneClone value = .ok value := by
  apply option_clone_exact
  intro item
  rfl
attribute [local simp] tenant_clone
private theorem initiator_clone (value : Option veoveo_types.identity.PrincipalId) :
    core.option.Option.Insts.CoreCloneClone.clone veoveo_types.identity.PrincipalId.Insts.CoreCloneClone value = .ok value := by
  apply option_clone_exact
  intro item
  rfl
attribute [local simp] initiator_clone
private theorem delegation_id_clone (value : Option veoveo_types.identity.DelegationId) :
    core.option.Option.Insts.CoreCloneClone.clone veoveo_types.identity.DelegationId.Insts.CoreCloneClone value = .ok value := by
  apply option_clone_exact
  intro item
  rfl
attribute [local simp] delegation_id_clone
private theorem timestamp_option_clone (value : Option NativeTimestamp) :
    core.option.Option.Insts.CoreCloneClone.clone
      (chrono.datetime.DateTime.Insts.CoreCloneClone chrono.offset.utc.Utc.Insts.CoreCloneClone
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc chrono.offset.utc.Utc.Insts.CoreCloneClone)
      value = .ok value := by
  apply option_clone_exact
  intro item
  rfl
attribute [local simp] timestamp_option_clone

theorem principal_clone (value : Principal) :
    identity.Principal.Insts.CoreCloneClone.clone value = .ok value := by
  simp [identity.Principal.Insts.CoreCloneClone.clone, set_clone_exact,
    option_clone_exact, bind_ok, bind_tc_ok]
attribute [local simp] principal_clone

private theorem access_subject_clone (value : identity.AccessTokenSubject) :
    identity.AccessTokenSubject.Insts.CoreCloneClone.clone value = .ok value := by
  simp [identity.AccessTokenSubject.Insts.CoreCloneClone.clone, set_clone_exact,
    option_clone_exact, bind_ok, bind_tc_ok]
attribute [local simp] access_subject_clone

theorem context_clone (value : Context) :
    internal_auth.request.HubRequestContext.Insts.CoreCloneClone.clone value = .ok value := by
  simp [internal_auth.request.HubRequestContext.Insts.CoreCloneClone.clone, option_clone_exact,
    execution.ExecutionContext.Insts.CoreCloneClone.clone,
    execution.CompositionRevision.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
attribute [local simp] context_clone

private theorem acting_clone (value : internal_auth.ActingServer) :
    internal_auth.ActingServer.Insts.CoreCloneClone.clone value = .ok value := by
  simp [internal_auth.ActingServer.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
attribute [local simp] acting_clone

private theorem vector_clone {T : Type} (clone : core.clone.Clone T)
    (law : ∀ value, clone.clone value = .ok value) (values : alloc.vec.Vec T) :
    alloc.vec.CloneVec.clone clone values = .ok values := by
  unfold alloc.vec.CloneVec.clone
  have copied := Slice.clone_spec (s := values.slice) (clone := clone.clone)
    (by intro value member; exact law value)
  have exactCopy := exact_of_total _ _ (WP.spec_mono copied (by intro output same; exact same.symm))
  rw [exactCopy]
  simp only [bind_ok, bind_tc_ok]

theorem delegation_clone (value : internal_auth.exchange.DelegationContext) :
    internal_auth.exchange.DelegationContext.Insts.CoreCloneClone.clone value = .ok value := by
  simp [internal_auth.exchange.DelegationContext.Insts.CoreCloneClone.clone, vector_clone,
    option_clone_exact, internal_auth.exchange.DelegatingTask.Insts.CoreCloneClone.clone,
    bind_ok, bind_tc_ok]
attribute [local simp] delegation_clone

/-- Complete deterministic independently specified claims value. Its relation to
ClaimsExact below is definitional; actual claims construction is proved separately. -/
def claimsValue (token : Identity) : internal_auth.AccessTokenClaims := {
  standard := {
    iss := token.issuer, sub := token.actor.id.value, aud := token.audience.value,
    client_id := token.client, exp := token.expires_at.seconds,
    nbf := token.not_before.seconds, iat := token.issued_at.seconds, jti := token.jwt_id,
    act := { sub := token.issuer, act := expectedPrior token.delegation_context token.acting_server },
    scope := expectedScope token.actor, groups := token.actor.groups, roles := token.actor.roles }
  authorization_details := alloc.vec.Vec.from (if token.permissions.values = [] then [] else
    [{ kind := .Permissions, actions := token.permissions }]) (by
      split <;> simp only [List.length_nil, List.length_singleton] <;> scalar_tac)
  invocation := expectedInvocation token }

theorem claims_value_exact (token : Identity) : ClaimsExact token (claimsValue token) := by
  simp [ClaimsExact, claimsValue]

theorem standard_claims_exact (token : Identity) (domain : ClaimsTextDomain token) :
    internal_auth.StandardClaims.new token.issuer token.client token.actor
      (encoded token.audience.value domain.2.1) token.acting_server token.delegation_context
      token.jwt_id token.issued_at token.not_before token.expires_at =
      .ok (claimsValue token).standard := by
  unfold internal_auth.StandardClaims.new
  simp only [veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok,
    veoveo_types.identity.PrincipalId.as_str]
  rw [encode_exact _ domain.1]
  simp only [bind_ok, bind_tc_ok, Str.Insts.AllocBorrowToOwnedString.to_owned,
    decode_exact]
  rw [forwarded_actors_exact, scope_claim_exact _ domain.2.2.1]
  simp [claimsValue, set_clone_exact, chrono.datetime.DateTime.timestamp,
    bind_ok, bind_tc_ok]

theorem option_vec_collect {T : Type} (value : Option T) :
    core.iter.traits.iterator.Iterator.collect.default
      (core.option.IntoIter.Insts.CoreIterTraitsIteratorIterator T)
      (core.iter.traits.collect.FromIteratorVec T) value =
      .ok (alloc.vec.Vec.from value.toList (by cases value <;> simp <;> scalar_tac)) := by
  let iterator := core.option.IntoIter.Insts.CoreIterTraitsIteratorIterator T
  have exhausted (accumulator : List T) :
      alloc.vec.FromIteratorVec.iterToList iterator none accumulator = .ok accumulator.reverse := by
    rw [alloc.vec.FromIteratorVec.iterToList]
    simp [iterator, core.option.IntoIter.Insts.CoreIterTraitsIteratorIterator.next,
      bind_ok, bind_tc_ok]
  unfold core.iter.traits.iterator.Iterator.collect.default
    core.iter.traits.collect.FromIteratorVec alloc.vec.FromIteratorVec.from_iter
  cases value with
  | none =>
    simp only [core.iter.traits.collect.IntoIterator.Blanket.into_iter, bind_ok, bind_tc_ok]
    rw [exhausted]
    simp
  | some item =>
    simp only [core.iter.traits.collect.IntoIterator.Blanket.into_iter, bind_ok, bind_tc_ok]
    have one : alloc.vec.FromIteratorVec.iterToList iterator (some item) [] = .ok [item] := by
      rw [alloc.vec.FromIteratorVec.iterToList]
      simp only [iterator, core.option.IntoIter.Insts.CoreIterTraitsIteratorIterator.next,
        bind_ok, bind_tc_ok]
      with_unfolding_all
        change alloc.vec.FromIteratorVec.iterToList iterator none [item] = Result.ok [item]
      exact exhausted [item]
    rw [one]
    have oneBound : 1 ≤ Usize.max := by scalar_tac
    simp [oneBound]

#print axioms scope_loop_exact
#print axioms scope_claim_exact
#print axioms prior_loop_exact
#print axioms forwarded_actors_exact
#print axioms claims_value_exact
#print axioms standard_claims_exact
#print axioms option_vec_collect

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthClaimsComponents.scope_loop_exact,
      ``InternalAuthClaimsComponents.scope_claim_exact, ``InternalAuthClaimsComponents.prior_loop_exact,
      ``InternalAuthClaimsComponents.forwarded_actors_exact, ``InternalAuthClaimsComponents.claims_value_exact,
      ``InternalAuthClaimsComponents.standard_claims_exact, ``InternalAuthClaimsComponents.option_vec_collect] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed claims component boundary in {declaration}: {found}"

end
end InternalAuthClaimsComponents
