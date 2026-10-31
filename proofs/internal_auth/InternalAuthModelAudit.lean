import RequestCompositionSdk.Funs
import Lean.Util.CollectAxioms

open RequestCompositionSdk

-- Dependency audit only: compilation/standard axioms do not prove native-library
-- refinement, totality, admission or the issuer/recipient correctness contract.
#print axioms internal_auth.request.HubRequestContext.validate_for
#print axioms internal_auth.exchange.validate_delegation
#print axioms veoveo_types.permission_names.PermissionName.is_owned_by
#print axioms internal_auth.scope_claim
#print axioms internal_auth.StandardClaims.new
#print axioms internal_auth.AccessTokenClaims.from_identity
#print axioms internal_auth.recipient.bind
#print axioms internal_auth.HubInternalTokenIssuer.create_identity
#print axioms internal_auth.HubInternalTokenIssuer.issue

run_cmd do
  let standard := #[``propext, ``Classical.choice, ``Quot.sound]
  for declaration in #[
      ``RequestCompositionSdk.internal_auth.request.HubRequestContext.validate_for,
      ``RequestCompositionSdk.internal_auth.exchange.validate_delegation,
      ``RequestCompositionSdk.veoveo_types.permission_names.PermissionName.is_owned_by,
      ``RequestCompositionSdk.internal_auth.scope_claim,
      ``RequestCompositionSdk.internal_auth.StandardClaims.new,
      ``RequestCompositionSdk.internal_auth.AccessTokenClaims.from_identity,
      ``RequestCompositionSdk.internal_auth.recipient.bind] do
    let found ← Lean.collectAxioms declaration
    unless (found.filter (fun axiomName => !standard.contains axiomName)).isEmpty do
      throwError "unreviewed SDK validation/claims primitive in {declaration}: {found}"
  let identity := standard ++ #[
    ``RequestCompositionSdk.chrono.offset.utc.Utc.now,
    ``RequestCompositionSdk.uuid.v7.Uuid.now_v7,
    ``RequestCompositionSdk.alloc.string.ToString.Blanket.to_string,
    ``RequestCompositionSdk.identity.JwtId.parse,
    ``RequestCompositionSdk.internal_auth.ensure_jwt_crypto_provider]
  let created ← Lean.collectAxioms ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.create_identity
  unless (created.filter (fun axiomName => !identity.contains axiomName)).isEmpty do
    throwError "unreviewed SDK identity primitive: {created}"
  let signing := identity.push
    ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.sign_forwarded
  let issued ← Lean.collectAxioms ``RequestCompositionSdk.internal_auth.HubInternalTokenIssuer.issue
  unless (issued.filter (fun axiomName => !signing.contains axiomName)).isEmpty do
    throwError "unreviewed SDK issuer primitive: {issued}"
