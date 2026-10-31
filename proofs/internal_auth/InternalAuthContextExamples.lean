import InternalAuthSpecification
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification

namespace InternalAuthContextExamples

/-- Populated inputs following the native hub-request-context.json wire shapes.
The extra role/label metadata exercises complete Principal equality. Every modeled
set is unique and, where its order is observable here, in native lexical order. -/
def userPrincipal (authenticatedAt : Option NativeTimestamp) : Principal := {
  id := ⟨"https://identity.example#user-one"⟩
  kind := .User
  issuer := ⟨"https://veoveo.example"⟩
  subject := ⟨"user-one"⟩
  tenant := some ⟨"tenant-a"⟩
  groups := ⟨[⟨"engineering"⟩]⟩
  group_roles := ⟨[{ group := ⟨"engineering"⟩, role := .Write }]⟩
  roles := ⟨[⟨"operator"⟩]⟩
  scopes := ⟨[⟨"operator:read"⟩, ⟨"operator:use"⟩]⟩
  data_labels := ⟨[⟨"internal"⟩]⟩
  assurances := ⟨[.UsPerson]⟩
  authenticated_at := authenticatedAt }

def automatedPrincipal (authenticatedAt : Option NativeTimestamp) : Principal := {
  id := ⟨"https://veoveo.example#worker"⟩
  kind := .Service
  issuer := ⟨"https://veoveo.example"⟩
  subject := ⟨"worker"⟩
  tenant := some ⟨"tenant-a"⟩
  groups := ⟨[]⟩
  group_roles := ⟨[]⟩
  roles := ⟨[]⟩
  scopes := ⟨[⟨"operator:read"⟩, ⟨"operator:use"⟩]⟩
  data_labels := ⟨[]⟩
  assurances := ⟨[]⟩
  authenticated_at := authenticatedAt }

/-- The delegated actor is distinct from the source user, inherits its scopes,
labels and assurances, and has no user groups or roles. Its authentication time
is an independent input: the validator imposes no source-time equality. -/
def delegatedActor (authenticatedAt : Option NativeTimestamp) : Principal := {
  id := ⟨"https://veoveo.example#agent-client"⟩
  kind := .Service
  issuer := ⟨"https://veoveo.example"⟩
  subject := ⟨"agent-client"⟩
  tenant := some ⟨"tenant-a"⟩
  groups := ⟨[]⟩
  group_roles := ⟨[]⟩
  roles := ⟨[]⟩
  scopes := ⟨[⟨"operator:read"⟩, ⟨"operator:use"⟩]⟩
  data_labels := ⟨[⟨"internal"⟩]⟩
  assurances := ⟨[.UsPerson]⟩
  authenticated_at := authenticatedAt }

/-- A fixture input builder, preserving the supplied audit/execution and token
metadata not examined by context admission. Preservation is not authentication. -/
def inputContext (base : Context) (source : Principal)
    (client : veoveo_types.platform_names.OAuthClientId)
    (mode : veoveo_types.provenance.InvocationMode)
    (family : Option identity.HubRefreshFamilyId)
    (initiator : Option veoveo_types.identity.PrincipalId)
    (delegation : Option veoveo_types.identity.DelegationId) : Context := {
  base with
  principal := source
  access_token := {
    base.access_token with
    issuer := source.issuer
    subject := source.subject
    oauth_client_id := client
    session_family := family
    work_context := ⟨"development"⟩
    invocation_mode := mode
    initiator := initiator
    delegation_id := delegation
    scopes := source.scopes } }

def inputAuthority (base : Authority)
    (provenance : veoveo_types.provenance.InvocationProvenance) : Authority := {
  base with
  work_context := ⟨"development"⟩
  tenant := ⟨"tenant-a"⟩
  provenance := provenance }

def directContext (base : Context) (authenticatedAt : Option NativeTimestamp) : Context :=
  inputContext base (userPrincipal authenticatedAt) ⟨"console"⟩ .Direct
    (some "01a08928-856e-7492-9b19-18356fdbeef4")
    (some (userPrincipal authenticatedAt).id) none

def directAuthority (base : Authority) : Authority :=
  inputAuthority base (.Direct ⟨"https://identity.example#user-one"⟩)

def automatedContext (base : Context) (authenticatedAt : Option NativeTimestamp) : Context :=
  inputContext base (automatedPrincipal authenticatedAt) ⟨"worker"⟩ .Automated none none none

def automatedAuthority (base : Authority) : Authority :=
  inputAuthority base .Automated

def delegatedContext (base : Context) (authenticatedAt : Option NativeTimestamp) : Context :=
  inputContext base (userPrincipal authenticatedAt) ⟨"agent-client"⟩ .Delegated none
    (some (userPrincipal authenticatedAt).id) (some ⟨"delegation-fixture"⟩)

def delegatedAuthority (base : Authority) : Authority :=
  inputAuthority base (.Delegated ⟨"https://identity.example#user-one"⟩
    ⟨"delegation-fixture"⟩)

/-- Closed, valid-shaped fixture seed, not a claim that these values were
cryptographically authenticated or that context admission checks their dates. -/
def seedAuthenticatedAt : NativeTimestamp := {
  seconds := IScalar.ofInt 1788220700
  nanoseconds := UScalar.ofNat 0 }

def seedIssuedAt : NativeTimestamp := {
  seconds := IScalar.ofInt 1788220800
  nanoseconds := UScalar.ofNat 0 }

def seedExpiresAt : NativeTimestamp := {
  seconds := IScalar.ofInt 4102444800
  nanoseconds := UScalar.ofNat 0 }

def seedContext : Context := {
  execution := {
    composition := ⟨"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"⟩ }
  audit := {
    id := ⟨"0199a000-0000-7000-8000-000000000001"⟩
    trace_id := ⟨"00000000000000000000000000000001"⟩
    span_id := ⟨"0000000000000001"⟩
    source_ip := none
    task := none }
  access_token := {
    issuer := ⟨"https://veoveo.example"⟩
    subject := ⟨"user-one"⟩
    oauth_client_id := ⟨"console"⟩
    session_family := some "01a08928-856e-7492-9b19-18356fdbeef4"
    audience := "https://veoveo.example/mcp/operator"
    work_context := ⟨"development"⟩
    invocation_mode := .Direct
    initiator := some ⟨"https://identity.example#user-one"⟩
    delegation_id := none
    scopes := (userPrincipal (some seedAuthenticatedAt)).scopes
    jwt_id := some "fixture-token"
    issued_at := seedIssuedAt
    not_before := none
    expires_at := seedExpiresAt }
  principal := userPrincipal (some seedAuthenticatedAt) }

def seedAuthority : Authority := {
  work_context := ⟨"development"⟩
  tenant := ⟨"tenant-a"⟩
  membership := .Contributor
  policy_revision := ⟨"r1"⟩
  output_policy := {
    owner := .Principal ⟨"https://identity.example#user-one"⟩
    initial_grants := alloc.vec.Vec.from [] (by simp)
    classification := none
    data_labels := ⟨[]⟩ }
  provenance := .Direct ⟨"https://identity.example#user-one"⟩ }

theorem direct_accepted (base : Context) (authority : Authority)
    (authenticatedAt : Option NativeTimestamp) :
    ContextAccepted (directContext base authenticatedAt) (userPrincipal authenticatedAt) (directAuthority authority) := by
  simp [ContextAccepted, directContext, directAuthority, inputContext, inputAuthority,
    userPrincipal]

theorem automated_accepted (base : Context) (authority : Authority)
    (authenticatedAt : Option NativeTimestamp) :
    ContextAccepted (automatedContext base authenticatedAt) (automatedPrincipal authenticatedAt)
      (automatedAuthority authority) := by
  simp [ContextAccepted, automatedContext, automatedAuthority, inputContext, inputAuthority,
    automatedPrincipal]

theorem delegated_accepted (base : Context) (authority : Authority)
    (sourceAt actorAt : Option NativeTimestamp) :
    ContextAccepted (delegatedContext base sourceAt) (delegatedActor actorAt)
      (delegatedAuthority authority) := by
  simp [ContextAccepted, delegatedContext, delegatedAuthority, inputContext, inputAuthority,
    userPrincipal, delegatedActor]

/-- All modes retain these arbitrary supplied fields; admission does not check
or authenticate them. Authority policy and membership fields are also retained. -/
theorem preserved_inputs (base : Context) (source : Principal)
    (client : veoveo_types.platform_names.OAuthClientId)
    (mode : veoveo_types.provenance.InvocationMode)
    (family : Option identity.HubRefreshFamilyId)
    (initiator : Option veoveo_types.identity.PrincipalId)
    (delegation : Option veoveo_types.identity.DelegationId)
    (authority : Authority) (provenance : veoveo_types.provenance.InvocationProvenance) :
    let context := inputContext base source client mode family initiator delegation
    context.audit = base.audit ∧ context.execution = base.execution ∧
    context.access_token.audience = base.access_token.audience ∧
    context.access_token.jwt_id = base.access_token.jwt_id ∧
    context.access_token.issued_at = base.access_token.issued_at ∧
    context.access_token.not_before = base.access_token.not_before ∧
    context.access_token.expires_at = base.access_token.expires_at ∧
    (inputAuthority authority provenance).membership = authority.membership ∧
    (inputAuthority authority provenance).policy_revision = authority.policy_revision ∧
    (inputAuthority authority provenance).output_policy = authority.output_policy := by
  exact ⟨rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl⟩

#print axioms direct_accepted
#print axioms automated_accepted
#print axioms delegated_accepted
#print axioms preserved_inputs

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthContextExamples.direct_accepted,
      ``InternalAuthContextExamples.automated_accepted,
      ``InternalAuthContextExamples.delegated_accepted,
      ``InternalAuthContextExamples.preserved_inputs] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed context example dependency ({declaration}): {found}"

end InternalAuthContextExamples
