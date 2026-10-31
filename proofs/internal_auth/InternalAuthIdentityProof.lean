import InternalAuthContextProof
import InternalAuthCorrectness
import InternalAuthIdentityExamples
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof
open InternalAuthContextExamples InternalAuthIdentityExamples

namespace InternalAuthIdentityProof
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

abbrev Issuer := internal_auth.HubInternalTokenIssuer
abbrev TokenError := internal_auth.InternalTokenError
abbrev Delegation := internal_auth.exchange.DelegationContext
abbrev Acting := internal_auth.ActingServer

/-- An independent refusal priority for the inherited chain itself. The head is
examined only after nonemptiness and the native depth bound have been checked. -/
def chainError (context : Delegation) : Option TokenError :=
  if context.actors.val = [] ∨ context.actors.val.length > 8 then
    some .DelegationDepthExceeded
  else if context.task.any (fun task =>
      match context.actors.val.head? with
      | none => false
      | some actor => decide (task.server ≠ actor.server ∨ task.deployment ≠ actor.deployment)) then
    some .InconsistentClaims
  else none

/-- This describes all arguments of validate_delegation, not only capped callers.
In particular, an inherited context without an acting server refuses before its
chain fields are examined. -/
def delegationDecision (acting : Option Acting) (task : Option identity.OpaqueTaskId)
    (context : Option Delegation) (expires : NativeTimestamp) : Option TokenError :=
  if acting.isNone ∧ (task.isSome ∨ context.isSome) then some .InconsistentClaims
  else match context with
  | none => none
  | some inherited =>
      match chainError inherited with
      | some error => some error
      | none =>
          if inherited.actors.val.length + (if acting.isSome then 1 else 0) > 8 then
            some .DelegationDepthExceeded
          else if task.isSome ∨ Native.compareTimestamp expires inherited.expires_at = .gt then
            some .InconsistentClaims
          else none

def validationResult (error : Option TokenError) : core.result.Result Unit TokenError :=
  match error with | none => .Ok () | some error => .Err error

/-- These are only physical text-representation bounds. ContextAccepted itself
is decided inside the result specification; it is never an admission premise. -/
def RequestTextDomain (request : Request) : Prop :=
  ∀ context, request.request_context = some context → ContextTextDomain context request.actor

def requestContextAccepted (request : Request) : Prop :=
  match request.request_context with
  | none => True
  | some context => ContextAccepted context request.actor request.authority ∧
      context.access_token.oauth_client_id = request.client

/-- Complete field specification and refusal priority through the actual external
observations. Primitive failure/divergence remains in the Result binds; no own
validator, identity constructor or fresh-ID success is assumed. The clock is
sampled after chain validation and before request-context validation, as in Rust.
This is an equation of values/dependencies, not an instrumented effect trace. -/
def identityOutcome (issuer : Issuer) (request : Request) :
    Result (core.result.Result Identity TokenError) := do
  internal_auth.ensure_jwt_crypto_provider
  match delegationError request with
  | some error => .ok (.Err error)
  | none =>
      let now ← chrono.offset.utc.Utc.now
      if requestContextAccepted request then
        if Native.compareTimestamp (effectiveDeadline request) now = .gt then
          let sample ← uuid.v7.Uuid.now_v7
          let text ← alloc.string.ToString.Blanket.to_string uuid.Uuid.Insts.CoreFmtDisplay sample
          let parsed ← identity.JwtId.parse alloc.string.String.Insts.CoreConvertAsRefStr text
          match parsed with
          | .Err error => .ok (.Err (.Identifier error))
          | .Ok jwtId => .ok (.Ok (InternalAuthCorrectness.identityValue issuer request now
              (effectiveDeadline request) jwtId))
        else .ok (.Err .ExpiredDelegation)
      else .ok (.Err .InvalidRequestContext)

private theorem usize_nine : 9 ≤ Usize.max := by
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem timestamp_gt (left right : NativeTimestamp) :
    core.cmp.PartialOrd.gt.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      .ok (decide (Native.compareTimestamp left right = .gt)) := by
  simp [core.cmp.PartialOrd.gt.trait_default, core.cmp.PartialOrd.gt.default,
    core.cmp.PartialOrd.gt_body,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp, bind_ok]

private theorem timestamp_le (left right : NativeTimestamp) :
    core.cmp.PartialOrd.le.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      .ok (decide (Native.compareTimestamp left right ≠ .gt)) := by
  cases order : Native.compareTimestamp left right <;>
    simp [core.cmp.PartialOrd.le.trait_default, core.cmp.PartialOrd.le.default,
      core.cmp.PartialOrd.le_body,
      chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime,
      chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp,
      order, bind_ok, bind_tc_ok]

private theorem timestamp_min (left right : NativeTimestamp) :
    core.cmp.Ord.min.trait_default
      (chrono.datetime.DateTime.Insts.CoreCmpOrd
        chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc) left right =
      .ok (earlier left right) := by
  simp only [core.cmp.Ord.min.trait_default, core.cmp.Ord.min.default,
    core.cmp.Ord.min_body, core.cmp.PartialOrd.lt.trait_default,
    core.cmp.PartialOrd.lt.default, core.cmp.PartialOrd.lt_body,
    chrono.datetime.DateTime.Insts.CoreCmpOrd,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime,
    chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp, bind_ok,
    Option.some.injEq, decide_eq_true_eq, earlier]
  cases left with
  | mk ls ln =>
    cases right with
    | mk rs rn =>
      simp only [Native.compareTimestamp, Int.compare_eq_ite_lt, Nat.compare_eq_ite_lt,
        bind_ok, bind_tc_ok, decide_eq_true_eq]
      split_ifs <;>
        simp_all [Result.ok.injEq, NativeTimestamp.mk.injEq,
          IScalar.eq_equiv, UScalar.eq_equiv] <;> omega

private theorem head_exact (actors : alloc.vec.Vec Acting) (first : Acting)
    (tail : List Acting) (contents : actors.val = first :: tail) :
    alloc.vec.Vec.index (core.slice.index.SliceIndexUsizeSlice Acting) actors 0#usize =
      .ok first := by
  simp [alloc.vec.Vec.index_slice_index, alloc.vec.Vec.index_usize, contents]

private theorem task_owner_exact (context : Delegation) (first : Acting)
    (tail : List Acting) (contents : context.actors.val = first :: tail)
    (task : internal_auth.exchange.DelegatingTask) :
    internal_auth.exchange.DelegationContext.validate.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegatingTaskBool.call_once
      context task = .ok (decide (task.server ≠ first.server ∨ task.deployment ≠ first.deployment)) := by
  unfold internal_auth.exchange.DelegationContext.validate.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegatingTaskBool.call_once
  rw [head_exact _ _ _ contents]
  by_cases server : task.server = first.server <;>
    simp [server, core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
      veoveo_types.platform_names.ServerSlug.Insts.CoreCmpPartialEqServerSlug.eq,
      veoveo_types.execution.DeploymentId.Insts.CoreCmpPartialEqDeploymentId.eq,
      bind_ok, bind_tc_ok]

theorem chain_validation_exact (context : Delegation) :
    internal_auth.exchange.DelegationContext.validate context =
      .ok (validationResult (chainError context)) := by
  cases contents : context.actors.val with
  | nil =>
      simp [internal_auth.exchange.DelegationContext.validate,
        alloc.vec.Vec.is_empty, contents, chainError, validationResult, bind_ok]
  | cons first tail =>
      have lengthValue : (alloc.vec.Vec.len context.actors).val = (first :: tail).length := by
        change context.actors.val.length = (first :: tail).length
        rw [contents]
      by_cases depth : (first :: tail).length > 8
      · have tailDepth : 8 ≤ tail.length := by simpa using depth
        simp [internal_auth.exchange.DelegationContext.validate,
          alloc.vec.Vec.is_empty, contents, chainError, validationResult,
          internal_auth.exchange.MAX_DELEGATION_DEPTH, lengthValue, tailDepth, bind_ok]
      · have tailDepth : ¬ 8 ≤ tail.length := by simpa using depth
        cases taskValue : context.task with
        | none =>
            simp [internal_auth.exchange.DelegationContext.validate,
              alloc.vec.Vec.is_empty, contents, chainError, validationResult,
              internal_auth.exchange.MAX_DELEGATION_DEPTH, lengthValue, tailDepth,
              core.option.Option.as_ref, core.option.Option.is_some_and,
              taskValue, bind_ok, bind_tc_ok]
        | some task =>
            have mapped := task_owner_exact context first tail contents task
            by_cases owner : task.server ≠ first.server ∨ task.deployment ≠ first.deployment <;>
              simp [internal_auth.exchange.DelegationContext.validate,
                alloc.vec.Vec.is_empty, contents, chainError, validationResult,
                internal_auth.exchange.MAX_DELEGATION_DEPTH, lengthValue, tailDepth,
                core.option.Option.as_ref, core.option.Option.is_some_and,
                internal_auth.exchange.DelegationContext.validate.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegatingTaskBool,
                taskValue, mapped, owner, bind_ok, bind_tc_ok]

/-- Depth has already bounded the input by eight. This is why the actual native
usize addition cannot overflow, rather than an additional success assumption. -/
private theorem bounded_actor_add (context : Delegation)
    (depth : context.actors.val.length ≤ 8) :
    UScalar.add (alloc.vec.Vec.len context.actors) (1#usize) =
      Result.ok (UScalar.ofNat (ty := .Usize) (context.actors.val.length + 1) (by
        have conservative : 9 ≤ UScalar.cMax .Usize := by decide +kernel
        omega)) := by
  have conservative : context.actors.val.length + 1 ≤ UScalar.cMax .Usize := by
    have small : 9 ≤ UScalar.cMax .Usize := by decide +kernel
    omega
  have safe := UScalar.bound_suffices .Usize (context.actors.val.length + 1) conservative
  change context.actors.val.length + 1 < 2 ^ System.Platform.numBits at safe
  change UScalar.tryMk .Usize (context.actors.val.length + 1) = _
  simp [UScalar.tryMk, UScalar.tryMkOpt, UScalar.ofNat, safe]

theorem delegation_validation_exact (acting : Option Acting)
    (task : Option identity.OpaqueTaskId) (context : Option Delegation)
    (expires : NativeTimestamp) :
    internal_auth.exchange.validate_delegation acting task context expires =
      .ok (validationResult (delegationDecision acting task context expires)) := by
  cases acting with
  | none =>
      cases task <;> cases context <;>
        simp [internal_auth.exchange.validate_delegation, delegationDecision,
          validationResult, core.option.Option.is_none, core.option.Option.is_some,
          bind_ok, bind_tc_ok]
  | some actor =>
      cases context with
      | none =>
          simp [internal_auth.exchange.validate_delegation, delegationDecision,
            validationResult, core.option.Option.is_none, core.option.Option.is_some]
      | some inherited =>
          cases task
          all_goals
            unfold internal_auth.exchange.validate_delegation
            with_unfolding_all
              simp only [core.option.Option.is_none, core.option.Option.is_some,
                Option.isNone, Option.isSome, Bool.false_eq_true, eq_self_iff_true,
                ite_false, ite_true]
            rw [chain_validation_exact]
            cases error : chainError inherited with
            | some error =>
                simp [error, delegationDecision, validationResult,
                  core.option.Option.is_none, core.option.Option.is_some,
                  core.result.Result.Insts.CoreOpsTry.branch,
                  core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
                  core.convert.FromSame, bind_ok, bind_tc_ok]
            | none =>
                have depth : inherited.actors.val.length ≤ 8 := by
                  unfold chainError at error
                  split at error
                  · contradiction
                  · omega
                with_unfolding_all
                  simp only [validationResult, core.result.Result.Insts.CoreOpsTry.branch,
                    core.convert.num.FromUsizeBool.from, ite_true, lift,
                    bind_ok, bind_tc_ok]
                have addStep := bounded_actor_add inherited depth
                with_unfolding_all
                  change ((alloc.vec.Vec.len inherited.actors + (1#usize)) : Result Usize) = _ at addStep
                rw [addStep]
                simp [delegationDecision, validationResult, error,
                  core.option.Option.is_none, core.option.Option.is_some,
                  core.result.Result.Insts.CoreOpsTry.branch,
                  internal_auth.exchange.MAX_DELEGATION_DEPTH,
                  timestamp_gt, bind_ok, bind_tc_ok]
                split_ifs <;> rfl

theorem request_delegation_decision (request : Request) :
    delegationDecision request.acting_server request.acting_task request.delegation_context
      (delegationDeadline request) = delegationError request := by
  cases contextValue : request.delegation_context with
  | none => simp [delegationDecision, delegationError, contextValue]
  | some context =>
      unfold delegationDecision delegationError chainError
      simp only [contextValue]
      split_ifs <;> simp_all
      all_goals grind

private theorem cap_exact (request : Request) :
    core.option.Option.map_or
      internal_auth.HubInternalTokenIssuer.create_identity.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegationContextDateTimeUtcUtc
      request.delegation_context request.expires_at request.expires_at =
      .ok (delegationDeadline request) := by
  cases context : request.delegation_context <;>
    simp [core.option.Option.map_or, delegationDeadline, context,
      internal_auth.HubInternalTokenIssuer.create_identity.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegationContextDateTimeUtcUtc,
      internal_auth.HubInternalTokenIssuer.create_identity.closure.Insts.CoreOpsFunctionFnOnceTupleShared1DelegationContextDateTimeUtcUtc.call_once,
      timestamp_min]

/-- Exact identity construction for arbitrary complete requests and permissions,
including every optional-context and acting-server combination. The independent
result retains native primitive errors/divergence and own-code refusal priority;
only the five physical text bounds are premises. -/
theorem create_identity_exact (issuer : Issuer) (request : Request)
    (textDomain : RequestTextDomain request) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer request =
      identityOutcome issuer request := by
  unfold internal_auth.HubInternalTokenIssuer.create_identity identityOutcome
  apply congrArg (fun continuation : Unit → Result (core.result.Result Identity TokenError) =>
    Aeneas.Std.bind internal_auth.ensure_jwt_crypto_provider continuation)
  funext providerUnit
  simp only [core.option.Option.as_ref, bind_ok, bind_tc_ok]
  rw [cap_exact]
  simp only [bind_ok, bind_tc_ok]
  rw [delegation_validation_exact, request_delegation_decision]
  cases error : delegationError request with
  | some error =>
      simp [validationResult, core.result.Result.Insts.CoreOpsTry.branch,
        core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
        core.convert.FromSame, bind_ok, bind_tc_ok]
  | none =>
      simp only [validationResult, core.result.Result.Insts.CoreOpsTry.branch,
        bind_ok, bind_tc_ok]
      apply congrArg (fun continuation : NativeTimestamp → Result (core.result.Result Identity TokenError) =>
        Aeneas.Std.bind chrono.offset.utc.Utc.now continuation)
      funext now
      cases contextValue : request.request_context with
      | none =>
          simp only [requestContextAccepted, contextValue, effectiveDeadline, ite_true]
          by_cases future : Native.compareTimestamp (delegationDeadline request) now = .gt
          · simp only [timestamp_le, future, ne_eq, not_true_eq_false, decide_false,
              Bool.false_eq_true, ite_false, ite_true,
              veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone, bind_ok, bind_tc_ok]
            simp only [internal_auth.fresh_jwt_id, bind_tc_eq, Aeneas.Std.bind_assoc]
            apply congrArg (fun continuation => Aeneas.Std.bind uuid.v7.Uuid.now_v7 continuation)
            funext sample
            apply congrArg (fun continuation => Aeneas.Std.bind
              (alloc.string.ToString.Blanket.to_string uuid.Uuid.Insts.CoreFmtDisplay sample) continuation)
            funext text
            apply congrArg (fun continuation => Aeneas.Std.bind
              (identity.JwtId.parse alloc.string.String.Insts.CoreConvertAsRefStr text) continuation)
            funext parsed
            cases parsed <;>
              simp [core.result.Result.Insts.CoreOpsTry.branch,
                core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
                core.convert.FromSame, InternalAuthCorrectness.identityValue,
                contextValue, bind_ok, bind_tc_ok]
          · simp [timestamp_le, future, ne_eq, bind_ok, bind_tc_ok]
      | some context =>
          dsimp only
          rw [validate_for_exact context request.actor request.authority
            (textDomain context contextValue)]
          by_cases accepted : ContextAccepted context request.actor request.authority
          · by_cases client : context.access_token.oauth_client_id = request.client
            · cases acting : request.acting_server
              all_goals
                by_cases future : Native.compareTimestamp (effectiveDeadline request) now = .gt
                · simp only [effectiveDeadline, contextValue, acting] at future
                  simp [accepted, client, requestContextAccepted, contextValue,
                    effectiveDeadline, acting, timestamp_min, timestamp_le, future,
                    core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
                    veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq,
                    core.option.Option.is_some,
                    veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone,
                    core.result.Result.Insts.CoreOpsTry.branch,
                    core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
                    core.convert.FromSame, bind_tc_eq, bind_ok, bind_tc_ok]
                  simp only [internal_auth.fresh_jwt_id, bind_tc_eq, Aeneas.Std.bind_assoc]
                  apply congrArg (fun continuation => Aeneas.Std.bind uuid.v7.Uuid.now_v7 continuation)
                  funext sample
                  apply congrArg (fun continuation => Aeneas.Std.bind
                    (alloc.string.ToString.Blanket.to_string uuid.Uuid.Insts.CoreFmtDisplay sample) continuation)
                  funext text
                  apply congrArg (fun continuation => Aeneas.Std.bind
                    (identity.JwtId.parse alloc.string.String.Insts.CoreConvertAsRefStr text) continuation)
                  funext parsed
                  cases parsed <;>
                    simp [core.result.Result.Insts.CoreOpsTry.branch,
                      core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
                      core.convert.FromSame, InternalAuthCorrectness.identityValue,
                      contextValue, effectiveDeadline, acting, bind_ok, bind_tc_ok]
                · simp only [effectiveDeadline, contextValue, acting] at future
                  simp [accepted, client, requestContextAccepted, contextValue,
                    effectiveDeadline, acting, timestamp_min, timestamp_le, future,
                    core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
                    veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq,
                    core.option.Option.is_some,
                    core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
            · simp [accepted, client, requestContextAccepted, contextValue,
                core.cmp.PartialEq.ne.trait_default, core.cmp.PartialEq.ne.default,
                veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq,
                core.result.Result.Insts.CoreOpsTry.branch, bind_ok, bind_tc_ok]
          · simp [accepted, requestContextAccepted, contextValue,
              core.result.Result.Insts.CoreOpsTry.branch,
              core.result.Result.Insts.CoreOpsTry_traitFromResidualResult.from_residual,
              core.convert.FromSame, bind_ok, bind_tc_ok]

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem direct_domain (base : Request)
    (permission : veoveo_types.permission_names.PermissionName) :
    RequestTextDomain (directRequest base permission) := by
  intro context contextValue
  change some (directContext (sourceWithExpiry seedExpiresAt)
    (some seedAuthenticatedAt)) = some context at contextValue
  cases Option.some.inj contextValue
  constructor <;> apply short_text <;>
    dsimp only [directRequest, directContext, inputContext, sourceWithExpiry, userPrincipal] <;>
    decide +kernel

private theorem acting_domain (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId) :
    RequestTextDomain (actingRequest base permission previous acting task) := by
  intro context contextValue
  change some (delegatedContext (sourceWithExpiry seedAuthenticatedAt)
    (some seedAuthenticatedAt)) = some context at contextValue
  cases Option.some.inj contextValue
  constructor <;> apply short_text <;>
    dsimp only [actingRequest, delegatedContext, inputContext, sourceWithExpiry,
      userPrincipal, delegatedActor] <;> decide +kernel

/-- Actual context-bearing Direct construction, with a nonempty supplied grant
and earlier source-token cap. Only native provider/time/ID observations are
premises; admission and deadline conditions are independently derived. -/
theorem direct_identity_constructed (issuer : Issuer) (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer (directRequest base permission) =
      .ok (.Ok (InternalAuthCorrectness.identityValue issuer (directRequest base permission)
        seedIssuedAt seedExpiresAt jwtId)) := by
  rw [create_identity_exact issuer _ (direct_domain base permission)]
  have admitted := direct_accepted (sourceWithExpiry seedExpiresAt) seedAuthority
    (some seedAuthenticatedAt)
  have contextAdmission : requestContextAccepted (directRequest base permission) := by
    exact ⟨admitted, rfl⟩
  have conditions := direct_constraints base permission
  unfold identityOutcome
  rw [provider]
  simp only [bind_ok, bind_tc_ok, conditions.1]
  rw [clock]
  simp only [bind_ok, bind_tc_ok, contextAdmission, ite_true, conditions.2,
    deadline_order.2.2.1, ite_true]
  rw [observation.uuid_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.text_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.parse_observed]
  simp only [bind_ok, bind_tc_ok, parsed]

/-- A populated inherited chain and matching originating Task are retained in
full. The original source token is already expired at the observed clock; acting
server presence correctly keeps the future inherited cap instead of re-capping
at that source expiry. This asserts no Task liveness or cryptographic validity. -/
theorem acting_identity_constructed (issuer : Issuer) (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId)
    (observation : InternalAuthCorrectness.IdObservation) (jwtId : identity.JwtId)
    (parsed : observation.parsed = .Ok jwtId)
    (provider : internal_auth.ensure_jwt_crypto_provider = .ok ())
    (clock : chrono.offset.utc.Utc.now = .ok seedIssuedAt) :
    internal_auth.HubInternalTokenIssuer.create_identity issuer
      (actingRequest base permission previous acting task) =
      .ok (.Ok (InternalAuthCorrectness.identityValue issuer
        (actingRequest base permission previous acting task)
        seedIssuedAt inheritedDeadline jwtId)) := by
  rw [create_identity_exact issuer _ (acting_domain base permission previous acting task)]
  have admitted := delegated_accepted (sourceWithExpiry seedAuthenticatedAt) seedAuthority
    (some seedAuthenticatedAt) (some seedIssuedAt)
  have contextAdmission : requestContextAccepted (actingRequest base permission previous acting task) := by
    exact ⟨admitted, rfl⟩
  have conditions := acting_constraints base permission previous acting task
  unfold identityOutcome
  rw [provider]
  simp only [bind_ok, bind_tc_ok, conditions.1]
  rw [clock]
  simp only [bind_ok, bind_tc_ok, contextAdmission, ite_true, conditions.2,
    deadline_order.2.2.2.1, ite_true]
  rw [observation.uuid_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.text_observed]
  simp only [bind_ok, bind_tc_ok]
  rw [observation.parse_observed]
  simp only [bind_ok, bind_tc_ok, parsed]

#print axioms chain_validation_exact
#print axioms delegation_validation_exact
#print axioms request_delegation_decision
#print axioms create_identity_exact
#print axioms direct_identity_constructed
#print axioms acting_identity_constructed

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthIdentityProof.chain_validation_exact,
      ``InternalAuthIdentityProof.delegation_validation_exact,
      ``InternalAuthIdentityProof.request_delegation_decision] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed SDK delegation dependency ({declaration}): {found}"
  let native := standard ++ #[``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider,
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse]
  for declaration in #[``InternalAuthIdentityProof.create_identity_exact,
      ``InternalAuthIdentityProof.direct_identity_constructed,
      ``InternalAuthIdentityProof.acting_identity_constructed] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !native.contains axiomName)).isEmpty do
      throwError "unreviewed SDK complete identity dependency ({declaration}): {found}"

end
end InternalAuthIdentityProof
