-- Handwritten external primitive models based on generated interfaces.
module
public import Aeneas
public import SdkGrantDetailModel.Types
@[expose] public section
open Aeneas Aeneas.Std

namespace SdkGrantDetailModel
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

namespace Native
/-- Traverse actual supplied element-clone Results; failure/divergence short-circuit.
The finite traversal is a model observation, not a native call-count/order trace. -/
def cloneList {T : Type _} (clone : T → Result T) : List T → Result (List T)
  | [] => .ok []
  | head :: tail => do
    let value ← clone head
    let rest ← cloneList clone tail
    .ok (value :: rest)
end Native

@[rust_fun
  "alloc::collections::btree::set::{core::clone::Clone<alloc::collections::btree::set::BTreeSet<@T, @A>>}::clone"]
def alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone
    {T : Type _} {A : Type _} (element : Aeneas.Std.core.clone.Clone T)
    (allocator : core.alloc.AllocatorClone A)
    (set : alloc.collections.btree.set.BTreeSet T A) :
    Result (alloc.collections.btree.set.BTreeSet T A) := do
  let values ← Native.cloneList element.clone set.toList
  .ok values.toFinset

@[rust_fun "alloc::string::{core::clone::Clone<alloc::string::String>}::clone"]
def alloc.string.String.Insts.CoreCloneClone.clone (value : String) : Result String :=
  .ok value

/-- Standard empty constructor, unused by the baseline generated bodies. Included
as explicit primitive support for the dependency-retaining source fault control. -/
@[rust_fun "alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<@T, alloc::alloc::Global>}::new"]
def alloc.collections.btree.set.BTreeSetTGlobal.new (T : Type _) :
    Result (alloc.collections.btree.set.BTreeSet T Aeneas.Std.alloc.alloc.Global) :=
  .ok ∅

end
end SdkGrantDetailModel
