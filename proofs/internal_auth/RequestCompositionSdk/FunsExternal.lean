-- Handwritten operational primitives; no own SDK validation body is replaced.
module
public import Aeneas
public import RequestCompositionSdk.Types
@[expose] public section
open Aeneas Aeneas.Std
namespace RequestCompositionSdk
noncomputable section
local instance (predicate : Prop) : Decidable predicate := Classical.propDecidable predicate

deriving instance DecidableEq for veoveo_types.provenance.InvocationMode

universe u v w

namespace Native
/-- Explicit native sorted-enumeration correspondence for observable scope text.
This is an input refinement law, not a runtime check or an axiom asserting it. -/
def ScopeTraversalSorted
    (scopes : OrderedSet veoveo_types.names.ScopeName) : Prop :=
  scopes.values.Pairwise (fun left right => left.value < right.value)

/-- Restricted ordinary, non-leap-second correspondence domain. Chrono also
admits leap-second representations; no universal native normalization is assumed. -/
def TimestampNonLeap (value : NativeTimestamp) : Prop :=
  value.nanoseconds.val < 1000000000

/-- Effects and updated predicate state are retained across every visited entry. -/
def find {I : Type u} {P : Type v} {T : Type w}
    (iterator : Aeneas.Std.core.iter.traits.iterator.Iterator I T)
    (predicate : Aeneas.Std.core.ops.function.FnMut P T Bool) (state : I) (test : P) :
    Result (Option T × I) := do
  let (entry, next) ← iterator.next state
  match entry with
  | none => .ok (none, next)
  | some value =>
    let (hit, test') ← predicate.call_mut test value
    if hit then .ok (some value, next) else find iterator predicate next test'
partial_fixpoint

def cloneList {T : Type u} (clone : T → Result T) : List T → Result (List T)
  | [] => .ok []
  | head :: tail => do
    let head' ← clone head
    let tail' ← cloneList clone tail
    .ok (head' :: tail')

def listEq {T : Type u} (equal : T → T → Result Bool) : List T → List T → Result Bool
  | [], [] => .ok true
  | left :: lefts, right :: rights => do
    let same ← equal left right
    if same then listEq equal lefts rights else .ok false
  | _, _ => .ok false

/-- UTF-8 byte representation uses the real pointer-width bound, not a U32 cap. -/
def stringToStr (value : String) : Result Str :=
  let bytes : List U8 := value.toByteArray.toList.map (fun x =>
    ⟨x.toNat, by cases x; simp only [UInt8.toNat_ofBitVec,
      UScalarTy.U8_numBits_eq, Nat.reducePow]; omega⟩)
  if representable : bytes.length ≤ Usize.max
  then .ok (Slice.from bytes representable)
  else .fail .maximumSizeExceeded

/-- Only native-valid UTF-8 is representable; invalid byte slices fail explicitly. -/
def strToString (value : Str) : Result String :=
  let bytes := ByteArray.mk (value.val.toArray.map (fun byte => UInt8.ofNat byte.val))
  match String.fromUTF8? bytes with
  | some text => .ok text
  | none => .fail .panic

def namespacePrefix (value : Str) : Result Str :=
  let namespaceBytes := value.val.takeWhile (fun byte => decide (byte.val ≠ 46))
  if representable : namespaceBytes.length ≤ Usize.max
  then .ok (Slice.from namespaceBytes representable)
  else .fail .maximumSizeExceeded

def instant (value : NativeTimestamp) : Int × Nat :=
  (value.seconds.val, value.nanoseconds.val)

/-- UTC timestamp ordering is lexicographic, retaining every subsecond case. -/
def compareTimestamp (left right : NativeTimestamp) : Ordering :=
  match compare left.seconds.val right.seconds.val with
  | .eq => compare left.nanoseconds.val right.nanoseconds.val
  | ordering => ordering

def writeText (value : String) (formatter : core.fmt.Formatter) := do
  let text ← stringToStr value
  core.fmt.Formatter.write_str formatter text
end Native

/-- [core::iter::traits::iterator::Iterator::find]:
    Source: '/rustc/library/core/src/iter/traits/iterator.rs', lines 2971:4-2974:38
    Name pattern: [core::iter::traits::iterator::Iterator::find]
    Visibility: public -/
@[trait_default, rust_fun "core::iter::traits::iterator::Iterator::find"]
def core.iter.traits.iterator.Iterator.find.default
  {Self : Type _} {P : Type _} {Clause0_Item : Type _} (IteratorInst :
  core.iter.traits.iterator.Iterator Self Clause0_Item)
  (opsfunctionFnMutPTupleShared0Clause0_ItemBoolInst : core.ops.function.FnMut
  P Clause0_Item Bool) :
  Self → P → Result ((Option Clause0_Item) × Self)
  := fun state test => Native.find IteratorInst opsfunctionFnMutPTupleShared0Clause0_ItemBoolInst state test

/-- [core::option::{core::option::Option<T>}::as_ref]:
    Source: '/rustc/library/core/src/option.rs', lines 743:4-743:44
    Name pattern: [core::option::{core::option::Option<@T>}::as_ref]
    Visibility: public -/
@[rust_fun "core::option::{core::option::Option<@T>}::as_ref"]
def core.option.Option.as_ref {T : Type _} : Option T → Result (Option T)
  := fun value => .ok value

/-- [core::option::{core::option::Option<T>}::map_or]:
    Source: '/rustc/library/core/src/option.rs', lines 1224:4-1227:28
    Name pattern: [core::option::{core::option::Option<@T>}::map_or]
    Visibility: public -/
@[rust_fun "core::option::{core::option::Option<@T>}::map_or"]
def core.option.Option.map_or
  {T : Type _} {U : Type _} {F : Type _} (opsfunctionFnOnceFTupleTUInst :
  core.ops.function.FnOnce F T U) :
  Option T → U → F → Result U
  := fun value fallback f => match value with
  | none => .ok fallback
  | some value => opsfunctionFnOnceFTupleTUInst.call_once f value

/-- [core::option::{impl core::clone::Clone for core::option::Option<T>}::clone]:
    Source: '/rustc/library/core/src/option.rs', lines 2278:4-2278:27
    Name pattern: [core::option::{core::clone::Clone<core::option::Option<@T>>}::clone]
    Visibility: public -/
@[rust_fun
  "core::option::{core::clone::Clone<core::option::Option<@T>>}::clone"]
def core.option.Option.Insts.CoreCloneClone.clone
  {T : Type _} (cloneCloneInst : core.clone.Clone T) :
  Option T → Result (Option T)
  := fun value => match value with
  | none => .ok none
  | some value => do let copied ← cloneCloneInst.clone value; .ok (some copied)

/-- [core::option::{impl core::iter::traits::collect::IntoIterator<T, core::option::IntoIter<T>> for core::option::Option<T>}::into_iter]:
    Source: '/rustc/library/core/src/option.rs', lines 2339:4-2339:37
    Name pattern: [core::option::{core::iter::traits::collect::IntoIterator<core::option::Option<@T>, @T, core::option::IntoIter<@T>>}::into_iter]
    Visibility: public -/
@[rust_fun
  "core::option::{core::iter::traits::collect::IntoIterator<core::option::Option<@T>, @T, core::option::IntoIter<@T>>}::into_iter"]
def core.option.Option.Insts.CoreIterTraitsCollectIntoIteratorTIntoIter.into_iter
  {T : Type _} : Option T → Result (core.option.IntoIter T)
  := fun value => .ok value

/-- [core::option::{impl core::cmp::PartialEq<core::option::Option<T>> for core::option::Option<T>}::eq]:
    Source: '/rustc/library/core/src/option.rs', lines 2441:4-2441:38
    Name pattern: [core::option::{core::cmp::PartialEq<core::option::Option<@T>, core::option::Option<@T>>}::eq]
    Visibility: public -/
@[rust_fun
  "core::option::{core::cmp::PartialEq<core::option::Option<@T>, core::option::Option<@T>>}::eq"]
def core.option.Option.Insts.CoreCmpPartialEqOption.eq
  {T : Type _} (cmpPartialEqInst : core.cmp.PartialEq T T) :
  Option T → Option T → Result Bool
  := fun left right => match left, right with
  | none, none => .ok true
  | some left, some right => cmpPartialEqInst.eq left right
  | _, _ => .ok false

/-- [core::option::{impl core::iter::traits::iterator::Iterator<A> for core::option::IntoIter<A>}::next]:
    Source: '/rustc/library/core/src/option.rs', lines 2635:4-2635:35
    Name pattern: [core::option::{core::iter::traits::iterator::Iterator<core::option::IntoIter<@A>, @A>}::next]
    Visibility: public -/
@[rust_fun
  "core::option::{core::iter::traits::iterator::Iterator<core::option::IntoIter<@A>, @A>}::next"]
def core.option.IntoIter.Insts.CoreIterTraitsIteratorIterator.next
  {A : Type _} :
  core.option.IntoIter A → Result ((Option A) × (core.option.IntoIter A))
  := fun value => .ok (value, none)

/-- [core::str::traits::{impl core::cmp::PartialEq<str> for str}::eq]:
    Source: '/rustc/library/core/src/str/traits.rs', lines 29:4-29:37
    Name pattern: [core::str::traits::{core::cmp::PartialEq<str, str>}::eq]
    Visibility: public -/
@[rust_fun "core::str::traits::{core::cmp::PartialEq<str, str>}::eq"]
def Str.Insts.CoreCmpPartialEqStr.eq : Str → Str → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [alloc::collections::btree::set::{impl core::cmp::PartialEq<alloc::collections::btree::set::BTreeSet<T, A>> for alloc::collections::btree::set::BTreeSet<T, A>}::eq]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 94:4-94:48
    Name pattern: [alloc::collections::btree::set::{core::cmp::PartialEq<alloc::collections::btree::set::BTreeSet<@T, @A>, alloc::collections::btree::set::BTreeSet<@T, @A>>}::eq]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{core::cmp::PartialEq<alloc::collections::btree::set::BTreeSet<@T, @A>, alloc::collections::btree::set::BTreeSet<@T, @A>>}::eq"]
def alloc.collections.btree.set.BTreeSet.Insts.CoreCmpPartialEqBTreeSet.eq
  {T : Type _} {A : Type _} (corecmpPartialEqInst : core.cmp.PartialEq T T)
  (coreallocAllocatorCloneInst : core.alloc.AllocatorClone A) :
  alloc.collections.btree.set.BTreeSet T A →
    alloc.collections.btree.set.BTreeSet T A → Result Bool
  := fun left right =>
  if left.values.length = right.values.length
  then Native.listEq corecmpPartialEqInst.eq left.values right.values
  else .ok false

/-- [alloc::collections::btree::set::{impl core::clone::Clone for alloc::collections::btree::set::BTreeSet<T, A>}::clone]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 118:4-118:27
    Name pattern: [alloc::collections::btree::set::{core::clone::Clone<alloc::collections::btree::set::BTreeSet<@T, @A>>}::clone]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{core::clone::Clone<alloc::collections::btree::set::BTreeSet<@T, @A>>}::clone"]
def alloc.collections.btree.set.BTreeSet.Insts.CoreCloneClone.clone
  {T : Type _} {A : Type _} (corecloneCloneInst : core.clone.Clone T)
  (coreallocAllocatorCloneInst : core.alloc.AllocatorClone A) :
  alloc.collections.btree.set.BTreeSet T A → Result
    (alloc.collections.btree.set.BTreeSet T A)
  := fun value => do
  let values ← Native.cloneList corecloneCloneInst.clone value.values
  .ok { values }

/-- [alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<T, A>}::iter]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 1245:4-1245:37
    Name pattern: [alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<@T, @A>}::iter]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<@T, @A>}::iter"]
def alloc.collections.btree.set.BTreeSet.iter
  {T : Type _} {A : Type _} (coreallocAllocatorCloneInst :
  core.alloc.AllocatorClone A) :
  alloc.collections.btree.set.BTreeSet T A → Result
    (alloc.collections.btree.set.Iter T)
  := fun value => .ok value.values

/-- [alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<T, A>}::is_empty]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 1292:4-1292:40
    Name pattern: [alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<@T, @A>}::is_empty]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{alloc::collections::btree::set::BTreeSet<@T, @A>}::is_empty"]
def alloc.collections.btree.set.BTreeSet.is_empty
  {T : Type _} {A : Type _} (coreallocAllocatorCloneInst :
  core.alloc.AllocatorClone A) :
  alloc.collections.btree.set.BTreeSet T A → Result Bool
  := fun value => .ok value.values.isEmpty

/-- [alloc::collections::btree::set::{impl core::iter::traits::collect::IntoIterator<&'a T, alloc::collections::btree::set::Iter<'a, T>> for &'a alloc::collections::btree::set::BTreeSet<T, A>}::into_iter]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 1546:4-1546:37
    Name pattern: [alloc::collections::btree::set::{core::iter::traits::collect::IntoIterator<&'a alloc::collections::btree::set::BTreeSet<@T, @A>, &'a @T, alloc::collections::btree::set::Iter<'a, @T>>}::into_iter]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{core::iter::traits::collect::IntoIterator<&'a alloc::collections::btree::set::BTreeSet<@T, @A>, &'a @T, alloc::collections::btree::set::Iter<'a, @T>>}::into_iter"]
def SharedABTreeSet.Insts.CoreIterTraitsCollectIntoIteratorSharedATIter.into_iter
  {T : Type _} {A : Type _} (coreallocAllocatorCloneInst :
  core.alloc.AllocatorClone A) :
  alloc.collections.btree.set.BTreeSet T A → Result
    (alloc.collections.btree.set.Iter T)
  := fun value => .ok value.values

/-- [alloc::collections::btree::set::{impl core::iter::traits::iterator::Iterator<&'a T> for alloc::collections::btree::set::Iter<'a, T>}::next]:
    Source: '/rustc/library/alloc/src/collections/btree/set.rs', lines 1765:4-1765:39
    Name pattern: [alloc::collections::btree::set::{core::iter::traits::iterator::Iterator<alloc::collections::btree::set::Iter<'a, @T>, &'a @T>}::next]
    Visibility: public -/
@[rust_fun
  "alloc::collections::btree::set::{core::iter::traits::iterator::Iterator<alloc::collections::btree::set::Iter<'a, @T>, &'a @T>}::next"]
def alloc.collections.btree.set.Iter.Insts.CoreIterTraitsIteratorIteratorSharedAT.next
  {T : Type _} :
  alloc.collections.btree.set.Iter T → Result ((Option T) ×
    (alloc.collections.btree.set.Iter T))
  := fun value => match value with
  | [] => .ok (none, [])
  | head :: tail => .ok (some head, tail)

/-- [alloc::str::{impl alloc::borrow::ToOwned<alloc::string::String> for str}::to_owned]:
    Source: '/rustc/library/alloc/src/str.rs', lines 252:4-252:32
    Name pattern: [alloc::str::{alloc::borrow::ToOwned<str, alloc::string::String>}::to_owned]
    Visibility: public -/
@[rust_fun
  "alloc::str::{alloc::borrow::ToOwned<str, alloc::string::String>}::to_owned"]
def Str.Insts.AllocBorrowToOwnedString.to_owned : Str → Result String
  := Native.strToString

/-- [alloc::string::{alloc::string::String}::new]:
    Source: '/rustc/library/alloc/src/string.rs', lines 446:4-446:32
    Name pattern: [alloc::string::{alloc::string::String}::new]
    Visibility: public -/
@[rust_fun "alloc::string::{alloc::string::String}::new"]
def alloc.string.String.new : Result String
  := .ok ""

/-- [alloc::string::{alloc::string::String}::push_str]:
    Source: '/rustc/library/alloc/src/string.rs', lines 1122:4-1122:44
    Name pattern: [alloc::string::{alloc::string::String}::push_str]
    Visibility: public -/
@[rust_fun "alloc::string::{alloc::string::String}::push_str"]
def alloc.string.String.push_str : String → Str → Result String
  := fun value fragment => do let text ← Native.strToString fragment; .ok (value ++ text)

/-- [alloc::string::{alloc::string::String}::push]:
    Source: '/rustc/library/alloc/src/string.rs', lines 1451:4-1451:36
    Name pattern: [alloc::string::{alloc::string::String}::push]
    Visibility: public -/
@[rust_fun "alloc::string::{alloc::string::String}::push"]
def alloc.string.String.push : String → Char → Result String
  := fun value char => .ok (value.push char)

/-- [alloc::string::{impl core::clone::Clone for alloc::string::String}::clone]:
    Source: '/rustc/library/alloc/src/string.rs', lines 2425:4-2425:27
    Name pattern: [alloc::string::{core::clone::Clone<alloc::string::String>}::clone]
    Visibility: public -/
@[rust_fun "alloc::string::{core::clone::Clone<alloc::string::String>}::clone"]
def alloc.string.String.Insts.CoreCloneClone.clone : String → Result String
  := fun value => .ok value

/-- [alloc::string::{impl core::cmp::PartialEq<alloc::string::String> for &'_0 str}::ne]:
    Source: '/rustc/library/alloc/src/string.rs', lines 2756:12-2756:46; this code is generated from a macro invocation at: '/rustc/library/alloc/src/string.rs', lines 2764:0-2764:25
    Name pattern: [alloc::string::{core::cmp::PartialEq<&'0 str, alloc::string::String>}::ne]
    Visibility: public -/
@[rust_fun
  "alloc::string::{core::cmp::PartialEq<&'0 str, alloc::string::String>}::ne"]
def Shared0Str.Insts.CoreCmpPartialEqString.ne
  : Str → String → Result Bool
  := fun value right => do let left ← Native.strToString value; .ok (decide (left ≠ right))

/-- [alloc::string::{impl alloc::string::ToString for T}::to_string]:
    Source: '/rustc/library/alloc/src/string.rs', lines 2965:4-2965:33
    Name pattern: [alloc::string::{alloc::string::ToString<@T>}::to_string]
    Visibility: public -/
@[rust_fun "alloc::string::{alloc::string::ToString<@T>}::to_string"]
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom alloc.string.ToString.Blanket.to_string
  {T : Type _} (corefmtDisplayInst : core.fmt.Display T) : T → Result String

/-- [alloc::string::{impl core::convert::AsRef<str> for alloc::string::String}::as_ref]:
    Source: '/rustc/library/alloc/src/string.rs', lines 3157:4-3157:28
    Name pattern: [alloc::string::{core::convert::AsRef<alloc::string::String, str>}::as_ref]
    Visibility: public -/
@[rust_fun
  "alloc::string::{core::convert::AsRef<alloc::string::String, str>}::as_ref"]
def alloc.string.String.Insts.CoreConvertAsRefStr.as_ref
  : String → Result Str
  := Native.stringToStr

/-- [alloc::vec::{alloc::vec::Vec<T>}::is_empty]:
    Source: '/rustc/library/alloc/src/vec/mod.rs', lines 3160:4-3160:40
    Name pattern: [alloc::vec::{alloc::vec::Vec<@T>}::is_empty]
    Visibility: public -/
@[rust_fun "alloc::vec::{alloc::vec::Vec<@T>}::is_empty"]
def alloc.vec.Vec.is_empty
  {T : Type _} (A : Type _) : alloc.vec.Vec T → Result Bool
  := fun value => .ok value.val.isEmpty

/-- [alloc::vec::{impl core::iter::traits::collect::IntoIterator<&'a T, core::slice::iter::Iter<'a, T>> for &'a alloc::vec::Vec<T>}::into_iter]:
    Source: '/rustc/library/alloc/src/vec/mod.rs', lines 4088:4-4088:40
    Name pattern: [alloc::vec::{core::iter::traits::collect::IntoIterator<&'a alloc::vec::Vec<@T>, &'a @T, core::slice::iter::Iter<'a, @T>>}::into_iter]
    Visibility: public -/
@[rust_fun
  "alloc::vec::{core::iter::traits::collect::IntoIterator<&'a alloc::vec::Vec<@T>, &'a @T, core::slice::iter::Iter<'a, @T>>}::into_iter"]
def SharedAVec.Insts.CoreIterTraitsCollectIntoIteratorSharedATIter.into_iter
  {T : Type _} (A : Type _) :
  alloc.vec.Vec T → Result (core.slice.iter.Iter T)
  := fun value => Aeneas.Std.core.slice.Slice.iter (Aeneas.Std.alloc.vec.Vec.deref value)

/-- [chrono::datetime::{impl core::clone::Clone for chrono::datetime::DateTime<Tz, Clause1_Offset>}::clone]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 49:9-49:14
    Name pattern: [chrono::datetime::{core::clone::Clone<chrono::datetime::DateTime<@Tz, @Clause1_Offset>>}::clone]
    Visibility: public -/
@[rust_fun
  "chrono::datetime::{core::clone::Clone<chrono::datetime::DateTime<@Tz, @Clause1_Offset>>}::clone"]
def chrono.datetime.DateTime.Insts.CoreCloneClone.clone
  {Tz : Type _} {Clause1_Offset : Type _} (corecloneCloneInst :
  core.clone.Clone Tz) (offsetTimeZoneInst : chrono.offset.TimeZone Tz
  Clause1_Offset) (corecloneCloneInst1 : core.clone.Clone Clause1_Offset) :
  chrono.datetime.DateTime Tz Clause1_Offset → Result
    (chrono.datetime.DateTime Tz Clause1_Offset)
  := fun value => .ok value

/-- [chrono::datetime::{chrono::datetime::DateTime<Tz, Clause0_Offset>}::timestamp]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 200:4-200:40
    Name pattern: [chrono::datetime::{chrono::datetime::DateTime<@Tz, @Clause0_Offset>}::timestamp]
    Visibility: public -/
@[rust_fun
  "chrono::datetime::{chrono::datetime::DateTime<@Tz, @Clause0_Offset>}::timestamp"]
def chrono.datetime.DateTime.timestamp
  {Tz : Type _} {Clause0_Offset : Type _} (offsetTimeZoneInst :
  chrono.offset.TimeZone Tz Clause0_Offset) :
  chrono.datetime.DateTime Tz Clause0_Offset → Result Std.I64
  := fun value => .ok value.seconds

/-- [chrono::datetime::{impl core::cmp::PartialEq<chrono::datetime::DateTime<Tz2, Clause1_Offset>> for chrono::datetime::DateTime<Tz, Clause0_Offset>}::eq]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 1473:4-1473:47
    Name pattern: [chrono::datetime::{core::cmp::PartialEq<chrono::datetime::DateTime<@Tz, @Clause0_Offset>, chrono::datetime::DateTime<@Tz2, @Clause1_Offset>>}::eq]
    Visibility: public -/
@[rust_fun
  "chrono::datetime::{core::cmp::PartialEq<chrono::datetime::DateTime<@Tz, @Clause0_Offset>, chrono::datetime::DateTime<@Tz2, @Clause1_Offset>>}::eq"]
def chrono.datetime.DateTime.Insts.CoreCmpPartialEqDateTime.eq
  {Tz : Type _} {Tz2 : Type _} {Clause0_Offset : Type _} {Clause1_Offset :
  Type _} (offsetTimeZoneInst : chrono.offset.TimeZone Tz Clause0_Offset)
  (offsetTimeZoneInst1 : chrono.offset.TimeZone Tz2 Clause1_Offset) :
  chrono.datetime.DateTime Tz Clause0_Offset → chrono.datetime.DateTime Tz2
    Clause1_Offset → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [chrono::datetime::{impl core::cmp::PartialOrd<chrono::datetime::DateTime<Tz2, Clause1_Offset>> for chrono::datetime::DateTime<Tz, Clause0_Offset>}::partial_cmp]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 1502:4-1502:68
    Name pattern: [chrono::datetime::{core::cmp::PartialOrd<chrono::datetime::DateTime<@Tz, @Clause0_Offset>, chrono::datetime::DateTime<@Tz2, @Clause1_Offset>>}::partial_cmp]
    Visibility: public -/
@[rust_fun
  "chrono::datetime::{core::cmp::PartialOrd<chrono::datetime::DateTime<@Tz, @Clause0_Offset>, chrono::datetime::DateTime<@Tz2, @Clause1_Offset>>}::partial_cmp"]
def chrono.datetime.DateTime.Insts.CoreCmpPartialOrdDateTime.partial_cmp
  {Tz : Type _} {Tz2 : Type _} {Clause0_Offset : Type _} {Clause1_Offset :
  Type _} (offsetTimeZoneInst : chrono.offset.TimeZone Tz Clause0_Offset)
  (offsetTimeZoneInst1 : chrono.offset.TimeZone Tz2 Clause1_Offset) :
  chrono.datetime.DateTime Tz Clause0_Offset → chrono.datetime.DateTime Tz2
    Clause1_Offset → Result (Option Ordering)
  := fun left right => .ok (some (Native.compareTimestamp left right))

/-- [chrono::datetime::{impl core::cmp::Ord for chrono::datetime::DateTime<Tz, Clause0_Offset>}::cmp]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/datetime/mod.rs', lines 1508:4-1508:51
    Name pattern: [chrono::datetime::{core::cmp::Ord<chrono::datetime::DateTime<@Tz, @Clause0_Offset>>}::cmp]
    Visibility: public -/
@[rust_fun
  "chrono::datetime::{core::cmp::Ord<chrono::datetime::DateTime<@Tz, @Clause0_Offset>>}::cmp"]
def chrono.datetime.DateTime.Insts.CoreCmpOrd.cmp
  {Tz : Type _} {Clause0_Offset : Type _} (offsetTimeZoneInst :
  chrono.offset.TimeZone Tz Clause0_Offset) :
  chrono.datetime.DateTime Tz Clause0_Offset → chrono.datetime.DateTime Tz
    Clause0_Offset → Result Ordering
  := fun left right => .ok (Native.compareTimestamp left right)

/-- [chrono::offset::utc::{impl core::clone::Clone for chrono::offset::utc::Utc}::clone]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 43:15-43:20
    Name pattern: [chrono::offset::utc::{core::clone::Clone<chrono::offset::utc::Utc>}::clone]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{core::clone::Clone<chrono::offset::utc::Utc>}::clone"]
def chrono.offset.utc.Utc.Insts.CoreCloneClone.clone
  : chrono.offset.utc.Utc → Result chrono.offset.utc.Utc
  := fun value => .ok value

/-- [chrono::offset::utc::{chrono::offset::utc::Utc}::now]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 95:4-95:33
    Name pattern: [chrono::offset::utc::{chrono::offset::utc::Utc}::now]
    Visibility: public -/
@[rust_fun "chrono::offset::utc::{chrono::offset::utc::Utc}::now"]
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom chrono.offset.utc.Utc.now
  :
  Result (chrono.datetime.DateTime chrono.offset.utc.Utc chrono.offset.utc.Utc)

/-- [chrono::offset::utc::{impl core::fmt::Debug for chrono::offset::utc::Utc}::fmt]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 143:4-143:56
    Name pattern: [chrono::offset::utc::{core::fmt::Debug<chrono::offset::utc::Utc>}::fmt]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{core::fmt::Debug<chrono::offset::utc::Utc>}::fmt"]
def chrono.offset.utc.Utc.Insts.CoreFmtDebug.fmt
  :
  chrono.offset.utc.Utc → core.fmt.Formatter → Result ((core.result.Result
    Unit core.fmt.Error) × core.fmt.Formatter)
  := fun _ formatter => Native.writeText "Z" formatter

/-- [chrono::offset::utc::{impl chrono::offset::Offset for chrono::offset::utc::Utc}::fix]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 137:4-137:32
    Name pattern: [chrono::offset::utc::{chrono::offset::Offset<chrono::offset::utc::Utc>}::fix]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::Offset<chrono::offset::utc::Utc>}::fix"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetOffset.fix
  : chrono.offset.utc.Utc → Result chrono.offset.fixed.FixedOffset
  := fun _ => .ok ()

/-- [chrono::offset::utc::{impl chrono::offset::TimeZone<chrono::offset::utc::Utc> for chrono::offset::utc::Utc}::offset_from_utc_datetime]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 131:4-131:67
    Name pattern: [chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_utc_datetime]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_utc_datetime"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc.offset_from_utc_datetime
  :
  chrono.offset.utc.Utc → chrono.naive.datetime.NaiveDateTime → Result
    chrono.offset.utc.Utc
  := fun _ _ => .ok ()

/-- [chrono::offset::utc::{impl chrono::offset::TimeZone<chrono::offset::utc::Utc> for chrono::offset::utc::Utc}::offset_from_utc_date]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 128:4-128:59
    Name pattern: [chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_utc_date]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_utc_date"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc.offset_from_utc_date
  :
  chrono.offset.utc.Utc → chrono.naive.date.NaiveDate → Result
    chrono.offset.utc.Utc
  := fun _ _ => .ok ()

/-- [chrono::offset::utc::{impl chrono::offset::TimeZone<chrono::offset::utc::Utc> for chrono::offset::utc::Utc}::offset_from_local_datetime]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 124:4-124:88
    Name pattern: [chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_local_datetime]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_local_datetime"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc.offset_from_local_datetime
  :
  chrono.offset.utc.Utc → chrono.naive.datetime.NaiveDateTime → Result
    (chrono.offset.LocalResult chrono.offset.utc.Utc)
  := fun _ _ => .ok (.Single ())

/-- [chrono::offset::utc::{impl chrono::offset::TimeZone<chrono::offset::utc::Utc> for chrono::offset::utc::Utc}::offset_from_local_date]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 121:4-121:80
    Name pattern: [chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_local_date]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::offset_from_local_date"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc.offset_from_local_date
  :
  chrono.offset.utc.Utc → chrono.naive.date.NaiveDate → Result
    (chrono.offset.LocalResult chrono.offset.utc.Utc)
  := fun _ _ => .ok (.Single ())

/-- [chrono::offset::utc::{impl chrono::offset::TimeZone<chrono::offset::utc::Utc> for chrono::offset::utc::Utc}::from_offset]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/chrono-0.4.45/src/offset/utc.rs', lines 117:4-117:39
    Name pattern: [chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::from_offset]
    Visibility: public -/
@[rust_fun
  "chrono::offset::utc::{chrono::offset::TimeZone<chrono::offset::utc::Utc, chrono::offset::utc::Utc>}::from_offset"]
def chrono.offset.utc.Utc.Insts.ChronoOffsetTimeZoneUtc.from_offset
  : chrono.offset.utc.Utc → Result chrono.offset.utc.Utc
  := fun value => .ok value

/-- [url::{url::Url}::as_str]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/lib.rs', lines 653:4-653:32
    Name pattern: [url::{url::Url}::as_str]
    Visibility: public -/
@[rust_fun "url::{url::Url}::as_str"]
def url.Url.as_str : url.Url → Result Str
  := fun value => Native.stringToStr value.value

/-- [uuid::fmt::{impl core::fmt::Display for uuid::Uuid}::fmt]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/uuid-1.25.0/src/fmt.rs', lines 32:4-32:60
    Name pattern: [uuid::fmt::{core::fmt::Display<uuid::Uuid>}::fmt]
    Visibility: public -/
@[rust_fun "uuid::fmt::{core::fmt::Display<uuid::Uuid>}::fmt"]
def uuid.Uuid.Insts.CoreFmtDisplay.fmt
  :
  uuid.Uuid → core.fmt.Formatter → Result ((core.result.Result Unit
    core.fmt.Error) × core.fmt.Formatter)
  := fun value formatter => Native.writeText value.value formatter

/-- [uuid::v7::{uuid::Uuid}::now_v7]:
    Source: '/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/uuid-1.25.0/src/v7.rs', lines 17:4-17:27
    Name pattern: [uuid::v7::{uuid::Uuid}::now_v7]
    Visibility: public -/
@[rust_fun "uuid::v7::{uuid::Uuid}::now_v7"]
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom uuid.v7.Uuid.now_v7 : Result uuid.Uuid

/-- [veoveo_audit_contract::model::{impl core::clone::Clone for veoveo_audit_contract::model::AuditRequest}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/audit/contract/src/model.rs', lines 60:16-60:21
    Name pattern: [veoveo_audit_contract::model::{core::clone::Clone<veoveo_audit_contract::model::AuditRequest>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_audit_contract::model::{core::clone::Clone<veoveo_audit_contract::model::AuditRequest>}::clone"]
def veoveo_audit_contract.model.AuditRequest.Insts.CoreCloneClone.clone
  :
  veoveo_audit_contract.model.AuditRequest → Result
    veoveo_audit_contract.model.AuditRequest
  := fun value => .ok value

/-- [veoveo_types::authority::{impl core::clone::Clone for veoveo_types::authority::InvocationAuthority}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/authority.rs', lines 72:16-72:21
    Name pattern: [veoveo_types::authority::{core::clone::Clone<veoveo_types::authority::InvocationAuthority>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::authority::{core::clone::Clone<veoveo_types::authority::InvocationAuthority>}::clone"]
def veoveo_types.authority.InvocationAuthority.Insts.CoreCloneClone.clone
  :
  veoveo_types.authority.InvocationAuthority → Result
    veoveo_types.authority.InvocationAuthority
  := fun value => .ok value

/-- [veoveo_types::digest::{impl core::clone::Clone for veoveo_types::digest::Sha256Digest}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/digest.rs', lines 9:0-9:51
    Name pattern: [veoveo_types::digest::{core::clone::Clone<veoveo_types::digest::Sha256Digest>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::digest::{core::clone::Clone<veoveo_types::digest::Sha256Digest>}::clone"]
def veoveo_types.digest.Sha256Digest.Insts.CoreCloneClone.clone
  :
  veoveo_types.digest.Sha256Digest → Result veoveo_types.digest.Sha256Digest
  := fun value => .ok value

/-- [veoveo_types::execution::{impl core::cmp::PartialEq<veoveo_types::execution::DeploymentId> for veoveo_types::execution::DeploymentId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/execution.rs', lines 18:0-18:46
    Name pattern: [veoveo_types::execution::{core::cmp::PartialEq<veoveo_types::execution::DeploymentId, veoveo_types::execution::DeploymentId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::execution::{core::cmp::PartialEq<veoveo_types::execution::DeploymentId, veoveo_types::execution::DeploymentId>}::eq"]
def veoveo_types.execution.DeploymentId.Insts.CoreCmpPartialEqDeploymentId.eq
  :
  veoveo_types.execution.DeploymentId → veoveo_types.execution.DeploymentId
    → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::execution::{impl core::clone::Clone for veoveo_types::execution::DeploymentId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/execution.rs', lines 18:0-18:46
    Name pattern: [veoveo_types::execution::{core::clone::Clone<veoveo_types::execution::DeploymentId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::execution::{core::clone::Clone<veoveo_types::execution::DeploymentId>}::clone"]
def veoveo_types.execution.DeploymentId.Insts.CoreCloneClone.clone
  :
  veoveo_types.execution.DeploymentId → Result
    veoveo_types.execution.DeploymentId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::DataLabelId> for veoveo_types::identity::DataLabelId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 5:0-5:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::DataLabelId, veoveo_types::identity::DataLabelId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::DataLabelId, veoveo_types::identity::DataLabelId>}::eq"]
def veoveo_types.identity.DataLabelId.Insts.CoreCmpPartialEqDataLabelId.eq
  :
  veoveo_types.identity.DataLabelId → veoveo_types.identity.DataLabelId →
    Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::DataLabelId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 5:0-5:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::DataLabelId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::DataLabelId>}::clone"]
def veoveo_types.identity.DataLabelId.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.DataLabelId → Result
    veoveo_types.identity.DataLabelId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::PrincipalId> for veoveo_types::identity::PrincipalId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 9:0-9:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::PrincipalId, veoveo_types::identity::PrincipalId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::PrincipalId, veoveo_types::identity::PrincipalId>}::eq"]
def veoveo_types.identity.PrincipalId.Insts.CoreCmpPartialEqPrincipalId.eq
  :
  veoveo_types.identity.PrincipalId → veoveo_types.identity.PrincipalId →
    Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::PrincipalId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 9:0-9:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::PrincipalId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::PrincipalId>}::clone"]
def veoveo_types.identity.PrincipalId.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.PrincipalId → Result
    veoveo_types.identity.PrincipalId
  := fun value => .ok value

/-- [veoveo_types::identity::{veoveo_types::identity::PrincipalId}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 9:0-9:69
    Name pattern: [veoveo_types::identity::{veoveo_types::identity::PrincipalId}::as_str]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{veoveo_types::identity::PrincipalId}::as_str"]
def veoveo_types.identity.PrincipalId.as_str
  : veoveo_types.identity.PrincipalId → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::TenantId> for veoveo_types::identity::TenantId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 13:0-13:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TenantId, veoveo_types::identity::TenantId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TenantId, veoveo_types::identity::TenantId>}::eq"]
def veoveo_types.identity.TenantId.Insts.CoreCmpPartialEqTenantId.eq
  :
  veoveo_types.identity.TenantId → veoveo_types.identity.TenantId → Result
    Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::TenantId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 13:0-13:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TenantId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TenantId>}::clone"]
def veoveo_types.identity.TenantId.Insts.CoreCloneClone.clone
  : veoveo_types.identity.TenantId → Result veoveo_types.identity.TenantId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::WorkContextId> for veoveo_types::identity::WorkContextId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 17:0-17:66
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::WorkContextId, veoveo_types::identity::WorkContextId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::WorkContextId, veoveo_types::identity::WorkContextId>}::eq"]
def veoveo_types.identity.WorkContextId.Insts.CoreCmpPartialEqWorkContextId.eq
  :
  veoveo_types.identity.WorkContextId → veoveo_types.identity.WorkContextId
    → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::WorkContextId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 17:0-17:66
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::WorkContextId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::WorkContextId>}::clone"]
def veoveo_types.identity.WorkContextId.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.WorkContextId → Result
    veoveo_types.identity.WorkContextId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::DelegationId> for veoveo_types::identity::DelegationId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 21:0-21:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::DelegationId, veoveo_types::identity::DelegationId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::DelegationId, veoveo_types::identity::DelegationId>}::eq"]
def veoveo_types.identity.DelegationId.Insts.CoreCmpPartialEqDelegationId.eq
  :
  veoveo_types.identity.DelegationId → veoveo_types.identity.DelegationId →
    Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::DelegationId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 21:0-21:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::DelegationId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::DelegationId>}::clone"]
def veoveo_types.identity.DelegationId.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.DelegationId → Result
    veoveo_types.identity.DelegationId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::GroupId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 25:0-25:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::GroupId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::GroupId>}::clone"]
def veoveo_types.identity.GroupId.Insts.CoreCloneClone.clone
  : veoveo_types.identity.GroupId → Result veoveo_types.identity.GroupId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::GroupId> for veoveo_types::identity::GroupId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 25:0-25:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::GroupId, veoveo_types::identity::GroupId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::GroupId, veoveo_types::identity::GroupId>}::eq"]
def veoveo_types.identity.GroupId.Insts.CoreCmpPartialEqGroupId.eq
  :
  veoveo_types.identity.GroupId → veoveo_types.identity.GroupId → Result
    Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::RoleId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 29:0-29:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::RoleId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::RoleId>}::clone"]
def veoveo_types.identity.RoleId.Insts.CoreCloneClone.clone
  : veoveo_types.identity.RoleId → Result veoveo_types.identity.RoleId
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::RoleId> for veoveo_types::identity::RoleId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 29:0-29:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::RoleId, veoveo_types::identity::RoleId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::RoleId, veoveo_types::identity::RoleId>}::eq"]
def veoveo_types.identity.RoleId.Insts.CoreCmpPartialEqRoleId.eq
  :
  veoveo_types.identity.RoleId → veoveo_types.identity.RoleId → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::TokenIssuer> for veoveo_types::identity::TokenIssuer}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 114:0-114:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TokenIssuer, veoveo_types::identity::TokenIssuer>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TokenIssuer, veoveo_types::identity::TokenIssuer>}::eq"]
def veoveo_types.identity.TokenIssuer.Insts.CoreCmpPartialEqTokenIssuer.eq
  :
  veoveo_types.identity.TokenIssuer → veoveo_types.identity.TokenIssuer →
    Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::TokenIssuer}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 114:0-114:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TokenIssuer>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TokenIssuer>}::clone"]
def veoveo_types.identity.TokenIssuer.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.TokenIssuer → Result
    veoveo_types.identity.TokenIssuer
  := fun value => .ok value

/-- [veoveo_types::identity::{impl core::cmp::PartialEq<veoveo_types::identity::TokenSubject> for veoveo_types::identity::TokenSubject}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 117:0-117:69
    Name pattern: [veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TokenSubject, veoveo_types::identity::TokenSubject>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::cmp::PartialEq<veoveo_types::identity::TokenSubject, veoveo_types::identity::TokenSubject>}::eq"]
def veoveo_types.identity.TokenSubject.Insts.CoreCmpPartialEqTokenSubject.eq
  :
  veoveo_types.identity.TokenSubject → veoveo_types.identity.TokenSubject →
    Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::identity::{veoveo_types::identity::TokenSubject}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 117:0-117:69
    Name pattern: [veoveo_types::identity::{veoveo_types::identity::TokenSubject}::as_str]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{veoveo_types::identity::TokenSubject}::as_str"]
def veoveo_types.identity.TokenSubject.as_str
  : veoveo_types.identity.TokenSubject → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::identity::{impl core::clone::Clone for veoveo_types::identity::TokenSubject}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/identity.rs', lines 117:0-117:69
    Name pattern: [veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TokenSubject>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::identity::{core::clone::Clone<veoveo_types::identity::TokenSubject>}::clone"]
def veoveo_types.identity.TokenSubject.Insts.CoreCloneClone.clone
  :
  veoveo_types.identity.TokenSubject → Result
    veoveo_types.identity.TokenSubject
  := fun value => .ok value

/-- [veoveo_types::names::{impl core::cmp::PartialEq<veoveo_types::names::ScopeName> for veoveo_types::names::ScopeName}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/names.rs', lines 4:0-4:37
    Name pattern: [veoveo_types::names::{core::cmp::PartialEq<veoveo_types::names::ScopeName, veoveo_types::names::ScopeName>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::names::{core::cmp::PartialEq<veoveo_types::names::ScopeName, veoveo_types::names::ScopeName>}::eq"]
def veoveo_types.names.ScopeName.Insts.CoreCmpPartialEqScopeName.eq
  :
  veoveo_types.names.ScopeName → veoveo_types.names.ScopeName → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::names::{impl core::clone::Clone for veoveo_types::names::ScopeName}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/names.rs', lines 4:0-4:37
    Name pattern: [veoveo_types::names::{core::clone::Clone<veoveo_types::names::ScopeName>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::names::{core::clone::Clone<veoveo_types::names::ScopeName>}::clone"]
def veoveo_types.names.ScopeName.Insts.CoreCloneClone.clone
  : veoveo_types.names.ScopeName → Result veoveo_types.names.ScopeName
  := fun value => .ok value

/-- [veoveo_types::names::{veoveo_types::names::ScopeName}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/names.rs', lines 4:0-4:37
    Name pattern: [veoveo_types::names::{veoveo_types::names::ScopeName}::as_str]
    Visibility: public -/
@[rust_fun "veoveo_types::names::{veoveo_types::names::ScopeName}::as_str"]
def veoveo_types.names.ScopeName.as_str
  : veoveo_types.names.ScopeName → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::permission_names::{impl core::clone::Clone for veoveo_types::permission_names::PermissionName}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/permission_names.rs', lines 6:0-6:42
    Name pattern: [veoveo_types::permission_names::{core::clone::Clone<veoveo_types::permission_names::PermissionName>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::permission_names::{core::clone::Clone<veoveo_types::permission_names::PermissionName>}::clone"]
def veoveo_types.permission_names.PermissionName.Insts.CoreCloneClone.clone
  :
  veoveo_types.permission_names.PermissionName → Result
    veoveo_types.permission_names.PermissionName
  := fun value => .ok value

/-- [veoveo_types::permission_names::{veoveo_types::permission_names::PermissionName}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/permission_names.rs', lines 6:0-6:42
    Name pattern: [veoveo_types::permission_names::{veoveo_types::permission_names::PermissionName}::as_str]
    Visibility: public -/
@[rust_fun
  "veoveo_types::permission_names::{veoveo_types::permission_names::PermissionName}::as_str"]
def veoveo_types.permission_names.PermissionName.as_str
  : veoveo_types.permission_names.PermissionName → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::permission_names::namespace]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/permission_names.rs', lines 51:0-51:33
    Name pattern: [veoveo_types::permission_names::namespace] -/
@[rust_fun "veoveo_types::permission_names::namespace"]
def veoveo_types.permission_names.namespace : Str → Result Str
  := Native.namespacePrefix

/-- [veoveo_types::platform_names::{veoveo_types::platform_names::ServerSlug}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 5:0-5:38
    Name pattern: [veoveo_types::platform_names::{veoveo_types::platform_names::ServerSlug}::as_str]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{veoveo_types::platform_names::ServerSlug}::as_str"]
def veoveo_types.platform_names.ServerSlug.as_str
  : veoveo_types.platform_names.ServerSlug → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::platform_names::{impl core::cmp::PartialEq<veoveo_types::platform_names::ServerSlug> for veoveo_types::platform_names::ServerSlug}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 5:0-5:38
    Name pattern: [veoveo_types::platform_names::{core::cmp::PartialEq<veoveo_types::platform_names::ServerSlug, veoveo_types::platform_names::ServerSlug>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{core::cmp::PartialEq<veoveo_types::platform_names::ServerSlug, veoveo_types::platform_names::ServerSlug>}::eq"]
def veoveo_types.platform_names.ServerSlug.Insts.CoreCmpPartialEqServerSlug.eq
  :
  veoveo_types.platform_names.ServerSlug →
    veoveo_types.platform_names.ServerSlug → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::platform_names::{impl core::clone::Clone for veoveo_types::platform_names::ServerSlug}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 5:0-5:38
    Name pattern: [veoveo_types::platform_names::{core::clone::Clone<veoveo_types::platform_names::ServerSlug>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{core::clone::Clone<veoveo_types::platform_names::ServerSlug>}::clone"]
def veoveo_types.platform_names.ServerSlug.Insts.CoreCloneClone.clone
  :
  veoveo_types.platform_names.ServerSlug → Result
    veoveo_types.platform_names.ServerSlug
  := fun value => .ok value

/-- [veoveo_types::platform_names::{impl core::cmp::PartialEq<veoveo_types::platform_names::OAuthClientId> for veoveo_types::platform_names::OAuthClientId}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 39:0-39:36
    Name pattern: [veoveo_types::platform_names::{core::cmp::PartialEq<veoveo_types::platform_names::OAuthClientId, veoveo_types::platform_names::OAuthClientId>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{core::cmp::PartialEq<veoveo_types::platform_names::OAuthClientId, veoveo_types::platform_names::OAuthClientId>}::eq"]
def veoveo_types.platform_names.OAuthClientId.Insts.CoreCmpPartialEqOAuthClientId.eq
  :
  veoveo_types.platform_names.OAuthClientId →
    veoveo_types.platform_names.OAuthClientId → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_types::platform_names::{veoveo_types::platform_names::OAuthClientId}::as_str]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 39:0-39:36
    Name pattern: [veoveo_types::platform_names::{veoveo_types::platform_names::OAuthClientId}::as_str]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{veoveo_types::platform_names::OAuthClientId}::as_str"]
def veoveo_types.platform_names.OAuthClientId.as_str
  : veoveo_types.platform_names.OAuthClientId → Result Str
  := fun value => Native.stringToStr value.value

/-- [veoveo_types::platform_names::{impl core::clone::Clone for veoveo_types::platform_names::OAuthClientId}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/platform_names.rs', lines 39:0-39:36
    Name pattern: [veoveo_types::platform_names::{core::clone::Clone<veoveo_types::platform_names::OAuthClientId>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::platform_names::{core::clone::Clone<veoveo_types::platform_names::OAuthClientId>}::clone"]
def veoveo_types.platform_names.OAuthClientId.Insts.CoreCloneClone.clone
  :
  veoveo_types.platform_names.OAuthClientId → Result
    veoveo_types.platform_names.OAuthClientId
  := fun value => .ok value

/-- [veoveo_types::provenance::{impl core::clone::Clone for veoveo_types::provenance::InvocationMode}::clone]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/provenance.rs', lines 7:16-7:21
    Name pattern: [veoveo_types::provenance::{core::clone::Clone<veoveo_types::provenance::InvocationMode>}::clone]
    Visibility: public -/
@[rust_fun
  "veoveo_types::provenance::{core::clone::Clone<veoveo_types::provenance::InvocationMode>}::clone"]
def veoveo_types.provenance.InvocationMode.Insts.CoreCloneClone.clone
  :
  veoveo_types.provenance.InvocationMode → Result
    veoveo_types.provenance.InvocationMode
  := fun value => .ok value

/-- [veoveo_types::provenance::{impl core::cmp::PartialEq<veoveo_types::provenance::InvocationMode> for veoveo_types::provenance::InvocationMode}::eq]:
    Source: '/cargo/git/checkouts/veoveo-core-becc5d46deb07dab/120306e/platform/types/src/provenance.rs', lines 7:29-7:38
    Name pattern: [veoveo_types::provenance::{core::cmp::PartialEq<veoveo_types::provenance::InvocationMode, veoveo_types::provenance::InvocationMode>}::eq]
    Visibility: public -/
@[rust_fun
  "veoveo_types::provenance::{core::cmp::PartialEq<veoveo_types::provenance::InvocationMode, veoveo_types::provenance::InvocationMode>}::eq"]
def veoveo_types.provenance.InvocationMode.Insts.CoreCmpPartialEqInvocationMode.eq
  :
  veoveo_types.provenance.InvocationMode →
    veoveo_types.provenance.InvocationMode → Result Bool
  := fun left right => .ok (decide (left = right))

/-- [veoveo_hub_sdk::identity::{veoveo_hub_sdk::identity::JwtId}::parse]:
    Source: 'crates/veoveo-hub-sdk/src/identity.rs', lines 167:0-167:43
    Visibility: public -/
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom identity.JwtId.parse
  {T0 : Type _} (coreconvertAsRefT0StrInst : core.convert.AsRef T0 Str) :
  T0 → Result (core.result.Result identity.JwtId
    veoveo_types.error.IdentifierError)

/-- [veoveo_hub_sdk::internal_auth::ensure_jwt_crypto_provider]:
    Source: 'crates/veoveo-hub-sdk/src/internal_auth.rs', lines 693:0-695:1 -/
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom internal_auth.ensure_jwt_crypto_provider : Result Unit

/-- [veoveo_hub_sdk::internal_auth::{veoveo_hub_sdk::internal_auth::HubInternalTokenIssuer}::sign_forwarded]:
    Source: 'crates/veoveo-hub-sdk/src/internal_auth.rs', lines 358:4-360:5 -/
-- OPEN NATIVE OBSERVATION: no success/output contract is assumed.
axiom internal_auth.HubInternalTokenIssuer.sign_forwarded
  :
  internal_auth.HubInternalTokenIssuer → internal_auth.AccessTokenClaims →
    Result (core.result.Result String internal_auth.InternalTokenError)


/-- Newly reached native text projection from the reviewed string expression.
Both Display and as_str are the exact pinned Core wrapper's underlying string. -/
@[rust_fun "veoveo_types::identity::{veoveo_types::identity::TokenIssuer}::as_str"]
def veoveo_types.identity.TokenIssuer.as_str
    (value : veoveo_types.identity.TokenIssuer) : Result Str := Native.stringToStr value.value

end
end RequestCompositionSdk
