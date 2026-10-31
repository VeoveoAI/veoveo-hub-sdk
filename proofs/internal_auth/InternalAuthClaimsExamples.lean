import InternalAuthClaimsComponents
import InternalAuthIdentityExamples
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk
open InternalAuthSpecification InternalAuthContextProof InternalAuthContextExamples
open InternalAuthIdentityExamples InternalAuthClaimsComponents

namespace InternalAuthClaimsExamples
noncomputable section

-- Independent typed inputs; this module imports no from_identity correctness
-- theorem, so a source-mutant witness can reuse these fixtures without stale proofs.
def firstActor : internal_auth.ActingServer := {
  server := ⟨"alpha"⟩, deployment := ⟨"0199a000-0000-7000-8000-000000000002"⟩,
  client := ⟨"alpha-client"⟩ }
def secondActor : internal_auth.ActingServer := {
  server := ⟨"beta"⟩, deployment := ⟨"0199a000-0000-7000-8000-000000000003"⟩,
  client := ⟨"beta-client"⟩ }
def currentActor : internal_auth.ActingServer := {
  server := ⟨"gamma"⟩, deployment := ⟨"0199a000-0000-7000-8000-000000000004"⟩,
  client := ⟨"gamma-client"⟩ }

def richIdentity : Identity := {
  issuer := ⟨"https://veoveo.example"⟩
  profile := "operator"
  server := ⟨"operator"⟩
  deployment := ⟨"0199a000-0000-7000-8000-000000000005"⟩
  audience := ⟨"https://veoveo.example/mcp/operator"⟩
  client := ⟨"agent-client"⟩
  actor := delegatedActor (some seedIssuedAt)
  authority := delegatedAuthority seedAuthority
  permissions := ⟨[⟨"operator.inspect"⟩, ⟨"operator.operate"⟩]⟩
  request_context := some (delegatedContext seedContext (some seedAuthenticatedAt))
  acting_server := some currentActor
  acting_task := none
  delegation_context := some {
    actors := alloc.vec.Vec.from [firstActor, secondActor] (by scalar_tac)
    task := some {
      server := firstActor.server
      deployment := firstActor.deployment
      task := "0199a000-0000-7000-8000-000000000006" }
    expires_at := inheritedDeadline }
  jwt_id := "0199a000-0000-7000-8000-000000000007"
  issued_at := seedIssuedAt
  not_before := seedIssuedAt
  expires_at := inheritedDeadline }

def emptyDirectIdentity : Identity := { richIdentity with
  client := ⟨"console"⟩
  actor := userPrincipal (some seedAuthenticatedAt)
  authority := directAuthority seedAuthority
  permissions := ⟨[]⟩
  request_context := some (directContext seedContext (some seedAuthenticatedAt))
  acting_server := none
  delegation_context := none
  expires_at := seedExpiresAt }

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]; omega
  · rw [width, U64.max_eq]; omega

private theorem scope_bounds : ∀ scope ∈
    ([⟨"operator:read"⟩, ⟨"operator:use"⟩] : List veoveo_types.names.ScopeName),
    TextRepresentable scope.value := by
  intro scope member
  simp only [List.mem_cons, List.not_mem_nil, or_false] at member
  rcases member with rfl | rfl <;> apply short_text <;> decide +kernel

theorem rich_domain : ClaimsTextDomain richIdentity := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · apply short_text; decide +kernel
  · apply short_text; decide +kernel
  · exact scope_bounds
  · apply short_text; decide +kernel

theorem empty_direct_domain : ClaimsTextDomain emptyDirectIdentity := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · apply short_text; decide +kernel
  · apply short_text; decide +kernel
  · exact scope_bounds
  · apply short_text; decide +kernel

/-- The fixtures separately satisfy the independent context relation. This is not
source authenticity, Task liveness, a complete issuer result or signing evidence. -/
theorem populated_inputs :
    Native.ScopeTraversalSorted richIdentity.actor.scopes ∧
    richIdentity.permissions.values ≠ [] ∧
    expectedPrior richIdentity.delegation_context richIdentity.acting_server =
      some (.mk currentActor.client (some (.mk secondActor.client
        (some (.mk firstActor.client none))))) ∧
    ContextAccepted (delegatedContext seedContext (some seedAuthenticatedAt))
      richIdentity.actor richIdentity.authority ∧
    ContextAccepted (directContext seedContext (some seedAuthenticatedAt))
      emptyDirectIdentity.actor emptyDirectIdentity.authority := by
  refine ⟨?_, by simp [richIdentity], rfl, ?_, ?_⟩
  · unfold Native.ScopeTraversalSorted
    change List.Pairwise (fun (left right : veoveo_types.names.ScopeName) => left.value < right.value)
      [⟨"operator:read"⟩, ⟨"operator:use"⟩]
    constructor
    · intro other member
      have same : other = (⟨"operator:use"⟩ : veoveo_types.names.ScopeName) := List.mem_singleton.mp member
      subst other
      change "operator:read" < "operator:use"
      decide +kernel
    · exact List.pairwise_singleton _ _
  · exact delegated_accepted seedContext seedAuthority (some seedAuthenticatedAt) (some seedIssuedAt)
  · exact direct_accepted seedContext seedAuthority (some seedAuthenticatedAt)

#print axioms rich_domain
#print axioms empty_direct_domain
#print axioms populated_inputs
run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthClaimsExamples.rich_domain,
      ``InternalAuthClaimsExamples.empty_direct_domain, ``InternalAuthClaimsExamples.populated_inputs] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed claims example boundary in {declaration}: {found}"

end
end InternalAuthClaimsExamples
