import Lake
open Lake DSL

require aeneas from git
  "https://github.com/AeneasVerif/aeneas.git" @
  "fdebf20d78e8007bb92fc17068f17fc2e6cfbbbf" / "backends/lean"

package sdkAuthorizationProof
lean_lib SdkAuthorizationProof
@[default_target]
lean_lib ResolutionProof

@[default_target]
lean_lib TemplateProof

@[default_target]
lean_lib PathProof

@[default_target]
lean_lib MatcherProof

@[default_target]
lean_lib AuthorizationProof

@[default_target]
lean_lib ContractInterpretation
