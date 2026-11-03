# Repository grant details (manual)

This package targets the actual SDK `PermissionsDetail::new`, `RepositoryDetail::new`
and `RepositoryDetail::grant`. The generated functions and types are exact output from
unmodified SDK `7360c0f` with Core `422dcf2`. The [extraction
receipt](evidence/extraction-20261031.json) records three roots, seven transparent own
functions, eight transparent own types, full field/ID checks, and successful
Charon/Aeneas translation. The fresh model build passed in 10.773 seconds and the proof
passed in 3.927 seconds, with all 13 executable guards and only standard axioms. The
existing native test passed, rejected the deliberate actions-dropping fault, then passed
after exact source restoration. The fresh mutant model compiled; the unchanged proof
rejected the wrong action value, and the independent witness passed. The final fresh
restored model and proof also passed. The [qualification
receipt](evidence/qualification-20261031.json) records measured stages and states the
scope and remaining external boundaries. Extraction alone is no correctness claim.

`RepositoryGrantProof` states complete permission/repository construction, projection of
the full repository ID and action set, and the actual new→grant roundtrip. Empty actions
are a legal read grant and have an explicit specialization. Constructor injectivity
prevents complete grants collapsing into one detail. Actual generated ID/action clone
laws are derived; no own-function success or desired whole-grant result is a premise.
Executable closure guards reject `sorryAx` and all axioms beyond `propext`,
`Classical.choice` and `Quot.sound`.

## Source and primitive boundaries

`SdkGrantDetailModel/{Types,Funs}.lean` are copied byte-for-byte. Untouched external
templates are retained under `extraction/` and are not compiled. Handwritten external
models supply Finset-valued BTreeSet, standard String clone and set clone. Set clone
traverses a finite observation list and calls the supplied actual element clone,
threading every returned value and short-circuiting modeled failure/divergence. Its
identity law is proved from a pointwise element law, then instantiated with the actual
generated String-newtype clones. It does not assume a chosen set result. The empty-set
constructor model is additional standard support for the synthetic source fault, unused
by baseline generated functions.

Theorems are universal Result equations **of these operational models**. Native Rust
allocation failure, abort/panic behavior, allocator cloning/effects, tree
representation, reference/alias/lifetime correspondence and native traversal/effect
order are external refinement boundaries. The allocator tag is erased by the value
model. String clone models exact value preservation under the same allocation/memory
boundary; it is not a proof of Rust's allocator or standard library. Finset equality
preserves whole action membership, not a native serialized traversal order. The finite
traversal must not be described as a call-count/side-effect trace for an arbitrary
effectful custom Clone implementation.

RepositoryId, RepositoryAction and PermissionName retain their full actual String
representations. Their parsers, validity checks and domain authenticity are not proved;
no lossy ID tag or hash substitutes for text. Detail kind is the native enum
constructor. JSON/serde/wire equivalence, signing/cryptography, issuer→verifier
composition, Hub assignment/group-grant computation and native Lore authorization are
outside this package. The separate full current verifier and producer translation
obligations remain open; this detail contract does not replace them.

## Manual checks

Use Lean 4.35.0 with pinned Aeneas `fdebf20`. The existing qualified private dependency
contains the recorded [instance-name
patch](https://github.com/VeoveoAI/veoveo-hub/blob/b5c08321d752c9e42a817b076fe82b2d53900619/proofs/request_composition/tooling/qualified-discriminant-instance.patch).
Reproduction retrieves the exact patch from that immutable history link; its
former Hub package was retired. No new tool patch is introduced. Whether this graph requires that existing patch and
clean-upstream reproduction are unmeasured; builder dependency identity must be recorded
explicitly. No CI/default target or native runtime integration is added.

```sh
LAKE_NO_UPDATE=1 lake build SdkGrantDetailModel
LAKE_NO_UPDATE=1 lake env lean RepositoryGrantProof.lean -o .lake/build/lib/lean/RepositoryGrantProof.olean
```

The new named `RepositoryGrantProof` target is also manual. Model ownership explicitly
includes all four modules to avoid a missing-import setup check. Baseline validation
must precede the control; accepted source/config hashes remain frozen while checked.

## Actual-source sensitivity control

[The patch](mutations/drop-projected-repository-actions.patch) changes only the actual
`RepositoryDetail::grant` projection to clone a fresh empty action set. This keeps the
existing action-clone dictionary reachable while deliberately losing nonempty actions.
It adds the ordinary standard empty constructor, not an own opaque oracle. Apply only to
an isolated exact SDK 7360c0f source; never edit the canonical runtime or generated
model. Fresh extraction must retain expected own interfaces before any unchanged-proof
rejection counts. Compiler/interface failures are setup diagnostics.

The independent [Funs-only witness](mutations/RepositoryGrantDropActionsWitness.lean)
uses a valid 32-character lowercase hexadecimal repository spelling and nonempty
`write`/`push-protected` actions. It states exact empty mutant output, inequality to the
supplied full grant, and retained legitimate empty-read projection. It passed in 2.754
seconds with three standard-axiom-only guards. It imports no failed correctness module
and must be checked only against freshly extracted mutated functions.

The existing native test is
`internal_auth::tests::a_runners_token_carries_the_callers_grant_on_its_linked_repository`.
It checks exact nonempty repository actions, signed/decoded detail fields, legal empty
reads without permissions and nonrunner None. The compiled fault failed its first
nonempty grant assertion (empty actions versus `push-protected` and `write`); its later
empty-read assertions were not reached on that failing execution. Baseline/restoration
and the independent Lean empty-read witness keep that distinction explicit. Reuse the
unchanged test, no new native harness:

```sh
cargo test --locked -p veoveo-hub-sdk --lib internal_auth::tests::a_runners_token_carries_the_callers_grant_on_its_linked_repository -- --exact --nocapture
```

Native checks are separate from Lean model proofs. After the intended negative result,
reverse the same source patch, verify the original 178-file map, regenerate all four raw
files exactly, and rebuild fresh own model/proof artifacts. Do not reuse accepted proof
OLeans against changed generated functions. Native baseline, fault and restored commands
took 51.382, 5.671 and 5.823 seconds. These are whole Cargo durations including
compilation, not grant-operation latency; harness times were 0.02, 0.01 and 0.02
seconds. Each command ran the exact named test once: baseline/restored passed 1/1 and
the fault failed its one test at the intended assertion. The original 178-file source map and all
four raw Lean outputs were restored exactly. Restored LLBC and native binary bytes
differ from their initial baselines; the receipt records their measured identities
without a cause claim.

The first proof attempt failed only on record-literal layout; the syntax repair passed
in 6.087 seconds with 13 guards and standard axioms only. The emitted standard empty-set
constructor then exposed an explicit `(T : Type _)` parameter, corrected from the unused
baseline support declaration’s implicit parameter. This is interface adoption, not a
native defect or a formal semantic rejection. The same repaired primitive is used for
the fresh baseline and mutant checks. The fresh baseline model and proof passed as
reported above. The mutant model then passed in 9.043 seconds. The unchanged proof
failed in 3.696 seconds on the exact empty-action value versus `detail.actions`; the
audit rejected cascading `sorryAx`, and no correctness OLean was emitted. The
independent Funs-only witness passed without importing that failed proof. Root reviewed
the complete residual and accepted it as a semantic rejection. The final fresh restored
model passed in 8.486 seconds and the proof in 3.184 seconds, again with all 13 guards
and only standard axioms. The 99,288-byte proof OLean has SHA-256
`7aebcc207eaf2154be1a34843cbe7da97094493aba459f6ef5e3b05aac4c0431`, exactly matching the
corrected baseline. The four freshly built model OLean hashes differ; their measured
identities are retained without a cause or binary reproducibility claim. No runtime
source was changed by this proof qualification.
