import InternalAuthIssueExamples
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof InternalAuthContextExamples InternalAuthIdentityExamples
open InternalAuthIdentityProof InternalAuthClaimsComponents InternalAuthIssueSpecification
open InternalAuthIssueExamples

namespace IssueGuardInversionWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate
abbrev Permission := veoveo_types.permission_names.PermissionName
abbrev Server := veoveo_types.platform_names.ServerSlug

-- Proof derivations of unchanged operational string/namespace wrappers only.
-- No IssueProof or IssueWitnesses import or artifact is used. Native Core-helper
-- refinement remains explicit, exactly as in the baseline operational graph.
private theorem encoded_values (text : String) (bound : TextRepresentable text) :
    (encoded text bound).val = utf8Bytes text := by
  unfold encoded
  rw [Slice.from_val]
  with_unfolding_all rfl

private theorem bytes_bounded (text : String) (bound : TextRepresentable text) :
    (utf8Bytes text).length ≤ Usize.max := by
  simpa only [utf8Bytes, List.length_map, ByteArray.length_toList, TextRepresentable] using bound

private theorem prefix_bounded (text : String) (bound : TextRepresentable text) :
    ((utf8Bytes text).takeWhile (fun byte => decide (byte.val ≠ 46))).length ≤ Usize.max := by
  exact (List.takeWhile_sublist _).length_le.trans (bytes_bounded text bound)

private def prefixSlice (text : String) (bound : TextRepresentable text) : Str :=
  Slice.from ((utf8Bytes text).takeWhile (fun byte => decide (byte.val ≠ 46)))
    (prefix_bounded text bound)

/-- Current operational Core-helper model only. Native transparency is blocked at
str::pattern::Pattern; this does not close that source-refinement boundary. -/
private theorem prefix_model_exact (text : String) (bound : TextRepresentable text) :
    Native.namespacePrefix (encoded text bound) = .ok (prefixSlice text bound) := by
  unfold Native.namespacePrefix
  rw [encoded_values]
  change (if sizeBound : ((utf8Bytes text).takeWhile (fun byte => decide (byte.val ≠ 46))).length ≤ Usize.max
    then Result.ok (Slice.from _ sizeBound) else Result.fail .maximumSizeExceeded) = _
  rw [dif_pos (prefix_bounded text bound)]
  rfl

private theorem slice_eq_iff (left right : Str) : left = right ↔ left.val = right.val := by
  exact Slice.eq_iff left right

/-- Extracted wrappers plus the existing explicit namespace model. The independent
predicate describes first-dot bytes; no own-code success is an input premise. -/
private theorem ownership_model_exact (permission : Permission) (server : Server)
    (permissionBound : TextRepresentable permission.value)
    (serverBound : TextRepresentable server.value) :
    veoveo_types.permission_names.PermissionName.is_owned_by permission server =
      .ok (decide (Owned permission server)) := by
  unfold veoveo_types.permission_names.PermissionName.is_owned_by
    veoveo_types.permission_names.PermissionName.namespace
    veoveo_types.permission_names.PermissionName.as_str
  rw [encode_exact _ permissionBound]
  simp only [bind_ok, veoveo_types.permission_names.namespace]
  rw [prefix_model_exact]
  simp only [bind_ok, veoveo_types.platform_names.ServerSlug.as_str]
  rw [encode_exact _ serverBound]
  simp only [bind_tc_eq, bind_ok, Str.Insts.CoreCmpPartialEqStr.eq]
  have comparison : prefixSlice permission.value permissionBound = encoded server.value serverBound ↔
      Owned permission server := by
    apply (slice_eq_iff (prefixSlice permission.value permissionBound)
      (encoded server.value serverBound)).trans
    simp only [prefixSlice, Slice.from_val, encoded_values, Owned]
  apply congrArg Result.ok
  by_cases owned : Owned permission server
  · have nativeEq := comparison.mpr owned
    simp [nativeEq, owned]
  · have nativeNe : prefixSlice permission.value permissionBound ≠ encoded server.value serverBound :=
      fun same => owned (comparison.mp same)
    simp [nativeNe, owned]

private abbrev permissionIterator :=
  alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT Permission
private abbrev permissionPredicate :=
  internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool

def ownedPermission : Permission := ⟨"operator.inspect"⟩

theorem independent_admission :
    firstForeign ordinaryRequest.permissions.values ordinaryRequest.server = none ∧
    delegationError ordinaryRequest = none ∧ requestContextAccepted ordinaryRequest ∧
    effectiveDeadline ordinaryRequest = seedExpiresAt ∧
    Native.compareTimestamp seedExpiresAt seedIssuedAt = .gt := by
  exact ⟨ordinary_constraints.1, ordinary_constraints.2.1,
    ordinary_constraints.2.2.1, ordinary_constraints.2.2.2.1,
    ordinary_constraints.2.2.2.2.2⟩

private theorem first_owned : Owned ownedPermission ordinaryRequest.server := by
  have clean := ordinary_constraints.1
  change firstForeign [ownedPermission, ⟨"operator.operate"⟩] ordinaryRequest.server = none at clean
  by_contra foreign
  simp [firstForeign, foreign] at clean

private theorem predicate_selects_owned :
    internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool.call_mut
      ordinaryRequest.server ownedPermission =
      .ok (true, ordinaryRequest.server) := by
  unfold internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool.call_mut
  rw [ownership_model_exact ownedPermission ordinaryRequest.server
    (ordinary_domain.permission_text.permissions ownedPermission (by simp [ordinaryRequest,
      InternalAuthClaimsExamples.richIdentity, ownedPermission])) ordinary_domain.permission_text.server]
  simp only [first_owned, decide_true, bind_ok, bind_tc_ok]

private theorem first_scan_selects_owned :
    Native.find permissionIterator permissionPredicate
      [ownedPermission, ⟨"operator.operate"⟩] ordinaryRequest.server =
      .ok (some ownedPermission, [⟨"operator.operate"⟩]) := by
  rw [Native.find]
  simp only [permissionIterator,
    alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next, bind_ok]
  with_unfolding_all
    change Aeneas.Std.bind
      (internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool.call_mut
        ordinaryRequest.server ownedPermission)
      (fun tested => if tested.1 then Result.ok (some ownedPermission, [⟨"operator.operate"⟩])
        else Native.find permissionIterator permissionPredicate [⟨"operator.operate"⟩] tested.2) = _
  rw [predicate_selects_owned]
  simp only [bind_ok, ite_true]

/-- Actual mutant refuses the first owned permission of an independently admitted
populated request. This result requires no native provider/clock/ID/signer premise. -/
theorem mutant_refuses_owned (issuer : internal_auth.HubInternalTokenIssuer) :
    internal_auth.HubInternalTokenIssuer.issue issuer ordinaryRequest =
      .ok (.Err (.PermissionOutsideAudience ownedPermission)) := by
  unfold internal_auth.HubInternalTokenIssuer.issue
  simp only [alloc.collections.btree.set.BTreeSet.iter, bind_ok,
    core.iter.traits.iterator.Iterator.find.default]
  have values : ordinaryRequest.permissions.values =
      [ownedPermission, ⟨"operator.operate"⟩] := rfl
  rw [values, first_scan_selects_owned]
  simp only [bind_tc_eq, bind_ok, bind_tc_ok,
    veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone.clone]
  with_unfolding_all rfl

def expectedIdentity (issuer : internal_auth.HubInternalTokenIssuer) (jwtId : identity.JwtId) : Identity :=
  InternalAuthCorrectness.identityValue issuer ordinaryRequest seedIssuedAt seedExpiresAt jwtId

/-- Reference success is derived from the independent input decisions and native
observations, never assumed as an issuer/create_identity/issueOutcome success. -/
theorem reference_succeeds (issuer : internal_auth.HubInternalTokenIssuer)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt)
    (bearer : String)
    (signed : internal_auth.HubInternalTokenIssuer.sign_forwarded issuer
      (claimsValue (expectedIdentity issuer jwtId)) = .ok (.Ok bearer)) :
    issueOutcome issuer ordinaryRequest =
      .ok (.Ok {bearer_token := bearer, identity := expectedIdentity issuer jwtId}) := by
  unfold issueOutcome
  rw [ordinary_constraints.1]
  unfold identityOutcome
  rw [provider]
  simp only [bind_ok, bind_tc_ok, ordinary_constraints.2.1]
  rw [clock]
  simp only [bind_ok, bind_tc_ok, ordinary_constraints.2.2.1, ite_true,
    ordinary_constraints.2.2.2.1, ordinary_constraints.2.2.2.2.2, ite_true]
  rw [observation.uuid_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.text_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.parse_observed]
  simp only [bind_ok, bind_tc_ok, parsed]
  change Aeneas.Std.bind
    (internal_auth.HubInternalTokenIssuer.sign_forwarded issuer
      (claimsValue (expectedIdentity issuer jwtId))) _ = _
  rw [signed]
  simp only [bind_ok, bind_tc_ok, expectedIdentity]

/-- Conditional only on external observations, the actual wrong refusal differs
from successful complete reference behavior. Signer error freedom is not assumed
for arbitrary observations. -/
theorem wrong_refusal_violates_reference (issuer : internal_auth.HubInternalTokenIssuer)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt)
    (bearer : String)
    (signed : internal_auth.HubInternalTokenIssuer.sign_forwarded issuer
      (claimsValue (expectedIdentity issuer jwtId)) = .ok (.Ok bearer)) :
    internal_auth.HubInternalTokenIssuer.issue issuer ordinaryRequest ≠
      issueOutcome issuer ordinaryRequest := by
  rw [mutant_refuses_owned, reference_succeeds issuer observation jwtId parsed provider clock bearer signed]
  intro same
  rw [Result.ok.injEq] at same
  cases same

#print axioms independent_admission
#print axioms mutant_refuses_owned
#print axioms reference_succeeds
#print axioms wrong_refusal_violates_reference
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let nativeIssue := #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse,
    ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.sign_forwarded]
  let admission ← Lean.collectAxioms ``IssueGuardInversionWitness.independent_admission
  unless (admission.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
    throwError "unreviewed independent issuer admission: {admission}"
  for declaration in #[``IssueGuardInversionWitness.mutant_refuses_owned,
      ``IssueGuardInversionWitness.reference_succeeds,
      ``IssueGuardInversionWitness.wrong_refusal_violates_reference] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !(standard ++ nativeIssue).contains axiomName)).isEmpty do
      throwError "unreviewed public issuer guard fault witness: {declaration}: {found}"
end
end IssueGuardInversionWitness
