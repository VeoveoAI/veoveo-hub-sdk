# Authorization route interpretation

This **manual, on-demand** package proves a partial SDK boundary of
HUB-CONTRACT-01: exact resource precedence, ordered full-value template agreement,
and parsed root/path/query matching. It computes a **route requirement**, not final
user authorization. `Authenticated` does not bypass Hub profile, scopes, principal,
current policy or delegated-authority admission. No new runtime gate, permission,
wire field, dependency, CI check, hook or runner is introduced.

## Production correspondence

The source basis is SDK `8489a58d12f880e88bb71b226c720e57a5399084` plus the
recorded overlay. The standalone crate imports the actual private
[`authorization_decision.rs`](../../crates/veoveo-hub-sdk/src/registration/authorization_decision.rs)
with `#[path]`; no algorithm is copied into the proof crate. Native
[`authorization.rs`](../../crates/veoveo-hub-sdk/src/registration/authorization.rs)
and [`authorization_template.rs`](../../crates/veoveo-hub-sdk/src/registration/authorization_template.rs)
call these same functions.

The native adapter keeps `BTreeMap` exact lookup and raw entry iteration,
`BTreeSet` query membership, native template parsing and Core URI decomposition.
An exact hit returns before template parsing/URI decoding. Otherwise every raw
candidate is matched in native order, the first complete access value is retained,
and the first differing match stops immediately. `Authenticated` and
`Permission(scope)` are distinct complete values; no permission-only projection
is used to decide agreement. The native adapter introduces no additional
collection construction or heap allocation: matching borrows the original path
and decoded segment vectors and query set. No performance benchmark was run.
Root comparison still precedes the existing decoded segment allocation. Returned
Rust borrow identity is supported by adapter source review and native `ptr::eq`
regressions; Aeneas's value model alone does not prove alias identity.

An unchanged-source probe, including precise opaque parser/Core/map boundaries,
could not translate the external higher-ranked `core::str::pattern` signature.
The small private seam separates native representation from actual decision loops.
The parser remains native; its variable validation, terminal-Tail grammar, Core
percent decoding and duplicate decoded query-name rejection are not proved here.
The safe Tail comparison `active < path_len - 1` occurs only after establishing a
nonempty terminal-Tail branch and preserves the old cardinality rule.

## Statements and explicit assumptions

- `ResolutionProof`: total ordered resolution for arbitrary finite raw-entry
  cursors; exact-hit original cursor; actual immediate next cursor on first
  conflict; success iff an exact hit or nonempty matching full values all agree.
- `TemplateProof` and `PathProof`: total indexed path scan and complete path result,
  including bounds/termination, literal equality, one-segment Scalar, trailing
  empty-segment trimming and zero-or-more-segment terminal Tail. Scalar checks
  segment existence, not nonempty contents or domain-ID validity.
- `MatcherProof`: total complete path/query result and root comparison. The raw
  view theorem allows arbitrary query flags; only the native-consistent corollary
  assumes `query_empty` iff decoded keys are empty and no query implies no keys.
- `AuthorizationProof`: composes actual generated root/path/query calls with the
  resolver's success/refusal theorem. `NativeMatcherTrace` explicitly connects the
  native parser/decomposition/borrow wiring to those calls; it does not assume a
  desired authorization result or an already-filtered candidate collection.
- `ContractInterpretation`: satisfiable equality, raw list cursor, decoded-string
  and finite-query laws, with complete finite `Authenticated`/`Permission` values,
  accepted agreement, rejected conflict and exact override. This mathematical
  interpretation is not a replacement SDK wire type or standard-library proof.
  Native regressions separately use actual typed SDK `RouteAccess` values.

Operational laws require total correct equality **and inequality** for complete
access/root values; native `next` enumerates a finite raw suffix with progress;
segment equality/emptiness matches decoded text; query membership/emptiness matches
the native declared set. Exact-map lookup correspondence, native iteration order,
parser/Core projections, the parent call sequence and native borrow wiring remain
reviewed/tested adapter obligations. No assumption supplies the overall matcher
result or final agreement result. `⦃ … ⦄` is total correctness, excluding modeled
Rust panic/failure/divergence; unsupported translation is a separate tool failure.

This slice does **not** prove Serde/schema equivalence, declaration validity checks,
route-generation/caller correspondence, overlapping typed resource dispatch,
registration/Discover identity comparison, Hub policy, SDK dispatch or cryptography.
The SDK readback helper exists, but Hub admission/policy uses checked stored
registration: no runtime Discover equality comparison was found or added. Wire
and declaration tests supplement, rather than constitute, formal proof.

## Repeat manually

Use Aeneas `fdebf20d78e8007bb92fc17068f17fc2e6cfbbbf`, Charon
`de8736f7c995ac54dc490345cef9ea5143c06c62`, Rust `nightly-2026-09-17` and
Lean `4.35.0`; `lake-manifest.json` pins dependency sources. On Linux,
Charon's driver needs the pinned Rust toolchain's `lib` on `LD_LIBRARY_PATH`.
From this directory, with those tools on `PATH`:

```text
cd rust
cargo test --locked --lib
charon cargo --preset=aeneas --dest-file sdk_authorization_proof.llbc -- --manifest-path Cargo.toml --lib --locked
cd ..
aeneas -backend lean -dest . rust/sdk_authorization_proof.llbc
lake build
lake env lean ResolutionProof.lean
lake env lean TemplateProof.lean
lake env lean PathProof.lean
lake env lean MatcherProof.lean
lake env lean AuthorizationProof.lean
lake env lean ContractInterpretation.lean
```

`SdkAuthorizationProof.lean` is generated and never hand-edited. The default Lake
build includes every handwritten module; direct checks print target axiom closure.
Accepted proofs must contain no `sorryAx` or custom unproved facts. The Lean kernel
checks elaborated terms; trust also includes Rust/Charon/Aeneas translation and the
stated native adapter laws. This is not a proof of the Rust compiler, standard
library, allocator or full server endpoint.

Native focused checks use the production Rust toolchain from the SDK root:

```text
cargo fmt --all --check
cargo test --locked -p veoveo-hub-sdk --features testing --test registration_wire
cargo test --locked -p veoveo-hub-sdk --no-default-features --features contract --test contract_boundary
cargo clippy --locked -p veoveo-hub-sdk --no-default-features --features contract --lib -- -D warnings
```

## Semantic controls

Apply each patch independently in an immutable SDK-root snapshot with `patch -p1`.
Fresh Rust compilation, Charon, Aeneas and the generated-module Lake build must
succeed before running the **unchanged** semantic proofs and separate witness.
A failed compiler/extractor is not an accepted negative control. The two inverted
matcher guards deliberately retain the same primitive calls and function APIs.

| Patch | Separate Rust witness | Intended formal failure |
|---|---|---|
| `exact-hit-none.patch` | `tests::exact_access_overrides_conflicting_templates_without_visiting_them` | exact result and success |
| `ignore-conflict.patch` | `tests::every_raw_candidate_is_matched_and_first_conflict_short_circuits` | full agreement/immediate stop |
| `accept-mismatching-literal.patch` | `tests::scalar_keeps_empty_segment_and_tail_accepts_zero_or_many` | literal/path equivalence |
| `invert-query-membership.patch` | `tests::trailing_slash_and_query_checks_keep_native_semantics` | decoded query membership |
| `reject-empty-tail.patch` | `tests::scalar_keeps_empty_segment_and_tail_accepts_zero_or_many` | valid zero-segment Tail acceptance |

Restore in a fresh snapshot and recover exact source/model/proof identities and
all positive checks. Negative diagnostics may contain `sorryAx` from intentionally
failed elaboration; they are never accepted proof evidence. Raw logs, caches,
LLBC and snapshots stay outside Git in `.cache/lean-pilot/sdk-contract-proof`;
only compact source/tool/command/result receipts belong under `evidence/`.

## Qualification status

The frozen seam4 source passes formatting and strict contract Clippy. Seam3's
native wire tests pass 28/28 and contract-boundary tests 3/3; seam4 changes only an
elided unused lifetime. Five standalone witnesses, Charon/Aeneas extraction and
generated Lean compilation passed on seam3. The generated model is
`f7c597d997b34b800578f39168924a4666426bc6d5a5dfee23bc7313708e2e0f` from
kernel `8ed2b007bb457eed5057054367e2c9e6aac38664bc9e89cddc580d61de3e6bf8`.
Fresh baseline and restoration pass all five Rust witnesses, Charon/Aeneas,
the default Lake build and all six direct handwritten checks. Printed target
axioms are limited to `propext`, `Classical.choice` and `Quot.sound`; no `sorryAx`
or custom unproved facts occur in accepted output. All five independent legal
mutants compile, extract and build their generated module, then fail the intended
unchanged semantic proof and named Rust assertion (exits 1 and 101). The restored
kernel, generated Lean and handwritten proofs recover their exact identities.

The [compact receipt](evidence/authorization-receipt.json) records measured stage
wall times, source/tool/model/proof identities, axiom closures, raw-log hashes and
excluded attempts. Accepted controls come from qualification executions 3 and 6;
execution 4's accidental partial baseline is retained and excluded. Literal/query
controls were rerun with the intended semantic module checked directly after an
incorrect dependency-stage selection; no source/proof repair was made for them.
The first fresh baseline took 575 ms for Rust tests, 279 ms for Charon, 1,019 ms
for Aeneas and 17,511 ms for the default Lake build using cached dependencies.
Final restoration took 189/153/1,026/17,110 ms for those same stages, followed by
six successful direct proof checks. These are per-stage timings, not a cold-tool
bootstrap or a system throughput benchmark.

This evidence applies to the frozen basis above; subsequent SDK changes, including
concurrent extension naming work, require separate source-correspondence review.
This is a behavior-preserving private seam, not a fix for an original authorization
bug. No full SDK gates, deployment, publication or application model call is claimed.
