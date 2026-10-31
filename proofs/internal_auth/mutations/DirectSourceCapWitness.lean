import InternalAuthIdentityExamples
import InternalAuthContextProof
import InternalAuthCorrectness
import Lean.Util.CollectAxioms

-- Test-only: compile against the direct-source-cap-bypass raw graph. No import
-- of the expected-failing IdentityProof module or any of its theorem objects.
open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples InternalAuthIdentityExamples InternalAuthContextProof

namespace DirectSourceCapWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem context_domain :
    ContextTextDomain (directContext (sourceWithExpiry seedExpiresAt)
      (some seedAuthenticatedAt)) (userPrincipal (some seedAuthenticatedAt)) := by
  constructor <;> apply short_text <;>
    dsimp only [directContext, inputContext, sourceWithExpiry, userPrincipal] <;> decide +kernel

private theorem expiry_le (left right : NativeTimestamp) :
    core.cmp.PartialOrd.le.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      Result.ok (decide (Native.compareTimestamp left right ≠ .gt)) := by
  cases order : Native.compareTimestamp left right <;>
    simp [core.cmp.PartialOrd.le.trait_default, core.cmp.PartialOrd.le.default,
      core.cmp.PartialOrd.le_body,
      chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp,
      order, bind_ok, bind_tc_ok]

/-- The complete independent baseline deadline remains the earlier source cap. -/
theorem independent_cap (base : Request)
    (permission : veoveo_types.permission_names.PermissionName) :
    effectiveDeadline (directRequest base permission) = seedExpiresAt ∧
    Native.compareTimestamp requestedDeadline (effectiveDeadline (directRequest base permission)) = .gt := by
  rw [(direct_constraints base permission).2]
  exact ⟨rfl, deadline_order.1⟩

/-- Actual wrong complete result, using only typed primitive observations. No own
validation, ID helper or identity-construction success is assumed. -/
theorem mutant_returns_requested_deadline (issuer : internal_auth.HubInternalTokenIssuer)
    (base : Request) (permission : veoveo_types.permission_names.PermissionName)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer (directRequest base permission) =
      Result.ok (.Ok (InternalAuthCorrectness.identityValue issuer (directRequest base permission)
        seedIssuedAt requestedDeadline jwtId)) := by
  have admitted := direct_accepted (sourceWithExpiry seedExpiresAt) seedAuthority
    (some seedAuthenticatedAt)
  have contextOk : internal_auth.request.HubRequestContext.validate_for
      (directContext (sourceWithExpiry seedExpiresAt) (some seedAuthenticatedAt))
      (userPrincipal (some seedAuthenticatedAt)) (directAuthority seedAuthority) =
      Result.ok (.Ok ()) := by
    rw [validate_for_exact _ _ _ context_domain]
    simp only [admitted, ite_true]
  have future : Native.compareTimestamp requestedDeadline seedIssuedAt = .gt := by decide +kernel
  unfold internal_auth.HubInternalTokenIssuer.create_identity
  rw [provider]
  simp only [directRequest, core.option.Option.as_ref, core.option.Option.map_or,
    internal_auth.exchange.validate_delegation,
    core.option.Option.is_none, core.option.Option.is_some,
    core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
  rw [clock]
  simp only [bind_ok, bind_tc_ok]
  rw [contextOk]
  simp [core.result.Result.Insts.CoreOpsTry.branch,
    core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
    veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq,
    directContext, inputContext, core.option.Option.is_some,
    expiry_le, future, veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone,
    bind_ok, bind_tc_ok]
  rw [InternalAuthCorrectness.fresh_jwt_id_exact observation]
  simp [InternalAuthCorrectness.idOutcome, parsed,
    core.result.Result.Insts.CoreOpsTry.branch,
    core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
    core.convert.FromSame, InternalAuthCorrectness.identityValue,
    directRequest, bind_ok, bind_tc_ok]

/-- The actual complete identity differs from the independently capped result. -/
theorem violates_capped_identity (issuer : internal_auth.HubInternalTokenIssuer)
    (base : Request) (permission : veoveo_types.permission_names.PermissionName)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer (directRequest base permission) ≠
      Result.ok (.Ok (InternalAuthCorrectness.identityValue issuer (directRequest base permission)
        seedIssuedAt (effectiveDeadline (directRequest base permission)) jwtId)) := by
  rw [mutant_returns_requested_deadline issuer base permission observation jwtId parsed provider clock,
    (direct_constraints base permission).2]
  intro equality
  simp only [Result.ok.injEq] at equality
  injection equality with identityEq
  have expiryEq := congrArg (fun value : Identity => value.expires_at) identityEq
  change requestedDeadline = seedExpiresAt at expiryEq
  have different : requestedDeadline ≠ seedExpiresAt := by decide +kernel
  exact different expiryEq

#print axioms independent_cap
#print axioms mutant_returns_requested_deadline
#print axioms violates_capped_identity

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let found ← Lean.collectAxioms ``DirectSourceCapWitness.independent_cap
  unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
    throwError "unreviewed independent direct deadline: {found}"
  let native := standard ++ #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse]
  for declaration in #[``DirectSourceCapWitness.mutant_returns_requested_deadline,
      ``DirectSourceCapWitness.violates_capped_identity] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !native.contains axiomName)).isEmpty do
      throwError "unreviewed direct deadline witness ({declaration}): {found}"

end
end DirectSourceCapWitness
