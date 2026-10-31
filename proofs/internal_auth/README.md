# Internal-token assembly (manual, in progress)

This SDK-owned package models the actual `issue`, `create_identity`, request-context,
claims, actor-chain and recipient functions. It does not import Hub proof code or
replace the issuer with a function that returns a chosen identity. The model
build and dependency audit pass. The initial bootstrap-identity,
signing-continuation and recipient contracts also pass; full context-bearing
issuance and independent complete signer-claims correctness remain pending.
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
It is a continuation factorization, not yet an independent proof of every claims
field. The separate recipient theorem proves exact server/resource/deployment
comparison, error priority and preservation of the entire presented recipient
triple under explicit comparator laws. It does not perform cryptographic verification.

All four theorem audits pass without `sorryAx`. The final direct check completed
in 2.86 seconds. `InternalAuthSpecification.lean` independently describes the full
context, deadline and claims contract; its elaboration alone establishes no
correctness theorem. `InternalAuthValueLaws.lean` proves six finite clone/equality
laws used by the next context/claims proof, with standard-only axiom audits.
See the [checkpoint receipt](evidence/identity-recipient-checkpoint.json).

These modules are opt-in manual targets:

```sh
lake build InternalAuthSpecification InternalAuthValueLaws InternalAuthCorrectness
```

The context-bearing direct/delegated issuance cases, all-owned nonempty permission sets,
independent complete signer claims, constructed issuance examples and issuer
semantic controls are still required before qualifying full issuance.

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
context controls below qualify wrong acceptance/refusal sensitivity; complete
claims and deadline composition remain pending.

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
verification correspondence remain external; actual own context, deadline,
permission namespace and recipient checks stay transparent.

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
model-boundary audit. The complete context-bearing issuer theorem and admitted issuance constructions
are the next proof obligations. Context admission and its controls do not close
the issuer or recipient verification gaps.
