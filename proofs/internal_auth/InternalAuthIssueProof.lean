import InternalAuthIssueSpecification
import InternalAuthClaimsProof
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof InternalAuthIdentityProof
open InternalAuthClaimsComponents InternalAuthIssueSpecification

namespace InternalAuthIssueProof
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate
abbrev Permission := veoveo_types.permission_names.PermissionName
abbrev Server := veoveo_types.platform_names.ServerSlug
abbrev Issuer := internal_auth.HubInternalTokenIssuer
abbrev IssueResult := core.result.Result internal_auth.IssuedHubInternalToken internal_auth.InternalTokenError

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
theorem ownership_model_exact (permission : Permission) (server : Server)
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

private theorem predicate_exact (permission : Permission) (server : Server)
    (permissionBound : TextRepresentable permission.value) (serverBound : TextRepresentable server.value) :
    permissionPredicate.call_mut server permission =
      .ok (decide (¬ Owned permission server), server) := by
  change internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool.call_mut
    server permission = _
  unfold internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool.call_mut
  rw [ownership_model_exact permission server permissionBound serverBound]
  by_cases owned : Owned permission server <;> simp [owned, bind_ok, bind_tc_ok]

private theorem scan_nil (server : Server) :
    Native.find permissionIterator permissionPredicate [] server = .ok (none, []) := by
  rw [Native.find]
  simp only [permissionIterator,
    alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next, bind_ok]
  with_unfolding_all rfl

private theorem scan_cons (head : Permission) (tail : List Permission) (server : Server)
    (headBound : TextRepresentable head.value) (serverBound : TextRepresentable server.value) :
    Native.find permissionIterator permissionPredicate (head :: tail) server =
      (if Owned head server then Native.find permissionIterator permissionPredicate tail server
        else .ok (some head, tail)) := by
  rw [Native.find]
  simp only [permissionIterator,
    alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next, bind_ok]
  with_unfolding_all
    change Aeneas.Std.bind (permissionPredicate.call_mut server head) (fun tested =>
      if tested.1 then Result.ok (some head, tail)
      else Native.find permissionIterator permissionPredicate tail tested.2) = _
  rw [predicate_exact head server headBound serverBound]
  by_cases owned : Owned head server <;> simp [owned, bind_ok]

/-- Termination and exact first bad permission for the actual finite find
recurrence. No native set ordering or desired result is assumed. -/
theorem first_foreign_scan (permissions : List Permission) (server : Server)
    (serverBound : TextRepresentable server.value)
    (bounds : ∀ permission ∈ permissions, TextRepresentable permission.value) :
    Native.find permissionIterator permissionPredicate permissions server ⦃ found =>
      found.1 = firstForeign permissions server ⦄ := by
  cases permissions with
  | nil => simp [scan_nil, firstForeign, WP.spec_ok]
  | cons head tail =>
    rw [scan_cons head tail server (bounds head List.mem_cons_self) serverBound]
    by_cases owned : Owned head server
    · rw [if_pos owned]
      have recursive := first_foreign_scan tail server serverBound
        (fun permission member => bounds permission (List.mem_cons_of_mem head member))
      apply WP.spec_mono recursive
      intro found specification
      simpa [firstForeign, owned] using specification
    · simp [owned, firstForeign, WP.spec_ok]
termination_by permissions.length

private theorem scan_returns (permissions : List Permission) (server : Server)
    (serverBound : TextRepresentable server.value)
    (bounds : ∀ permission ∈ permissions, TextRepresentable permission.value) :
    ∃ remaining, Native.find permissionIterator permissionPredicate permissions server =
      .ok (firstForeign permissions server, remaining) := by
  have total := first_foreign_scan permissions server serverBound bounds
  cases operation : Native.find permissionIterator permissionPredicate permissions server using Result.cases with
  | ret found =>
    rw [operation] at total
    have first := (WP.spec_ok found).mp total
    cases found with
    | mk entry remaining =>
      refine ⟨remaining, ?_⟩
      exact congrArg Result.ok (Prod.ext first rfl)
  | vis effect continuation =>
    rw [operation] at total
    exact False.elim ((WP.spec_vis effect continuation).mp total)
  | div =>
    rw [operation] at total
    exact False.elim (WP.spec_div.mp total)

/-- Claims-domain closure is derived from the actual independent constructor's
complete field preservation, not assumed as an own-function result. -/
private theorem constructed_claims_domain (issuer : Issuer) (request : Request)
    (domain : IssueTextDomain request) (now expires : NativeTimestamp) (jwtId : identity.JwtId) :
    ClaimsTextDomain (InternalAuthCorrectness.identityValue issuer request now expires jwtId) := by
  exact ⟨domain.actor_id, domain.audience, domain.scopes, domain.rendered_scopes⟩
private def signContinuation (issuer : Issuer) (token : Identity)
    (claims : internal_auth.AccessTokenClaims) : Result IssueResult := do
  let signed ← internal_auth.HubInternalTokenIssuer.sign_forwarded issuer claims
  match signed with
  | .Err error => .ok (.Err error)
  | .Ok bearer => .ok (.Ok { bearer_token := bearer, identity := token })

/-- Both sides retain every primitive effect/failure/divergence of the already
proved identity outcome. Only its constructed successful records require claims
encoding, and their domain is derived from request preservation. -/
private theorem claims_after_identity (issuer : Issuer) (request : Request)
    (domain : IssueTextDomain request) :
    (do
      let result ← identityOutcome issuer request
      match result with
      | .Err error => Result.ok (core.result.Result.Err error)
      | .Ok token => do
        let claims ← internal_auth.AccessTokenClaims.from_identity token
        signContinuation issuer token claims) =
    (do
      let result ← identityOutcome issuer request
      match result with
      | .Err error => Result.ok (core.result.Result.Err error)
      | .Ok token => signContinuation issuer token (claimsValue token)) := by
  unfold identityOutcome
  simp only [bind_tc_eq, Aeneas.Std.bind_assoc]
  apply congrArg (fun continuation : Unit → Result IssueResult =>
    Aeneas.Std.bind internal_auth.ensure_jwt_crypto_provider continuation)
  funext providerUnit
  cases delegated : delegationError request with
  | some error => simp [delegated, bind_ok]
  | none =>
    simp only [delegated, bind_tc_eq, Aeneas.Std.bind_assoc]
    apply congrArg (fun continuation : NativeTimestamp → Result IssueResult =>
      Aeneas.Std.bind chrono.offset.utc.Utc.now continuation)
    funext now
    by_cases context : requestContextAccepted request
    · by_cases future : Native.compareTimestamp (effectiveDeadline request) now = .gt
      · simp only [if_pos context, if_pos future, bind_tc_eq, Aeneas.Std.bind_assoc]
        apply congrArg (fun continuation : uuid.Uuid → Result IssueResult =>
          Aeneas.Std.bind uuid.v7.Uuid.now_v7 continuation)
        funext uuidValue
        apply congrArg (fun continuation : String → Result IssueResult =>
          Aeneas.Std.bind (alloc.string.ToString.Blanket.to_string
            uuid.Uuid.Insts.CoreFmtDisplay uuidValue) continuation)
        funext uuidText
        apply congrArg (fun continuation :
            core.result.Result identity.JwtId veoveo_types.error.IdentifierError → Result IssueResult =>
          Aeneas.Std.bind (identity.JwtId.parse alloc.string.String.Insts.CoreConvertAsRefStr uuidText)
            continuation)
        funext parsed
        cases parsed with
        | Err error => simp [bind_ok]
        | Ok jwtId =>
          simp only [bind_tc_eq, bind_ok, Aeneas.Std.bind_assoc]
          rw [InternalAuthClaimsProof.from_identity_exact _
            (constructed_claims_domain issuer request domain now (effectiveDeadline request) jwtId)]
          simp only [bind_ok]
      · simp [context, future, bind_ok]
    · simp [context, bind_ok]

private theorem sign_try_exact (issuer : Issuer) (token : Identity)
    (claims : internal_auth.AccessTokenClaims) :
    (do
      let signed ← internal_auth.HubInternalTokenIssuer.sign_forwarded issuer claims
      let control ← core.result.Result.Insts.CoreOpsTry.branch signed
      match control with
      | .Continue bearer => Result.ok (core.result.Result.Ok
          ({bearer_token := bearer, identity := token} : internal_auth.IssuedHubInternalToken))
      | .Break residual =>
        core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual
          internal_auth.IssuedHubInternalToken (core.convert.FromSame internal_auth.InternalTokenError) residual) =
      signContinuation issuer token claims := by
  unfold signContinuation
  apply congrArg (fun continuation : core.result.Result String internal_auth.InternalTokenError → Result IssueResult =>
    Aeneas.Std.bind (internal_auth.HubInternalTokenIssuer.sign_forwarded issuer claims) continuation)
  funext signed
  cases signed <;>
    simp [core.result.Result.Insts.CoreOpsTry.branch,
      core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
      core.convert.FromSame, bind_ok]

/-- Guard priority is a value equation independent of provider/time/ID/signing
observations, not an instrumented native call-count or side-effect trace. -/
theorem foreign_permission_refused (issuer : Issuer) (request : Request)
    (domain : PermissionTextDomain request) (foreign : Permission)
    (first : firstForeign request.permissions.values request.server = some foreign) :
    internal_auth.HubInternalTokenIssuer.issue issuer request =
      .ok (.Err (.PermissionOutsideAudience foreign)) := by
  obtain ⟨remaining, scan⟩ := scan_returns request.permissions.values request.server
    domain.server domain.permissions
  unfold internal_auth.HubInternalTokenIssuer.issue
  simp only [alloc.collections.btree.set.BTreeSet.iter, bind_ok,
    core.iter.traits.iterator.Iterator.find.default]
  rw [scan]
  simp only [bind_ok, first]
  with_unfolding_all
    change Aeneas.Std.bind
      (veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone.clone foreign)
      (fun value => Result.ok (core.result.Result.Err
        (internal_auth.InternalTokenError.PermissionOutsideAudience value))) = _
  simp only [veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone.clone, bind_ok]

private theorem no_foreign_issue (issuer : Issuer) (request : Request)
    (domain : IssueTextDomain request)
    (clean : firstForeign request.permissions.values request.server = none) :
    internal_auth.HubInternalTokenIssuer.issue issuer request =
    (do
      let result ← identityOutcome issuer request
      match result with
      | .Err error => Result.ok (core.result.Result.Err error)
      | .Ok token => signContinuation issuer token (claimsValue token)) := by
  obtain ⟨remaining, scan⟩ := scan_returns request.permissions.values request.server
    domain.permission_text.server domain.permission_text.permissions
  unfold internal_auth.HubInternalTokenIssuer.issue
  simp only [alloc.collections.btree.set.BTreeSet.iter, bind_ok,
    core.iter.traits.iterator.Iterator.find.default]
  rw [scan]
  simp only [bind_ok, clean]
  rw [create_identity_exact issuer request domain.context_text]
  calc
    _ = (do
      let result ← identityOutcome issuer request
      match result with
      | .Err error => Result.ok (core.result.Result.Err error)
      | .Ok token => do
        let claims ← internal_auth.AccessTokenClaims.from_identity token
        signContinuation issuer token claims) := by
      apply congrArg (fun continuation : core.result.Result Identity internal_auth.InternalTokenError → Result IssueResult =>
        Aeneas.Std.bind (identityOutcome issuer request) continuation)
      funext result
      cases result with
      | Err error =>
        simp [core.result.Result.Insts.CoreOpsTry.branch,
          core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
          core.convert.FromSame, bind_ok]
      | Ok token =>
        simp only [core.result.Result.Insts.CoreOpsTry.branch, bind_ok]
        apply congrArg (fun continuation : internal_auth.AccessTokenClaims → Result IssueResult =>
          Aeneas.Std.bind (internal_auth.AccessTokenClaims.from_identity token) continuation)
        funext claims
        exact sign_try_exact issuer token claims
    _ = _ := claims_after_identity issuer request domain

/-- Complete actual public issuer in the current operational graph: exact first
foreign error, every independent identity outcome, complete signer argument and
all signer Rust errors/native failures/divergence. Core namespace/native-library
refinement boundaries remain explicit; no successful own output is a premise. -/
theorem issue_exact (issuer : Issuer) (request : Request) (domain : IssueTextDomain request) :
    internal_auth.HubInternalTokenIssuer.issue issuer request = issueOutcome issuer request := by
  unfold issueOutcome
  cases first : firstForeign request.permissions.values request.server with
  | some foreign => exact foreign_permission_refused issuer request domain.permission_text foreign first
  | none => exact no_foreign_issue issuer request domain first

#print axioms ownership_model_exact
#print axioms first_foreign_scan
#print axioms foreign_permission_refused
#print axioms issue_exact
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let nativeIdentity := #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse,
    ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.sign_forwarded]
  for declaration in #[``InternalAuthIssueProof.ownership_model_exact,
      ``InternalAuthIssueProof.first_foreign_scan] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed ownership model/finite scan boundary in {declaration}: {found}"
  for declaration in #[``InternalAuthIssueProof.foreign_permission_refused,
      ``InternalAuthIssueProof.issue_exact] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !(standard ++ nativeIdentity).contains axiomName)).isEmpty do
      throwError "unreviewed complete public issuer boundary in {declaration}: {found}"
end
end InternalAuthIssueProof
