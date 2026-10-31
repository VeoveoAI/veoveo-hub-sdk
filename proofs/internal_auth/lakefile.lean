import Lake
open Lake DSL

require aeneas from git
  "https://github.com/AeneasVerif/aeneas.git" @
  "fdebf20d78e8007bb92fc17068f17fc2e6cfbbbf" / "backends/lean"

package sdkInternalAuthProof
@[default_target]
lean_lib RequestCompositionSdk
lean_lib InternalAuthModelAudit
lean_lib InternalAuthSpecification
lean_lib InternalAuthValueLaws
lean_lib InternalAuthCorrectness
lean_lib InternalAuthContextProof
