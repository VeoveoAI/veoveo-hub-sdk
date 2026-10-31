import RequestCompositionSdk.Funs

open Aeneas Aeneas.Std RequestCompositionSdk

namespace InternalAuthSpecification
noncomputable section
local instance (p : Prop) : Decidable p := Classical.propDecidable p

abbrev Context := internal_auth.request.HubRequestContext
abbrev Principal := identity.Principal
abbrev Authority := veoveo_types.authority.InvocationAuthority
abbrev Request := internal_auth.ForwardedRequest
abbrev Identity := internal_auth.HubInternalIdentity

def invocationMode (provenance : veoveo_types.provenance.InvocationProvenance) :
    veoveo_types.provenance.InvocationMode :=
  match provenance with
  | .Direct _ => .Direct
  | .Delegated _ _ => .Delegated
  | .Automated => .Automated

/-- Independent field admission; it does not call validate_for or assert its
desired output. Principal equality includes authenticated_at and every set. -/
def ContextAccepted (context : Context) (actor : Principal) (authority : Authority) : Prop :=
  let source := context.principal
  let token := context.access_token
  token.issuer = source.issuer ∧ token.subject = source.subject ∧
  token.scopes = source.scopes ∧ token.work_context = authority.work_context ∧
  source.tenant = some authority.tenant ∧ actor.tenant = source.tenant ∧
  token.invocation_mode = invocationMode authority.provenance ∧
  (match authority.provenance with
  | .Direct initiator =>
      actor = source ∧ initiator = source.id ∧ token.initiator = some initiator ∧
      token.delegation_id = none
  | .Automated =>
      actor = source ∧ source.kind = .Service ∧
      source.subject.value = token.oauth_client_id.value ∧
      token.initiator = none ∧ token.delegation_id = none ∧ token.session_family = none
  | .Delegated initiator delegationId =>
      source.kind = .User ∧ initiator = source.id ∧ token.initiator = some initiator ∧
      token.delegation_id = some delegationId ∧ actor.kind = .Service ∧
      actor.id.value = token.issuer.value ++ "#" ++ token.oauth_client_id.value ∧
      actor.issuer = token.issuer ∧ actor.subject.value = token.oauth_client_id.value ∧
      actor.scopes = source.scopes ∧ actor.data_labels = source.data_labels ∧
      actor.assurances = source.assurances ∧ actor.groups.values = [] ∧
      actor.group_roles.values = [] ∧ actor.roles.values = [] ∧ token.session_family = none) ∧
  (token.session_family.isSome → source.kind = .User ∧ token.invocation_mode = .Direct)

/-- Full timestamp comparison, before the later NumericDate projection. -/
def earlier (left right : NativeTimestamp) : NativeTimestamp :=
  if Native.compareTimestamp left right = .lt then left else right

def delegationDeadline (request : Request) : NativeTimestamp :=
  match request.delegation_context with
  | none => request.expires_at
  | some context => earlier request.expires_at context.expires_at

/-- An acting server retains the delegation/request cap even when the original
source token has expired. Direct calls with context also retain the source cap. -/
def effectiveDeadline (request : Request) : NativeTimestamp :=
  match request.request_context, request.acting_server with
  | some context, none => earlier (delegationDeadline request) context.access_token.expires_at
  | _, _ => delegationDeadline request

/-- Independent delegation refusal priority, including the first actor's Task
ownership and the native maximum of eight server actors. No nonempty-vector
premise is assumed: emptiness itself is a modeled refusal. -/
def delegationError (request : Request) : Option internal_auth.InternalTokenError :=
  if request.acting_server.isNone ∧
      (request.acting_task.isSome ∨ request.delegation_context.isSome) then
    some .InconsistentClaims
  else match request.delegation_context with
  | none => none
  | some context =>
      if context.actors.val = [] ∨ context.actors.val.length > 8 then
        some .DelegationDepthExceeded
      else if context.task.any (fun task =>
          match context.actors.val.head? with
          | none => false
          | some actor => decide (task.server ≠ actor.server ∨ task.deployment ≠ actor.deployment)) then
        some .InconsistentClaims
      else if context.actors.val.length + (if request.acting_server.isSome then 1 else 0) > 8 then
        some .DelegationDepthExceeded
      else if request.acting_task.isSome ∨
          Native.compareTimestamp (delegationDeadline request) context.expires_at = .gt then
        some .InconsistentClaims
      else none

def expectedScope (actor : Principal) : Option String :=
  match actor.scopes.values with
  | [] => none
  | _ :: _ => some (String.intercalate " " (actor.scopes.values.map (·.value)))

def expectedPrior (context : Option internal_auth.exchange.DelegationContext)
    (acting : Option internal_auth.ActingServer) : Option internal_auth.PriorActor :=
  acting.map fun current =>
    let previous := (context.map (·.actors.val) |>.getD []).foldl
      (fun chain actor => some (.mk actor.client chain)) none
    .mk current.client previous

def expectedInvocation (token : Identity) : internal_auth.Invocation := {
  profile := token.profile, server := token.server, deployment := token.deployment,
  actor := token.actor, authority := token.authority, request_context := token.request_context,
  acting_server := token.acting_server.map (·.server),
  acting_deployment := token.acting_server.map (·.deployment),
  acting_task := token.acting_task, delegation_context := token.delegation_context }

/-- Complete independently described claims. Observable scope text follows the
modeled traversal; native canonical text additionally needs ScopeTraversalSorted
and native set-enumeration correspondence. NumericDates explicitly project only
seconds, so this contract asserts no positive whole-second lifetime. -/
def ClaimsExact (token : Identity) (claims : internal_auth.AccessTokenClaims) : Prop :=
  claims.standard.iss = token.issuer ∧
  claims.standard.sub = token.actor.id.value ∧ claims.standard.aud = token.audience.value ∧
  claims.standard.client_id = token.client ∧ claims.standard.exp = token.expires_at.seconds ∧
  claims.standard.nbf = token.not_before.seconds ∧ claims.standard.iat = token.issued_at.seconds ∧
  claims.standard.jti = token.jwt_id ∧
  claims.standard.act = { sub := token.issuer, act := expectedPrior token.delegation_context token.acting_server } ∧
  claims.standard.scope = expectedScope token.actor ∧
  claims.standard.groups = token.actor.groups ∧ claims.standard.roles = token.actor.roles ∧
  claims.authorization_details.val =
    (if token.permissions.values = [] then [] else
      [{ kind := .Permissions, actions := token.permissions }]) ∧
  claims.invocation = expectedInvocation token

end
end InternalAuthSpecification
