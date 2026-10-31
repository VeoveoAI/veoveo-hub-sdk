import InternalAuthContextExamples

open Aeneas Aeneas.Std RequestCompositionSdk InternalAuthSpecification
open InternalAuthContextExamples

namespace InternalAuthIdentityExamples

/-- Independent fixture inputs. These definitions import no identity-correctness
module and can be reused by source controls without importing a false theorem. -/
def requestedDeadline : NativeTimestamp := {
  seconds := IScalar.ofInt 4102444920
  nanoseconds := UScalar.ofNat 0 }

def inheritedDeadline : NativeTimestamp := {
  seconds := IScalar.ofInt 4102444860
  nanoseconds := UScalar.ofNat 0 }

def sourceWithExpiry (expires : NativeTimestamp) : Context := {
  seedContext with access_token := { seedContext.access_token with expires_at := expires } }

/-- The private identity constructor preserves arbitrary singleton permissions.
Public issue ownership/current-policy provenance is a separate obligation. -/
def directRequest (base : Request)
    (permission : veoveo_types.permission_names.PermissionName) : Request := {
  base with
  client := ⟨"console"⟩
  actor := userPrincipal (some seedAuthenticatedAt)
  authority := directAuthority seedAuthority
  permissions := ⟨[permission]⟩
  request_context := some (directContext (sourceWithExpiry seedExpiresAt)
    (some seedAuthenticatedAt))
  acting_server := none
  acting_task := none
  delegation_context := none
  expires_at := requestedDeadline }

/-- A real nonempty ordered inherited chain, with its originating Task owned by
that first actor. These typed values assert no native Task liveness/authenticity. -/
def inheritedContext (previous : internal_auth.ActingServer)
    (task : identity.OpaqueTaskId) : internal_auth.exchange.DelegationContext := {
  actors := alloc.vec.Vec.from [previous] (by
    change 1 ≤ Usize.max
    rcases Usize.bounds_eq with width | width
    · rw [width, U32.max_eq]; decide +kernel
    · rw [width, U64.max_eq]; decide +kernel)
  task := some { server := previous.server, deployment := previous.deployment, task := task }
  expires_at := inheritedDeadline }

/-- Source expiry is before the clock fixture, inherited deadline is future and
requested deadline is later. The acting-server Option controls the exemption
from another source-token cap, independently of the provenance enum in general. -/
def actingRequest (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId) : Request := {
  base with
  client := ⟨"agent-client"⟩
  actor := delegatedActor (some seedIssuedAt)
  authority := delegatedAuthority seedAuthority
  permissions := ⟨[permission]⟩
  request_context := some (delegatedContext (sourceWithExpiry seedAuthenticatedAt)
    (some seedAuthenticatedAt))
  acting_server := some acting
  acting_task := none
  delegation_context := some (inheritedContext previous task)
  expires_at := requestedDeadline }

/-- Concrete non-leap timestamps, retaining full DateTime values. No current
native clock observation is inferred from these mathematical fixture comparisons. -/
theorem deadline_order :
    Native.compareTimestamp requestedDeadline seedExpiresAt = .gt ∧
    Native.compareTimestamp requestedDeadline inheritedDeadline = .gt ∧
    Native.compareTimestamp seedExpiresAt seedIssuedAt = .gt ∧
    Native.compareTimestamp inheritedDeadline seedIssuedAt = .gt ∧
    Native.compareTimestamp seedAuthenticatedAt seedIssuedAt = .lt ∧
    Native.compareTimestamp inheritedDeadline inheritedDeadline = .eq := by
  decide +kernel

theorem direct_constraints (base : Request)
    (permission : veoveo_types.permission_names.PermissionName) :
    delegationError (directRequest base permission) = none ∧
    effectiveDeadline (directRequest base permission) = seedExpiresAt := by
  simp [directRequest, delegationError, delegationDeadline, effectiveDeadline,
    earlier, directContext, inputContext, sourceWithExpiry, deadline_order.1]

theorem acting_constraints (base : Request)
    (permission : veoveo_types.permission_names.PermissionName)
    (previous acting : internal_auth.ActingServer) (task : identity.OpaqueTaskId) :
    delegationError (actingRequest base permission previous acting task) = none ∧
    effectiveDeadline (actingRequest base permission previous acting task) = inheritedDeadline := by
  simp [actingRequest, inheritedContext, delegationError, delegationDeadline,
    effectiveDeadline, earlier, alloc.vec.Vec.from_val, deadline_order.2.1,
    deadline_order.2.2.2.2.2]

#print axioms deadline_order
#print axioms direct_constraints
#print axioms acting_constraints

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[``InternalAuthIdentityExamples.deadline_order,
      ``InternalAuthIdentityExamples.direct_constraints,
      ``InternalAuthIdentityExamples.acting_constraints] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed identity fixture dependency ({declaration}): {found}"

end InternalAuthIdentityExamples
