import Lake
open Lake DSL

require aeneas from git
  "https://github.com/AeneasVerif/aeneas.git" @
  "fdebf20d78e8007bb92fc17068f17fc2e6cfbbbf" / "backends/lean"

package sdkRepositoryGrantProof

lean_lib SdkGrantDetailModel where
  roots := #[`SdkGrantDetailModel.TypesExternal, `SdkGrantDetailModel.Types,
    `SdkGrantDetailModel.FunsExternal, `SdkGrantDetailModel.Funs]
lean_lib RepositoryGrantProof
