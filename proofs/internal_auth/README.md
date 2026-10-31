# Internal-token assembly (manual, in progress)

This SDK-owned package models the actual `issue`, `create_identity`, request-context,
claims, actor-chain and recipient functions. It does not import Hub proof code or
replace the issuer with a function that returns a chosen identity. The model
build and dependency audit pass. The initial bootstrap-identity,
signing-continuation and recipient contracts also pass. Generic complete identity
construction, delegation refusal priority and context-bearing Direct/acting-server
successes, independent complete claims assembly and generic public-issuer
composition now pass in the recorded operational graph. Populated issuer examples
and source controls remain pending; native-helper/signing/Hub refinement remains
explicit.
There is no runner or CI integration.

The input is the recorded SDK `e268032` slice, Core `120306e`, with the reviewed
delegated-ID string expression. Extraction succeeds with both roots and all 15
inventoried own functions transparent. The 35 focused internal-auth tests, format
check and strict library Clippy pass against both the `e268032` fixture and a
private current-main `154a473` compilation context. The exact runtime expression
is adopted and recorded in SDK `19a1e651`; these are focused native/source checks,
not issuer proof results.
Current SDK main has additional unrelated work. The inspected local Hub source
pins SDK `239622d`; the observed newer Hub remote-tracking source pins `154a473`
and includes additional App-origin cases. Neither whole-current-main applicability,
pinned-Hub integration nor deployed applicability is claimed.
Generated `Types.lean` and `Funs.lean` are copied byte-for-byte from those reviewed
outputs. Untouched external templates are retained under `extraction/` and are
never compiled or used as model axioms. Dependency pins and other proof packages
are unchanged. Complete issuer correctness and Hub integration remain pending.

The string expression remedy preserves the existing delegated comparison at the
same guard position. Pinned Aeneas erases `fmt::Arguments` to Unit, so its old
formatted comparison could not support an exact argument-dependent model. The
remedy is proof preparation, not a security rule or reported runtime defect.
The second preparation change renames only the private `from_identity` parameter
and its uses to `token_identity`: the old binder shadowed the generated `identity`
module name in Lean. The 35 focused native tests pass in 16.525 s, format in
0.295 s and strict library Clippy in 3.824 s for this adopted candidate. Fresh
extraction changes only that binder/references, wrapping and
source spans; external interfaces are unchanged. No generated output was repaired
by hand.

## First model checkpoint

The combined model build passes in 9.91 s and the dependency audit in 2.57 s.
All seven audited own validation/ownership/renderer/claims/recipient functions
have standard-only axiom closures. `create_identity` additionally retains exactly
five explicit native observations: clock, UUID generation, UUID text conversion,
fallible ID parsing and provider setup. `issue` adds only the concrete signer.
The executable allowlists pass with no `sorryAx`; this is a source-model and
dependency checkpoint, not an issuer theorem or a native refinement proof. See
the [model receipt](evidence/internal-auth-model.json).

## Initial identity and recipient contracts

`InternalAuthCorrectness.lean` proves the actual identity constructor's complete
output for the native API's context-omitted bootstrap case: arbitrary typed
payloads, empty permissions, and no acting server, Task or inherited delegation.
Expiry rejection and native ID-parse errors are preserved. Provider, clock and
ID outcomes remain explicit external observations; no own validation-success
premise is supplied.

The issue theorem connects that identity to the actual claims constructor and
concrete signer, preserving signing errors and modeled failure/divergence.
It is a continuation factorization; complete independent claims assembly is
established separately below. The separate recipient theorem proves exact
server/resource/deployment comparison, error priority and preservation of the
entire presented recipient triple under explicit comparator laws. It does not
perform cryptographic verification.

All four theorem audits pass without `sorryAx`. The final direct check completed
in 2.86 seconds. `InternalAuthSpecification.lean` independently describes the full
context, deadline and claims contract; its elaboration alone establishes no
correctness theorem. `InternalAuthValueLaws.lean` proves six finite clone/equality
laws used by the context/claims proofs, with standard-only axiom audits.
See the [checkpoint receipt](evidence/identity-recipient-checkpoint.json).

These modules are opt-in manual targets:

```sh
lake build InternalAuthSpecification InternalAuthValueLaws InternalAuthCorrectness
```

Populated all-owned nonempty-permission issuer constructions and source controls
are still required. The generic public composition below remains model-relative;
native helper and cryptographic correspondence is not inferred from it.

## Exact context admission

`InternalAuthContextProof.validate_for_exact` proves the actual extracted
`HubRequestContext::validate_for` returns `Ok(Ok(()))` exactly when the independent
`ContextAccepted` predicate holds, and `Ok(Err(InvalidRequestContext))` otherwise.
It covers Direct, Automated and Delegated provenance, full direct/automated
Principal equality including every set and `authenticated_at`, delegated actor
identity and inherited attributes, common tenant/Work Context/token metadata,
and the browser-session restriction.

The explicit `ContextTextDomain` assumptions are five pointer-sized UTF-8 byte
bounds: source subject, actor ID, actor subject, token issuer and OAuth client.
UTF-8 contents and conversion roundtrip are proved against the operational string
model. These representation assumptions are not new Rust admission checks, and no
premise assumes that the validator succeeds. The finite independent
`invocationMode` projection is an `abbrev` so dependent decision dictionaries
specialize consistently; its match and the admission predicate are unchanged.

The direct check passed in 4.60 seconds and the named target in 5.82 seconds.
Their executable dependency audit permits
only `propext`, `Classical.choice` and `Quot.sound`, and rejects `sorryAx`.
The proof keeps an 800,000-heartbeat allowance local to this theorem and uses
Lean's supported `backward.split = false` only for the final case analysis;
neither setting changes runtime behavior or the statement. See the
[context receipt](evidence/context-admission.json).

The opt-in target is:

```sh
lake build InternalAuthContextProof
```

This proves model-relative context admission, not authentication of the supplied
context, current-policy provenance, expiry, complete issuance/claims, or verifier
cryptography. The two actual-source context controls below qualify this
validator contract. Native collection/string/allocation correspondence remains
the boundary described below.

## Constructed admitted contexts

`InternalAuthContextExamples.lean` supplies populated independent input
constructions aligned to the native `hub-request-context.json` modes. It imports
no context-correctness theorem, so semantic-control witnesses can reuse the
inputs without importing a failed mutated proof. Direct has a browser session
family and nonempty groups, group roles, roles, scopes, labels and assurances.
Automated is a service principal. Delegated has a distinct service actor, inherits
nonempty source scopes/labels/assurances and clears user groups and roles. Scope
traversals are unique and in lexical order.

`InternalAuthContextWitnesses.lean` proves actual validation success in all three
modes from the independently proved admission predicates. The five text bounds
are discharged for the concrete fixture strings, rather than supplied as
admission assumptions. A closed seed context and authority give a no-argument
three-mode success theorem, with distinct source/actor authentication times in
the delegated case. Generic theorems also preserve arbitrary supplied audit,
execution, token dates/audience/ID, and authority membership/policy/output fields;
those preserved inputs are not authenticated evidence. Authority provenance is
not coupled to an acting-server value by these context-only constructions.

All eight key declarations have executable standard-only dependency audits. The
first passing direct checks completed in 2.53 s (examples) and 2.66 s (witnesses).
The final named-target check passed in 7.32 s with all eight audits and no
`sorryAx`. Its inputs use the native fixture's UUIDv7-shaped audit ID; that
fidelity correction changes no validator condition. Exact source versions,
final target results and prior proof-authoring errors are in the
[admitted-context receipt](evidence/context-examples.json).

```sh
lake build InternalAuthContextExamples InternalAuthContextWitnesses
```

This is model-relative admission of constructed inputs, not full issuance,
authenticity, cryptography, current-policy provenance or complete issuance. The
context controls below qualify wrong acceptance/refusal sensitivity. The generic
issuer composition below reuses these contracts; native-helper/signing/Hub
correspondence remains open.

## Actual-source context controls

Two deliberate private Rust mutations qualify the frozen all-mode theorem;
canonical runtime, raw models and checked theorem statements are unchanged.
The [receipt](evidence/context-semantic-controls.json) records exact patches,
source/tool/output hashes, timings, isolated build inputs, proof failures,
independent witnesses and restoration checks.

- [Direct actor bypass](mutations/direct-actor-equality-bypass.patch) removes only
  Direct's actor/source rejection. The [independent witness](mutations/DirectActorBypassWitness.lean)
  adds `superuser` to the populated user's roles while retaining every other
  Principal field. The mutant accepts and the independent predicate refuses.
  This proves no downstream grant change.
- [Automated kind mutation](mutations/automated-kind-refusal.patch) changes only
  the required kind Service to User. The [independent witness](mutations/AutomatedKindRefusalWitness.lean)
  proves refusal of the independently admitted populated Service fixture. The
  frozen theorem separately exposes valid-Service refusal and User acceptance
  cases; these are distinct counterexamples.

Each source variant used the unchanged two-root/34-include/seven-opaque extraction
selection and a fresh package build directory. The actual raw model and unaffected
helpers built first, then the unchanged `validate_for_exact` contract failed on
its semantic obligation. Standalone witnesses imported Examples/raw/value laws,
never the failed correctness module or its objects. Their seven key audits pass
with standard-only dependencies and no `sorryAx`. Direct's witness-only retry
added the missing native inequality default unfolding; the Automated patch-path
and existing-job failures stopped before Charon. These were qualification setup
or proof-authoring errors, not native defects.

Both patches were reversed in their original private source trees. Full original
173-file maps and all four canonical raw output hashes were restored exactly.
Fresh restored builds of ContextProof, Examples and Witnesses passed all nine
audits in 17.317 s (Direct) and 17.032 s (Automated). The controls apply to the
recorded e268 source plus adopted String/binder preparation, not automatically
to another SDK/Hub pin or extraction. These mutation witnesses are checked only
against their respective mutant graph and are not default targets.

The manual sequence in each fresh variant is:

```sh
lake build RequestCompositionSdk InternalAuthSpecification InternalAuthValueLaws InternalAuthContextExamples
lake env lean InternalAuthContextProof.lean # expected semantic rejection
# Run only the witness matching that private mutant graph:
lake env lean mutations/DirectActorBypassWitness.lean
# or: lake env lean mutations/AutomatedKindRefusalWitness.lean
# After same-tree source reversal and exact raw4 regeneration, in a fresh package:
lake build InternalAuthContextProof InternalAuthContextExamples InternalAuthContextWitnesses
```

These intentional faults establish model-relative contract sensitivity and
constructed wrong results. They introduce no runtime rule, security defect claim,
authentication, complete issuer/claims or cryptographic verification guarantee.
No native/full-stack suite, deployment, dependency update or paid call was needed.

## Complete identity construction

`InternalAuthIdentityProof.create_identity_exact` proves the actual transparent
constructor equals an independent complete outcome for arbitrary typed requests
and permissions. Optional context is generic: this includes context omission with
acting/delegation fields, as well as supplied Direct, Automated or Delegated
contexts. No own validation/construction-success premise is used. The only input
restriction is `RequestTextDomain`, the existing five physical text bounds for
each supplied context.

The constructor's priority is preserved: provider observation; pre-cap by the
inherited deadline; acting/Task/chain consistency, depth and originating-Task
ownership; clock observation; independent context admission and OAuth-client
match; effective expiry refusal; then UUID/text/fallible-ID observations. The
inherited vector is proved nonempty before head indexing, and depth bounds derive
safe native-size addition. Every output field is explicit through `identityValue`,
including complete Principal, authority, permissions, request context, acting
server/Task, ordered inherited chain/Task, full timestamps and parsed token ID.

Direct calls with context cap expiry by the original source access token. Calls
with an acting server retain the requested/inherited cap, even when that source
token has already expired; this condition is keyed to the Option, independently
of provenance. Both Rust errors and Aeneas primitive failures/divergence remain
visible. Issued-at and not-before equal the same full clock value. The constructor
asserts no 60-second or subscription TTL; those come from the separate Hub caller.

`InternalAuthIdentityExamples.lean` imports no identity-correctness theorem. Its
reusable independent fixtures support two actual success constructions: a
populated Direct context with an earlier source cap, and an acting-server case
with a nonempty inherited actor chain and matching originating Task, expired
source token, future inherited cap and later requested deadline. The fixtures
supply nonempty permissions; the private constructor preserves arbitrary supplied
values, while public issue ownership/current-policy provenance is separate.
Success premises describe only native provider/clock/UUID/text/parser outcomes,
not an own-code success. No Task liveness, authority authenticity, UUID uniqueness
or actual native clock value is inferred from the fixtures.

The generic result passed directly in 4.86 s. Independent fixtures passed in
2.52 s and the complete proof with both constructions in 5.06 s. The exact final
named targets pass in 8.14 s. All nine executable audits reject `sorryAx`:
fixtures and validation laws are standard-only; the complete identity and its
constructions additionally retain exactly provider setup, clock, UUID generation,
UUID text and fallible JWT-ID parsing. See the compact
[identity receipt](evidence/identity-construction.json).

```sh
lake build InternalAuthIdentityExamples InternalAuthIdentityProof
```

Two actual-source deadline controls now qualify this contract. Removing only the
Direct/no-acting source minimum makes the unchanged generic theorem fail; a
standalone witness derives a complete returned identity whose requested deadline
is strictly later than the independent source cap. Repeating that source minimum
for an acting server also rejects the unchanged theorem; a separate witness
proves refusal of an independently admitted request with an inherited actor chain,
matching originating Task, future inherited deadline and expired source token.
These are deliberate test faults, not discovered product defects. The witnesses
import independent fixtures and unchanged compiled helpers, never the failing
identity theorem or its OLean. They assume only typed native observations.

For each fault, same-tree source reversal reproduced all four canonical generated
outputs, and a fresh build of the unchanged identity targets passed: Direct in
21.186 s and acting in 19.729 s. The two wrong-result witnesses passed in 2.698 s
and 2.972 s respectively; all passing witness and restored-target audits exclude
`sorryAx` and retain only their recorded standard/native boundaries. The initial
acting witness failed proof normalization before its corrected leaf passed; setup
failures and that proof-authoring failure are recorded separately. See
[deadline semantic controls](evidence/identity-deadline-controls.json).

Generic public guard and identity/claims-to-signer composition is now described
below. Native helper/signing correspondence, verification wrapper and Hub
pinned-stack integration remain pending. No runtime/model/extraction/dependency change or new native test
was required for this proof-only constructor checkpoint.

## Complete claims assembly

`InternalAuthClaimsProof.from_identity_exact` proves the actual transparent
`AccessTokenClaims.from_identity` returns the complete deterministic `claimsValue`
for any typed identity in `ClaimsTextDomain`. `actual_claims_complete` connects that
result to the unchanged independent `ClaimsExact` specification. No own-code
validation, subcall-success or desired-claims premise is supplied. The domain
contains physical text bounds for actor ID, audience, each scope and the full
rendered scope text; it adds no native admission rule.

The result includes issuer, subject, audience, OAuth client, token ID, all three
NumericDate seconds projections, issuer/current/inherited actor nesting, scope
text, groups, roles, the empty-or-singleton permission detail with every action,
and the complete invocation with Principal, authority, request context, deployment,
acting server/Task and inherited delegation fields. The actual scope and prior-actor
loops are proved finite and preserve exact traversal, spacing and nesting; actual
`StandardClaims.new`, clones and Option-to-Vec collection are derived. Native
canonical scope text still requires the existing sorted/unique native traversal
correspondence, and date/allocation/UTF8 refinement boundaries below remain.

`InternalAuthClaimsComponents.lean` holds these reusable proofs and the independent
claims value. `InternalAuthClaimsExamples.lean` imports no full claims-correctness
theorem. Its closed rich fixture has two lexical scopes, two inherited actors then
the current actor, a matching originating Task, two permission actions and a
populated context/authority. The empty-permission Direct fixture retains populated
metadata and scope text while emitting no authorization detail or prior actor.
Their independent `ContextAccepted` facts are separately proved; claims assembly
itself authenticates no input and establishes no Task liveness or policy grant.

The generic proof passed in 2.83 s; the split components, independent examples and
complete proof passed in 2.79 s, 2.76 s and 2.71 s. Final named targets passed in
7.58 s. All fourteen executable audits use only standard axioms and exclude
`sorryAx`; this constructor calls no provider, clock, UUID generator or signer.
See [claims assembly evidence](evidence/claims-assembly.json).

```sh
lake build InternalAuthClaimsComponents InternalAuthClaimsExamples InternalAuthClaimsProof
```

The actual-Rust permission-detail inversion control and exact restoration are
qualified below. Generic public composition is described next; Core namespace
refinement, cryptographic signing, verification wrapper and Hub pinned-stack
integration remain open. No runtime, generated body, operational
model, dependency pin or native suite changed for this proof milestone.

## Actual-source permission-detail control

A one-expression private Rust fault removes `!` from the `from_identity`
permission-empty guard. The unchanged complete-claims theorem rejects both wrong
branches: a nonempty set loses its detail, while an empty set emits a spurious
empty detail. `mutations/PermissionDetailInversionWitness.lean` independently
derives the complete actual wrong values for the two closed fixtures and proves
they violate unchanged `ClaimsExact`. All other independently specified fields
are preserved. This is a deliberate regression fault, not a product defect or a
current-policy-grant assertion.

The witness imports fresh unaffected Components and independent Examples, never
the expected-failing ClaimsProof or its OLean. Model/component build passed in
19.635 s; the frozen contract rejected in 2.758 s; the independent witness passed
in 2.764 s with three standard-only audits and no `sorryAx`. Same-tree source
reversal restored all 173 archived source hashes. Regeneration reproduced all
four canonical raw outputs exactly, and a fresh three-target build passed in
21.292 s with all fourteen claims audits standard-only. OLean identities are
recorded for each build; binary equality to earlier packages is not claimed.

The initial raw diagnostic used a stale pre-binder comparison; the corrected
comparison against checked canonical raw contains only the inverted Boolean.
A restoration static-review pause concerned an unused old LLBC initializer;
the continuation binds its effective digest to the measured restored receipt.
Neither correction changed raw/source bytes or required repeating extraction.
See [permission-detail control evidence](evidence/claims-permission-control.json).
Temporary extraction trees, runners and log streams are evidence assets rather
than package source. Canonical runtime, models and theorem statements are unchanged;
the native namespace refinement, cryptographic signing and Hub boundaries remain
open.

## Complete generic public issuer (model-relative)

`InternalAuthIssueProof.issue_exact` proves the complete actual public `issue`
body equals the independent `issueOutcome` for arbitrary complete typed requests
in `IssueTextDomain`. This covers all permission sets, optional-context and
acting/delegation combinations within physical representation bounds. It assumes
no own-code validation/construction/claims success and no successful signer.

The independent `firstForeign` preserves the exact first offending permission in
the retained traversal. Its error precedes identity processing.
`foreign_permission_refused` needs only physical permission/server bounds and the
computed input relation; provider/time/ID/signing success is not a premise.
Otherwise the theorem preserves every independent identity outcome, derives the
created identity's claims-domain bounds from request-field preservation, and
passes exactly the complete `claimsValue` to actual `sign_forwarded`. Signer Rust
errors propagate; success returns the same complete identity and returned bearer.
Aeneas primitive failures/divergence are retained. These are value/argument and
dependency equations, not instrumented effect-order or call-count traces.

Ownership is explicitly relative to the existing operational Core namespace
model: first-ASCII-dot UTF-8 byte-prefix equality, including empty/no-dot modeled
values. No unproved parsed-name invariant narrows the domain. The unchanged helper
exposure probe reached a transparent Core body and opaque third-party
`split_once`, but pinned Aeneas failed on `str::pattern` type signatures and emitted
no Lean output. The source/native helper refinement gap remains; standard-only
ownership/scan audits do not establish unconditional native ownership. Native
sorted/unique traversal, UTF8/allocation/date/scalar correspondence and signing
serialization/cryptography remain explicit boundaries.

The independent specification typechecked in 2.56 s. The generic proof passed in
2.76 s and final named targets in 5.33 s. Both ownership/scan audits use only standard
axioms. Actual refusal and full issuer audits add exactly the existing six native
symbols: provider check, clock, UUID v7, UUID text, fallible JWT-ID parser and
concrete signer. No `sorryAx` is accepted. See the
[public-issue receipt](evidence/public-issue.json).

```sh
lake build InternalAuthIssueSpecification InternalAuthIssueProof
```

Closed populated nonempty-permission issuer success/refusal examples and a
meaningful actual-Rust issuer source control are the next qualification steps.
They are not included in this generic checkpoint. Native Core-helper refinement,
cryptographic verification, supplied authority/current-policy provenance and Hub
pinned-stack composition remain open. No runtime/generated/model/dependency change
or native/full-stack suite was needed for this proof-only checkpoint.

## Operational boundaries

`TypesExternal.lean` and `FunsExternal.lean` give ordinary native-library operations
explicit value semantics. Set values retain an ordered traversal list. Native set
uniqueness/sorted traversal and valid scope spellings are explicit correspondence
conditions; no arbitrary Finset enumeration is treated as canonical scope text.
`Native.ScopeTraversalSorted` states the explicit lexical traversal law for scope
values; it is not asserted as an axiom. Cloning traverses actual values, iterator
find invokes the real predicate, and
string append/first-dot namespace projection retain their input bytes. No business
validation algorithm is duplicated in the models.

Text conversion checks the actual pointer-sized byte bound and UTF-8 validity.
Native Rust strings meet these representation conditions; model failures for
malformed/unrepresentable values are not new Rust authorization checks. Time retains
signed I64 seconds and nanoseconds. `Native.TimestampNonLeap` restricts the
correspondence domain to ordinary nanoseconds below one billion; Chrono also
supports leap-second values, which are not claimed covered by that law. This is
not a native rejection rule. NumericDate is the seconds projection; positive
full-DateTime lifetime does not imply positive whole-second token lifetime or an
exact JWT timestamp roundtrip.

The only open primitive observations are clock sampling, UUID generation,
UUID text conversion, fallible native JWT-ID parsing, crypto-provider setup and
concrete `sign_forwarded` with the actual complete claims argument. Both Aeneas
failure/divergence and the Rust Result error remain visible. No observation is an
assumed context/identity/issuer success. Signing serialization and cryptographic
verification correspondence remain external. Context/deadline checks, recipient
binding and the `PermissionName::namespace`/`is_owned_by` wrappers are extracted
transparently. The private Core `permission_names::namespace` body is not
extracted: handwritten `Native.namespacePrefix` models its first-dot projection.
That own-helper/native refinement gap remains explicit before claiming full
native permission provenance. The private identity constructor does not call it.

Always-successful String, Vec and clone operations retain allocation/memory
availability and value/alias correspondence boundaries. Result and argument
equations in Aeneas value semantics are not an instrumented side-effect trace or
call-count proof. Native effect ordering remains source correspondence unless
explicitly modeled. Recipient binding returns and preserves the presented
`Recipient { server, deployment, resource }` triple. It does not by itself return
or preserve a whole identity; verifier parsing and crypto correspondence remain
outside that function's contract.

Use Lean 4.35.0 and the existing pinned Aeneas dependency. Builder checks use
an isolated private dependency/build tree with recorded effective configuration;
shared caches and dependencies are not patched. The initial command is the model
target `LAKE_NO_UPDATE=1 lake build RequestCompositionSdk`, followed by the named
model-boundary audit. Populated public-issuer examples and an actual-source guard
control are the next qualification steps. Native helper/refinement, cryptographic
signing, verifier and Hub integration gaps remain open.
