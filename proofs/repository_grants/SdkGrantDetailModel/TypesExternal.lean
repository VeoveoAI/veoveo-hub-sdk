-- Handwritten external primitive model based on the generated interface.
module
public import Aeneas
@[expose] public section
open Aeneas Aeneas.Std

namespace SdkGrantDetailModel

/-- Extensional finite value model. Native tree order, uniqueness, allocation and
borrow/alias correspondence are explicit refinement boundaries. -/
@[rust_type "alloc::collections::btree::set::BTreeSet"]
abbrev alloc.collections.btree.set.BTreeSet (T : Type _) (A : Type _) := Finset T

end SdkGrantDetailModel
