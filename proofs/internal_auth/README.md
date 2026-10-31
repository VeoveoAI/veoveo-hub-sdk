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

The context-bearing direct/delegated cases, all-owned nonempty permission sets,
independent complete signer claims, valid constructed examples and semantic
mutation controls are still required before qualifying full issuance.

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
model-boundary audit. The complete context-bearing issuer theorem and admitted
direct and delegated constructions are the next proof obligations; they are not
proved by the initial bootstrap or recipient results.
