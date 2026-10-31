import RequestCompositionSdk.Funs
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk

namespace InternalAuthCorrectness
noncomputable section
local instance (p : Prop) : Decidable p := Classical.propDecidable p

abbrev Request := internal_auth.ForwardedRequest
abbrev Identity := internal_auth.HubInternalIdentity
abbrev Issuer := internal_auth.HubInternalTokenIssuer
abbrev TokenError := internal_auth.InternalTokenError
abbrev Time := NativeTimestamp

/-- Independent complete identity value. The expiry argument is supplied by the
deadline contract, not by an assumed result of the issuer. NumericDate conversion
belongs to claims construction; these fields retain the full native timestamp. -/
def identityValue (issuer : Issuer) (request : Request)
    (now expires : Time) (jwtId : identity.JwtId) : Identity := {
  issuer := issuer.issuer, profile := request.profile, server := request.server,
  deployment := request.deployment, audience := request.audience,
  client := request.client, actor := request.actor, authority := request.authority,
  permissions := request.permissions, request_context := request.request_context,
  acting_server := request.acting_server, acting_task := request.acting_task,
  delegation_context := request.delegation_context, jwt_id := jwtId,
  issued_at := now, not_before := now, expires_at := expires }

/-- Observed native ID outcomes, retaining both Rust success and Rust parse error.
There is no assumption about fresh_jwt_id or another own-code function. These
equations describe value observations, not effect counts or a side-effect trace. -/
structure IdObservation where
  sampleUuid : uuid.Uuid
  text : String
  parsed : core.result.Result identity.JwtId veoveo_types.error.IdentifierError
  uuid_observed : uuid.v7.Uuid.now_v7 = .ok sampleUuid
  text_observed : alloc.string.ToString.Blanket.to_string
    uuid.Uuid.Insts.CoreFmtDisplay sampleUuid = .ok text
  parse_observed : identity.JwtId.parse
    alloc.string.String.Insts.CoreConvertAsRefStr text = .ok parsed

def idOutcome (observation : IdObservation) : core.result.Result identity.JwtId TokenError :=
  match observation.parsed with
  | .Ok jwtId => .Ok jwtId
  | .Err error => .Err (.Identifier error)

theorem fresh_jwt_id_exact (observation : IdObservation) :
    internal_auth.fresh_jwt_id = .ok (idOutcome observation) := by
  unfold internal_auth.fresh_jwt_id
  rw [observation.uuid_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.text_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.parse_observed]
  cases parsed : observation.parsed <;>
    simp [idOutcome, parsed, bind_ok, bind_tc_ok]

/-- A legitimate bootstrap omission allowed by the native API, used here only as
an initial integrated algebra check. Context-bearing direct and delegated requests
are separate obligations; this constructor does not pretend to discharge them. -/
def bootstrapRequest (request : Request) : Request := { request with
  permissions := ⟨[]⟩
  request_context := none
  acting_server := none
  acting_task := none
  delegation_context := none }

def bootstrapOutcome (issuer : Issuer) (request : Request) (now : Time)
    (observation : IdObservation) : core.result.Result Identity TokenError :=
  if Native.compareTimestamp request.expires_at now = .gt then
    match idOutcome observation with
    | .Ok jwtId => .Ok (identityValue issuer (bootstrapRequest request)
        now request.expires_at jwtId)
    | .Err error => .Err error
  else .Err .ExpiredDelegation

private theorem timestamp_le_exact (left right : Time) :
    core.cmp.PartialOrd.le.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      .ok (decide (Native.compareTimestamp left right ≠ .gt)) := by
  cases ordering : Native.compareTimestamp left right <;>
    simp [core.cmp.PartialOrd.le.trait_default,
      core.cmp.PartialOrd.le.default, core.cmp.PartialOrd.le_body,
      chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime,
      chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp,
      ordering, bind_ok, bind_tc_ok]

/-- Complete actual create_identity value and its expiry/ID-error branches for
bootstrap requests. Inputs are arbitrary complete typed payloads; only external
provider/time/ID observations are assumed. No own validation-success premise. -/
theorem bootstrap_identity_exact (issuer : Issuer) (request : Request) (now : Time)
    (observation : IdObservation)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok now) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer
      (bootstrapRequest request) = .ok (bootstrapOutcome issuer request now observation) := by
  unfold internal_auth.HubInternalTokenIssuer.create_identity
  rw [provider]
  simp only [bootstrapRequest, core.option.Option.as_ref,
    core.option.Option.map_or, internal_auth.exchange.validate_delegation,
    core.option.Option.is_none, core.option.Option.is_some,
    core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
  rw [clock]
  simp only [bind_ok, bind_tc_ok]
  rw [timestamp_le_exact]
  simp only [bind_ok, bind_tc_ok]
  by_cases future : Native.compareTimestamp request.expires_at now = .gt
  · simp only [future, ne_eq, not_true_eq_false, decide_false, Bool.false_eq_true,
      ↓reduceIte]
    simp only [veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone,
      bind_ok, bind_tc_ok]
    rw [fresh_jwt_id_exact observation]
    cases parsed : observation.parsed <;>
      simp [bootstrapOutcome, future, idOutcome, parsed, identityValue,
        bootstrapRequest, core.result.Result.Insts.CoreOpsTry.branch,
        core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
        core.convert.FromSame, bind_ok, bind_tc_ok]
  · simp [future, bootstrapOutcome, ne_eq, bind_ok, bind_tc_ok]

/-- Actual issue continuation after this admitted bootstrap identity. Claims are
still produced by the transparent native from_identity body, and that very value
is supplied to the concrete signer. This is a factorization for development;
independent complete claims provenance is not yet claimed by this lemma. -/
def signOutcome (issuer : Issuer) (identity : Identity) :
    Result (core.result.Result internal_auth.IssuedHubInternalToken TokenError) := do
  let claims ← internal_auth.AccessTokenClaims.from_identity identity
  let signed ← internal_auth.HubInternalTokenIssuer.sign_forwarded issuer claims
  match signed with
  | .Err error => .ok (.Err error)
  | .Ok bearer => .ok (.Ok { bearer_token := bearer, identity })

private theorem empty_permission_search (server : veoveo_types.platform_names.ServerSlug) :
    core.iter.traits.iterator.Iterator.find.default
      (alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT
        veoveo_types.permission_names.PermissionName)
      internal_auth.HubInternalTokenIssuer.issue.closure.Insts.CoreOpsFunctionFnMutTupleShared1Shared2PermissionNameBool
      [] server = .ok (none, []) := by
  unfold core.iter.traits.iterator.Iterator.find.default
  rw [Native.find]
  simp only [alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT,
    alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next,
    bind_ok, bind_tc_ok] <;> rfl

theorem bootstrap_issue_exact (issuer : Issuer) (request : Request) (now : Time)
    (observation : IdObservation)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok now) :
    internal_auth.HubInternalTokenIssuer.issue issuer (bootstrapRequest request) =
      (match bootstrapOutcome issuer request now observation with
      | .Err error => .ok (.Err error)
      | .Ok created => signOutcome issuer created) := by
  unfold internal_auth.HubInternalTokenIssuer.issue
  simp only [bootstrapRequest, alloc.collections.btree.set.BTreeSet.iter,
    empty_permission_search, bind_ok, bind_tc_ok]
  have created := bootstrap_identity_exact issuer request now observation provider clock
  simp only [bootstrapRequest] at created
  rw [created]
  cases outcome : bootstrapOutcome issuer request now observation with
  | Err error =>
    simp [outcome, core.result.Result.Insts.CoreOpsTry.branch,
      core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
      core.convert.FromSame, bind_ok, bind_tc_ok]
  | Ok created =>
    simp only [outcome, core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
    unfold signOutcome
    cases claimsStep : internal_auth.AccessTokenClaims.from_identity created with
    | ret claims =>
      cases signing : internal_auth.HubInternalTokenIssuer.sign_forwarded issuer claims with
      | ret signed =>
        cases signed <;>
          simp [claimsStep, signing, core.result.Result.Insts.CoreOpsTry.branch,
            core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
            core.convert.FromSame, bind_ok, bind_tc_ok]
      | div => simp [claimsStep, signing, bind_ok, bind_tc_ok, bind_div, bind_tc_div]
      | vis effect next =>
        cases effect with
        | fail error =>
          have nextEmpty : next = PEmpty.elim := by
            funext impossible
            exact impossible.elim
          rw [nextEmpty, ← Result.fail_eq_vis] at signing
          simp [claimsStep, signing, bind_ok, bind_tc_ok, bind_fail, bind_tc_fail]
    | div => simp [claimsStep, bind_div, bind_tc_div]
    | vis effect next =>
      cases effect with
      | fail error =>
        have nextEmpty : next = PEmpty.elim := by
          funext impossible
          exact impossible.elim
        rw [nextEmpty, ← Result.fail_eq_vis] at claimsStep
        simp [claimsStep, bind_fail, bind_tc_fail]
        apply congrArg (Result.vis (RustEffect.Input.fail error))
        funext impossible
        exact impossible.elim

/-- Exact recipient error priority and complete presented triple preservation.
The presented value is assumed already decoded/verified by the caller; this
contract neither parses a token nor verifies its cryptographic signature. -/
theorem recipient_exact {S D R : Type} [DecidableEq S] [DecidableEq D] [DecidableEq R]
    (serverEq : core.cmp.PartialEq S S) (deploymentEq : core.cmp.PartialEq D D)
    (resourceEq : core.cmp.PartialEq R R)
    (serverLaw : ∀ left right, serverEq.ne left right = .ok (decide (left ≠ right)))
    (deploymentLaw : ∀ left right, deploymentEq.ne left right = .ok (decide (left ≠ right)))
    (resourceLaw : ∀ left right, resourceEq.ne left right = .ok (decide (left ≠ right)))
    (expected presented : internal_auth.recipient.Recipient S D R) :
    internal_auth.recipient.bind serverEq deploymentEq resourceEq expected presented =
      .ok (if expected.server ≠ presented.server ∨ expected.resource ≠ presented.resource
        then .Err .Audience else if expected.deployment ≠ presented.deployment
        then .Err .Deployment else .Ok presented) := by
  unfold internal_auth.recipient.bind
  rw [serverLaw]
  by_cases server : expected.server = presented.server
  · simp only [server, ne_eq, not_true_eq_false, decide_false, bind_ok, bind_tc_ok,
      Bool.false_eq_true, ↓reduceIte]
    rw [resourceLaw]
    by_cases resource : expected.resource = presented.resource
    · simp only [resource, ne_eq, not_true_eq_false, decide_false, bind_ok, bind_tc_ok,
        Bool.false_eq_true, ↓reduceIte]
      rw [deploymentLaw]
      by_cases deployment : expected.deployment = presented.deployment <;>
        simp [server, resource, deployment, bind_ok, bind_tc_ok]
    · simp [server, resource, bind_ok, bind_tc_ok]
  · simp [server, bind_ok, bind_tc_ok]

#print axioms fresh_jwt_id_exact
#print axioms bootstrap_identity_exact
#print axioms bootstrap_issue_exact
#print axioms recipient_exact

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let idPrimitives := standard ++ #[
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse]
  let identityPrimitives := idPrimitives ++ #[
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider]
  for (declaration, allowed) in #[
      (``InternalAuthCorrectness.fresh_jwt_id_exact, idPrimitives),
      (``InternalAuthCorrectness.bootstrap_identity_exact, identityPrimitives),
      (``InternalAuthCorrectness.bootstrap_issue_exact, identityPrimitives.push
        ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.sign_forwarded),
      (``InternalAuthCorrectness.recipient_exact, standard)] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !allowed.contains axiomName)).isEmpty do
      throwError "unreviewed issuer correctness dependency in {declaration}: {found}"

end
end InternalAuthCorrectness
