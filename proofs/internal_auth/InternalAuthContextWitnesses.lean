import InternalAuthContextExamples
import InternalAuthContextProof
import Lean.Util.CollectAxioms

open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples InternalAuthContextProof

namespace InternalAuthContextWitnesses

private theorem short_text (text : String) (small : text.toByteArray.size ≤ 64) :
    TextRepresentable text := by
  unfold TextRepresentable
  rcases Usize.bounds_eq with width | width
  · rw [width, U32.max_eq]
    omega
  · rw [width, U64.max_eq]
    omega

private theorem direct_text_domain (base : Context) (authenticatedAt : Option NativeTimestamp) :
    ContextTextDomain (directContext base authenticatedAt) (userPrincipal authenticatedAt) := by
  constructor <;> apply short_text <;>
    dsimp only [directContext, inputContext, userPrincipal] <;> decide +kernel

private theorem automated_text_domain (base : Context) (authenticatedAt : Option NativeTimestamp) :
    ContextTextDomain (automatedContext base authenticatedAt) (automatedPrincipal authenticatedAt) := by
  constructor <;> apply short_text <;>
    dsimp only [automatedContext, inputContext, automatedPrincipal] <;> decide +kernel

private theorem delegated_text_domain (base : Context)
    (sourceAt actorAt : Option NativeTimestamp) :
    ContextTextDomain (delegatedContext base sourceAt) (delegatedActor actorAt) := by
  constructor <;> apply short_text <;>
    dsimp only [delegatedContext, inputContext, userPrincipal, delegatedActor] <;> decide +kernel

/-- Actual transparent validator successes, constructed from populated input
fields rather than a premise asserting admission. The Direct example carries a
browser session family; Automated and Delegated carry none. No acting_server or
request issuance condition is imposed by these context-only witnesses. -/
theorem direct_admitted (base : Context) (authority : Authority)
    (authenticatedAt : Option NativeTimestamp) :
    internal_auth.request.HubRequestContext.validate_for
      (directContext base authenticatedAt) (userPrincipal authenticatedAt) (directAuthority authority) =
      Result.ok (.Ok ()) := by
  rw [validate_for_exact _ _ _ (direct_text_domain base authenticatedAt)]
  rw [if_pos (direct_accepted base authority authenticatedAt)]

theorem automated_admitted (base : Context) (authority : Authority)
    (authenticatedAt : Option NativeTimestamp) :
    internal_auth.request.HubRequestContext.validate_for
      (automatedContext base authenticatedAt) (automatedPrincipal authenticatedAt) (automatedAuthority authority) =
      Result.ok (.Ok ()) := by
  rw [validate_for_exact _ _ _ (automated_text_domain base authenticatedAt)]
  rw [if_pos (automated_accepted base authority authenticatedAt)]

theorem delegated_admitted (base : Context) (authority : Authority)
    (sourceAt actorAt : Option NativeTimestamp) :
    internal_auth.request.HubRequestContext.validate_for
      (delegatedContext base sourceAt) (delegatedActor actorAt) (delegatedAuthority authority) =
      Result.ok (.Ok ()) := by
  rw [validate_for_exact _ _ _ (delegated_text_domain base sourceAt actorAt)]
  rw [if_pos (delegated_accepted base authority sourceAt actorAt)]

/-- No supplied base context, authority or timestamp premise is needed for
inhabited admission in any mode. The generic results above retain arbitrary
unexamined inputs; this closed specialization makes non-vacuity explicit. -/
theorem all_modes_admitted :
    internal_auth.request.HubRequestContext.validate_for
      (directContext seedContext (some seedAuthenticatedAt))
      (userPrincipal (some seedAuthenticatedAt)) (directAuthority seedAuthority) =
      Result.ok (.Ok ()) ∧
    internal_auth.request.HubRequestContext.validate_for
      (automatedContext seedContext (some seedAuthenticatedAt))
      (automatedPrincipal (some seedAuthenticatedAt)) (automatedAuthority seedAuthority) =
      Result.ok (.Ok ()) ∧
    internal_auth.request.HubRequestContext.validate_for
      (delegatedContext seedContext (some seedAuthenticatedAt))
      (delegatedActor (some seedIssuedAt)) (delegatedAuthority seedAuthority) =
      Result.ok (.Ok ()) := by
  exact ⟨direct_admitted seedContext seedAuthority (some seedAuthenticatedAt),
    automated_admitted seedContext seedAuthority (some seedAuthenticatedAt),
    delegated_admitted seedContext seedAuthority (some seedAuthenticatedAt) (some seedIssuedAt)⟩

#print axioms direct_admitted
#print axioms automated_admitted
#print axioms delegated_admitted
#print axioms all_modes_admitted

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthContextWitnesses.direct_admitted,
      ``InternalAuthContextWitnesses.automated_admitted,
      ``InternalAuthContextWitnesses.delegated_admitted,
      ``InternalAuthContextWitnesses.all_modes_admitted] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed admitted context witness ({declaration}): {found}"

end InternalAuthContextWitnesses
