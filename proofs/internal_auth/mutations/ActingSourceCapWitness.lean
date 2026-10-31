import InternalAuthIdentityExamples
import InternalAuthContextProof
import Lean.Util.CollectAxioms

-- Test-only: compile against acting-source-cap-repeated raw functions. No import
-- of IdentityProof or its expected-failing theorem objects.
open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples InternalAuthIdentityExamples InternalAuthContextProof

namespace ActingSourceCapWitness
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

private abbrev signedContext : Context :=
  delegatedContext (sourceWithExpiry seedAuthenticatedAt) (some seedAuthenticatedAt)

/-- All independent SDK constructor checks are satisfied. This establishes no
native Task liveness, signed-context authenticity or current-policy provenance. -/
theorem independent_admission (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId) :
    delegationError (actingRequest base permission previous acting task) = none ∧
    ContextAccepted signedContext (actingRequest base permission previous acting task).actor
      (actingRequest base permission previous acting task).authority ∧
    signedContext.access_token.oauth_client_id = (actingRequest base permission previous acting task).client ∧
    effectiveDeadline (actingRequest base permission previous acting task) = inheritedDeadline ∧
    Native.compareTimestamp (effectiveDeadline (actingRequest base permission previous acting task))
      seedIssuedAt = .gt ∧
    Native.compareTimestamp signedContext.access_token.expires_at seedIssuedAt = .lt := by
  have constraints := acting_constraints base permission previous acting task
  refine ⟨constraints.1, ?_, rfl, constraints.2, ?_, ?_⟩
  · exact delegated_accepted (sourceWithExpiry seedAuthenticatedAt) seedAuthority
      (some seedAuthenticatedAt) (some seedIssuedAt)
  · rw [constraints.2]
    exact deadline_order.2.2.2.1
  · exact deadline_order.2.2.2.2.1

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem context_domain :
    ContextTextDomain signedContext (delegatedActor (some seedIssuedAt)) := by
  constructor <;> apply short_text <;>
    dsimp only [signedContext, delegatedContext, inputContext, sourceWithExpiry,
      userPrincipal, delegatedActor] <;> decide +kernel

private theorem min_right (left right : NativeTimestamp)
    (rightEarlier : Native.compareTimestamp right left = .lt) :
    core.cmp.Ord.min.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpOrd
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right = Result.ok right := by
  simp [core.cmp.Ord.min.trait_default, core.cmp.Ord.min.default,
    core.cmp.Ord.min_body, core.cmp.PartialOrd.lt_body,
    chrono.datetime.DateTime.Insts.CoreCmpOrd,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp,
    rightEarlier, bind_ok, bind_tc_ok]

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

private theorem expiry_gt (left right : NativeTimestamp) :
    core.cmp.PartialOrd.gt.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      Result.ok (decide (Native.compareTimestamp left right = .gt)) := by
  simp [core.cmp.PartialOrd.gt.trait_default, core.cmp.PartialOrd.gt.default,
    core.cmp.PartialOrd.gt_body,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp, bind_ok]

private theorem one_plus_one : ((1#usize) + (1#usize) : Result Usize) = Result.ok (2#usize) := by
  have safe := UScalar.bound_suffices .Usize 2 (by decide +kernel)
  change 2 < 2 ^ System.Platform.numBits at safe
  with_unfolding_all
    change UScalar.tryMk .Usize 2 = Result.ok (2#usize)
  simp [UScalar.tryMk, UScalar.tryMkOpt, safe]

private theorem inherited_chain_valid (previous : internal_auth.ActingServer)
    (task : identity.OpaqueTaskId) :
    internal_auth.exchange.DelegationContext.validate (inheritedContext previous task) =
      Result.ok (.Ok ()) := by
  simp [internal_auth.exchange.DelegationContext.validate, inheritedContext,
    alloc.vec.Vec.is_empty, internal_auth.exchange.MAX_DELEGATION_DEPTH,
    core.option.Option.as_ref, core.option.Option.is_some_and,
    internal_auth.exchange.DelegationContext.validate.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegatingTaskBool,
    internal_auth.exchange.DelegationContext.validate.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegatingTaskBool.call_once,
    alloc.vec.Vec.index_slice_index, alloc.vec.Vec.index_usize, alloc.vec.Vec.from_val,
    core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
    veoveo_types.platform_names.ServerSlug.Insts.CoreCmpPartialEqServerSlug.eq,
    veoveo_types.execution.DeploymentId.Insts.CoreCmpPartialEqDeploymentId.eq,
    bind_ok, bind_tc_ok]

private theorem delegation_valid (previous acting : internal_auth.ActingServer)
    (task : identity.OpaqueTaskId) :
    internal_auth.exchange.validate_delegation (some acting) none
      (some (inheritedContext previous task)) inheritedDeadline = Result.ok (.Ok ()) := by
  unfold internal_auth.exchange.validate_delegation
  with_unfolding_all
    simp only [core.option.Option.is_none, core.option.Option.is_some,
      Option.isNone, Option.isSome, Bool.false_eq_true, eq_self_iff_true,
      ite_false, ite_true]
  rw [inherited_chain_valid]
  simp [core.result.Result.Insts.CoreOpsTry.branch,
    core.convert.num.FromUsizeBool.from, lift, inheritedContext,
    alloc.vec.Vec.from_val, one_plus_one,
    internal_auth.exchange.MAX_DELEGATION_DEPTH,
    expiry_gt, deadline_order.2.2.2.2.2, bind_ok, bind_tc_ok]
  with_unfolding_all
    change (do
      let total ← ((1#usize) + (1#usize) : Result Usize)
      if total > (8#usize) then
        Result.ok (core.result.Result.Err internal_auth.InternalTokenError.DelegationDepthExceeded)
      else Result.ok (core.result.Result.Ok ())) = Result.ok (core.result.Result.Ok ())
  rw [one_plus_one]
  simp [bind_ok]

/-- The deliberate extra source cap causes actual refusal of these independently
admitted inputs. Only provider/clock observations are needed before the refusal;
there is no assumed constructor error or native Task-liveness premise. -/
theorem mutant_refuses_admitted_acting (issuer : internal_auth.HubInternalTokenIssuer)
    (base : Request) (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer
      (actingRequest base permission previous acting task) =
      Result.ok (.Err internal_auth.InternalTokenError.ExpiredDelegation) := by
  have admitted := delegated_accepted (sourceWithExpiry seedAuthenticatedAt) seedAuthority
    (some seedAuthenticatedAt) (some seedIssuedAt)
  have contextOk : internal_auth.request.HubRequestContext.validate_for signedContext
      (delegatedActor (some seedIssuedAt)) (delegatedAuthority seedAuthority) =
      Result.ok (.Ok ()) := by
    rw [validate_for_exact _ _ _ context_domain]
    simp only [admitted, ite_true]
  have requestedLater : Native.compareTimestamp inheritedDeadline requestedDeadline = .lt := by
    decide +kernel
  have sourceEarlier : Native.compareTimestamp seedAuthenticatedAt inheritedDeadline = .lt := by
    decide +kernel
  -- Expand the independent record input before the generated body, so its
  -- dependent Bool decision dictionaries see the same literal Option fields.
  dsimp only [actingRequest]
  unfold internal_auth.HubInternalTokenIssuer.create_identity
  rw [provider]
  simp only [core.option.Option.as_ref, core.option.Option.map_or,
    internal_auth.HubInternalTokenIssuer.create_identity.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegationContextDateTimeUtcUtc,
    internal_auth.HubInternalTokenIssuer.create_identity.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegationContextDateTimeUtcUtc.call_once,
    bind_ok, bind_tc_ok]
  rw [show (inheritedContext previous task).expires_at = inheritedDeadline from rfl]
  with_unfolding_all rw [min_right _ _ requestedLater]
  simp only [bind_ok, bind_tc_ok]
  rw [delegation_valid]
  simp only [core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
  rw [clock]
  simp only [bind_ok, bind_tc_ok]
  rw [contextOk]
  simp [core.result.Result.Insts.CoreOpsTry.branch,
    core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
    veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq,
    signedContext, delegatedContext, inputContext, sourceWithExpiry,
    core.option.Option.is_some, min_right _ _ sourceEarlier,
    expiry_le, deadline_order.2.2.2.2.1, bind_ok, bind_tc_ok]

/-- Effective expiry is future under the independent specification, while the
actual mutant refuses. Complete context/delegation/client admission is established
in independent_admission above, separately from this actual-result theorem. -/
theorem future_deadline_wrongly_refused (issuer : internal_auth.HubInternalTokenIssuer)
    (base : Request) (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    Native.compareTimestamp (effectiveDeadline (actingRequest base permission previous acting task))
      seedIssuedAt = .gt ∧
    internal_auth.HubInternalTokenIssuer.create_identity issuer
      (actingRequest base permission previous acting task) =
      Result.ok (.Err internal_auth.InternalTokenError.ExpiredDelegation) := by
  exact ⟨(independent_admission base permission previous acting task).2.2.2.2.1,
    mutant_refuses_admitted_acting issuer base permission previous acting task provider clock⟩

#print axioms independent_admission
#print axioms mutant_refuses_admitted_acting
#print axioms future_deadline_wrongly_refused

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  let found ← Lean.collectAxioms ``ActingSourceCapWitness.independent_admission
  unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
    throwError "unreviewed independent acting admission: {found}"
  -- The concrete constructor's statement contains its full primitive dependency
  -- graph, even though this refusal theorem needs no UUID/parse observations.
  let native := standard ++ #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse]
  for declaration in #[``ActingSourceCapWitness.mutant_refuses_admitted_acting,
      ``ActingSourceCapWitness.future_deadline_wrongly_refused] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !native.contains axiomName)).isEmpty do
      throwError "unreviewed acting deadline witness ({declaration}): {found}"

end
end ActingSourceCapWitness
