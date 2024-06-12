use super::{
    mir::{Body, Mutability, Safety},
    with, DefId, Error, Symbol,
};
use crate::abi::Layout;
use crate::cycle_check;
use crate::mir::alloc::{read_target_int, read_target_uint, AllocId};
use crate::target::MachineInfo;
use crate::{crate_def::CrateDef, mir::mono::StaticDef};
use crate::{Filename, Opaque};
use serde::{
    ser::{Error as SerError, SerializeStruct, SerializeTupleStruct, SerializeTupleVariant},
    Serialize, Serializer,
};
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Range;
// use std::backtrace::Backtrace;

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct Ty(usize);

impl Serialize for Ty {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut cs = serializer.serialize_struct("Ty", 2)?;
        cs.serialize_field("id", &self.0)?;
        if cycle_check(|scc| scc.types.contains(self)) {
            debug!("Cycling at type ({}, {:?})", self.0, self.kind());
            cs.serialize_field("kind", &Option::<TyKind>::None)?;
        } else {
            cycle_check(|scc| scc.types.insert(*self));
            cs.serialize_field("kind", &Some(self.kind()))?;
            cycle_check(|scc| scc.types.remove(self));
        }
        cs.end()
    }
}

impl Debug for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ty").field("id", &self.0).field("kind", &self.kind()).finish()
    }
}

/// Constructors for `Ty`.
impl Ty {
    /// Create a new type from a given kind.
    pub fn from_rigid_kind(kind: RigidTy) -> Ty {
        with(|cx| cx.new_rigid_ty(kind))
    }

    /// Create a new array type.
    pub fn try_new_array(elem_ty: Ty, size: u64) -> Result<Ty, Error> {
        Ok(Ty::from_rigid_kind(RigidTy::Array(elem_ty, Const::try_from_target_usize(size)?)))
    }

    /// Create a new array type from Const length.
    pub fn new_array_with_const_len(elem_ty: Ty, len: Const) -> Ty {
        Ty::from_rigid_kind(RigidTy::Array(elem_ty, len))
    }

    /// Create a new pointer type.
    pub fn new_ptr(pointee_ty: Ty, mutability: Mutability) -> Ty {
        Ty::from_rigid_kind(RigidTy::RawPtr(pointee_ty, mutability))
    }

    /// Create a new reference type.
    pub fn new_ref(reg: Region, pointee_ty: Ty, mutability: Mutability) -> Ty {
        Ty::from_rigid_kind(RigidTy::Ref(reg, pointee_ty, mutability))
    }

    /// Create a new pointer type.
    pub fn new_tuple(tys: &[Ty]) -> Ty {
        Ty::from_rigid_kind(RigidTy::Tuple(Vec::from(tys)))
    }

    /// Create a new closure type.
    pub fn new_closure(def: ClosureDef, args: GenericArgs) -> Ty {
        Ty::from_rigid_kind(RigidTy::Closure(def, args))
    }

    /// Create a new coroutine type.
    pub fn new_coroutine(def: CoroutineDef, args: GenericArgs, mov: Movability) -> Ty {
        Ty::from_rigid_kind(RigidTy::Coroutine(def, args, mov))
    }

    /// Create a new box type that represents `Box<T>`, for the given inner type `T`.
    pub fn new_box(inner_ty: Ty) -> Ty {
        with(|cx| cx.new_box_ty(inner_ty))
    }

    /// Create a type representing `usize`.
    pub fn usize_ty() -> Ty {
        Ty::from_rigid_kind(RigidTy::Uint(UintTy::Usize))
    }

    /// Create a type representing `bool`.
    pub fn bool_ty() -> Ty {
        Ty::from_rigid_kind(RigidTy::Bool)
    }

    /// Create a type representing a signed integer.
    pub fn signed_ty(inner: IntTy) -> Ty {
        Ty::from_rigid_kind(RigidTy::Int(inner))
    }

    /// Create a type representing an unsigned integer.
    pub fn unsigned_ty(inner: UintTy) -> Ty {
        Ty::from_rigid_kind(RigidTy::Uint(inner))
    }

    /// Get a type layout.
    pub fn layout(self) -> Result<Layout, Error> {
        with(|cx| cx.ty_layout(self))
    }
}

impl Ty {
    pub fn kind(&self) -> TyKind {
        with(|context| context.ty_kind(*self))
    }
}

derive_serialize! {
/// Represents a pattern in the type system
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Pattern {
    Range { start: Option<Const>, end: Option<Const>, include_end: bool },
}

/// Represents a constant in MIR or from the Type system.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Const {
    /// The constant kind.
    pub(crate) kind: ConstantKind,
    /// The constant type.
    pub(crate) ty: Ty,
    /// Used for internal tracking of the internal constant.
    pub id: ConstId,
}
}

impl Const {
    /// Build a constant. Note that this should only be used by the compiler.
    pub fn new(kind: ConstantKind, ty: Ty, id: ConstId) -> Const {
        Const { kind, ty, id }
    }

    /// Retrieve the constant kind.
    pub fn kind(&self) -> &ConstantKind {
        &self.kind
    }

    /// Get the constant type.
    pub fn ty(&self) -> Ty {
        self.ty
    }

    /// Creates an interned usize constant.
    fn try_from_target_usize(val: u64) -> Result<Self, Error> {
        with(|cx| cx.try_new_const_uint(val.into(), UintTy::Usize))
    }

    /// Try to evaluate to a target `usize`.
    pub fn eval_target_usize(&self) -> Result<u64, Error> {
        with(|cx| cx.eval_target_usize(self))
    }

    /// Create a constant that represents a new zero-sized constant of type T.
    /// Fails if the type is not a ZST or if it doesn't have a known size.
    pub fn try_new_zero_sized(ty: Ty) -> Result<Const, Error> {
        with(|cx| cx.try_new_const_zst(ty))
    }

    /// Build a new constant that represents the given string.
    ///
    /// Note that there is no guarantee today about duplication of the same constant.
    /// I.e.: Calling this function multiple times with the same argument may or may not return
    /// the same allocation.
    pub fn from_str(value: &str) -> Const {
        with(|cx| cx.new_const_str(value))
    }

    /// Build a new constant that represents the given boolean value.
    pub fn from_bool(value: bool) -> Const {
        with(|cx| cx.new_const_bool(value))
    }

    /// Build a new constant that represents the given unsigned integer.
    pub fn try_from_uint(value: u128, uint_ty: UintTy) -> Result<Const, Error> {
        with(|cx| cx.try_new_const_uint(value, uint_ty))
    }
}

derive_serialize! {
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConstId(usize);
}

type Ident = Opaque;

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Region {
    pub kind: RegionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum RegionKind {
    ReEarlyParam(EarlyParamRegion),
    ReBound(DebruijnIndex, BoundRegion),
    ReStatic,
    RePlaceholder(Placeholder<BoundRegion>),
    ReErased,
}
}

pub(crate) type DebruijnIndex = u32;

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EarlyParamRegion {
    pub def_id: RegionDef,
    pub index: u32,
    pub name: Symbol,
}
}

pub(crate) type BoundVar = u32;

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BoundRegion {
    pub var: BoundVar,
    pub kind: BoundRegionKind,
}
}

pub(crate) type UniverseIndex = u32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Hash)]
pub struct Placeholder<T> {
    pub universe: UniverseIndex,
    pub bound: T,
}

derive_serialize! {
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span(usize);
}

impl Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Span")
            .field("id", &self.0)
            .field("repr", &with(|cx| cx.span_to_string(*self)))
            .finish()
    }
}

impl Span {
    /// Return filename for diagnostic purposes
    pub fn get_filename(&self) -> Filename {
        with(|c| c.get_filename(self))
    }

    /// Return lines that corespond to this `Span`
    pub fn get_lines(&self) -> LineInfo {
        with(|c| c.get_lines(self))
    }
}

derive_serialize! {
#[derive(Clone, Copy, Debug, Hash)]
/// Information you get from `Span` in a struct form.
/// Line and col start from 1.
pub struct LineInfo {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TyKind {
    RigidTy(RigidTy),
    #[serde(serialize_with = "serialize_alias")]
    Alias(AliasKind, AliasTy),
    // from rustc_middle::ty::TyKind::Param(rustc_middle::ty::ParamTy)
    Param(ParamTy),
    // from rustc_middle::ty::TyKind::Bound(DebruijnIndex, rustc_middle::ty::BoundTy)
    // usize param is DebruijnIndex, second param is wrapped u32 that represents bound variable identifier
    Bound(usize, BoundTy),
}
}

#[instrument(level = "debug", skip(serializer))]
fn serialize_alias<S>(akind: &AliasKind, aty: &AliasTy, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    struct AliasTyExtra<'a>(&'a AliasTy, &'a AliasKind);
    impl Serialize for AliasTyExtra<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut cs = serializer.serialize_struct("AliasTy", 2)?;
            match self.1 {
                AliasKind::Projection => cs.serialize_field("id", &self.0.def_id)?,
                _ => cs.serialize_field("ty", &with(|cx| cx.def_ty_with_args(self.0.def_id.def_id(), &self.0.args)))?,
            };
            cs.serialize_field("args", &self.0.args)?;
            cs.end()
        }
    }

    let mut cs = serializer.serialize_tuple_struct("Alias", 2)?;
    cs.serialize_field(akind)?;
    cs.serialize_field(&AliasTyExtra(aty,&akind))?;
    cs.end()
}

impl TyKind {
    pub fn rigid(&self) -> Option<&RigidTy> {
        if let TyKind::RigidTy(inner) = self { Some(inner) } else { None }
    }

    #[inline]
    pub fn is_unit(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Tuple(data)) if data.is_empty())
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Bool))
    }

    #[inline]
    pub fn is_char(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Char))
    }

    #[inline]
    pub fn is_trait(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Dynamic(_, _, DynKind::Dyn)))
    }

    #[inline]
    pub fn is_enum(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Adt(def, _)) if def.kind() == AdtKind::Enum)
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Adt(def, _)) if def.kind() == AdtKind::Struct)
    }

    #[inline]
    pub fn is_union(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Adt(def, _)) if def.kind() == AdtKind::Union)
    }

    #[inline]
    pub fn is_adt(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Adt(..)))
    }

    #[inline]
    pub fn is_ref(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Ref(..)))
    }

    #[inline]
    pub fn is_fn(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::FnDef(..)))
    }

    #[inline]
    pub fn is_fn_ptr(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::FnPtr(..)))
    }

    #[inline]
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            TyKind::RigidTy(
                RigidTy::Bool
                    | RigidTy::Char
                    | RigidTy::Int(_)
                    | RigidTy::Uint(_)
                    | RigidTy::Float(_)
            )
        )
    }

    #[inline]
    pub fn is_float(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Float(_)))
    }

    #[inline]
    pub fn is_integral(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Int(_) | RigidTy::Uint(_)))
    }

    #[inline]
    pub fn is_numeric(&self) -> bool {
        self.is_integral() || self.is_float()
    }

    #[inline]
    pub fn is_signed(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Int(_)))
    }

    #[inline]
    pub fn is_str(&self) -> bool {
        *self == TyKind::RigidTy(RigidTy::Str)
    }

    #[inline]
    pub fn is_cstr(&self) -> bool {
        let TyKind::RigidTy(RigidTy::Adt(def, _)) = self else {
            return false;
        };
        with(|cx| cx.adt_is_cstr(*def))
    }

    #[inline]
    pub fn is_slice(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Slice(_)))
    }

    #[inline]
    pub fn is_array(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Array(..)))
    }

    #[inline]
    pub fn is_mutable_ptr(&self) -> bool {
        matches!(
            self,
            TyKind::RigidTy(RigidTy::RawPtr(_, Mutability::Mut))
                | TyKind::RigidTy(RigidTy::Ref(_, _, Mutability::Mut))
        )
    }

    #[inline]
    pub fn is_raw_ptr(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::RawPtr(..)))
    }

    /// Tests if this is any kind of primitive pointer type (reference, raw pointer, fn pointer).
    #[inline]
    pub fn is_any_ptr(&self) -> bool {
        self.is_ref() || self.is_raw_ptr() || self.is_fn_ptr()
    }

    #[inline]
    pub fn is_coroutine(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Coroutine(..)))
    }

    #[inline]
    pub fn is_closure(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Closure(..)))
    }

    #[inline]
    pub fn is_box(&self) -> bool {
        match self {
            TyKind::RigidTy(RigidTy::Adt(def, _)) => def.is_box(),
            _ => false,
        }
    }

    #[inline]
    pub fn is_simd(&self) -> bool {
        matches!(self, TyKind::RigidTy(RigidTy::Adt(def, _)) if def.is_simd())
    }

    pub fn trait_principal(&self) -> Option<Binder<ExistentialTraitRef>> {
        if let TyKind::RigidTy(RigidTy::Dynamic(predicates, _, _)) = self {
            if let Some(Binder { value: ExistentialPredicate::Trait(trait_ref), bound_vars }) =
                predicates.first()
            {
                Some(Binder { value: trait_ref.clone(), bound_vars: bound_vars.clone() })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Returns the type of `ty[i]` for builtin types.
    pub fn builtin_index(&self) -> Option<Ty> {
        match self.rigid()? {
            RigidTy::Array(ty, _) | RigidTy::Slice(ty) => Some(*ty),
            _ => None,
        }
    }

    /// Returns the type and mutability of `*ty` for builtin types.
    ///
    /// The parameter `explicit` indicates if this is an *explicit* dereference.
    /// Some types -- notably unsafe ptrs -- can only be dereferenced explicitly.
    pub fn builtin_deref(&self, explicit: bool) -> Option<TypeAndMut> {
        match self.rigid()? {
            RigidTy::Adt(def, args) if def.is_box() => {
                Some(TypeAndMut { ty: *args.0.first()?.ty()?, mutability: Mutability::Not })
            }
            RigidTy::Ref(_, ty, mutability) => {
                Some(TypeAndMut { ty: *ty, mutability: *mutability })
            }
            RigidTy::RawPtr(ty, mutability) if explicit => {
                Some(TypeAndMut { ty: *ty, mutability: *mutability })
            }
            _ => None,
        }
    }

    /// Get the function signature for function like types (Fn, FnPtr, and Closure)
    pub fn fn_sig(&self) -> Option<PolyFnSig> {
        match self {
            TyKind::RigidTy(RigidTy::FnDef(def, args)) => Some(with(|cx| cx.fn_sig(*def, args))),
            TyKind::RigidTy(RigidTy::FnPtr(sig)) => Some(sig.clone()),
            TyKind::RigidTy(RigidTy::Closure(_def, args)) => Some(with(|cx| cx.closure_sig(args))),
            _ => None,
        }
    }

    /// Get the discriminant type for this type.
    pub fn discriminant_ty(&self) -> Option<Ty> {
        self.rigid().map(|ty| with(|cx| cx.rigid_ty_discriminant_ty(ty)))
    }
}

pub struct TypeAndMut {
    pub ty: Ty,
    pub mutability: Mutability,
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum RigidTy {
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    #[serde(serialize_with = "serialize_adtdef")]
    Adt(AdtDef, GenericArgs),
    Foreign(ForeignDef),
    Str,
    Array(Ty, Const),
    Pat(Ty, Pattern), // NOTE: uses unstable pattern_types feature declared via pattern_types! macro
    // Semantically, restricts type membership to elements which match the pattern
    Slice(Ty),
    RawPtr(Ty, Mutability),
    Ref(Region, Ty, Mutability),
    #[serde(serialize_with = "serialize_fndef")]
    FnDef(FnDef, GenericArgs),
    FnPtr(PolyFnSig),
    #[serde(serialize_with = "serialize_closuredef")]
    Closure(ClosureDef, GenericArgs),
    // FIXME(stable_mir): Movability here is redundant
    Coroutine(CoroutineDef, GenericArgs, Movability),
    Dynamic(Vec<Binder<ExistentialPredicate>>, Region, DynKind), // TODO: what is the best way to process?
    Never,
    Tuple(Vec<Ty>),
    CoroutineWitness(CoroutineWitnessDef, GenericArgs), // TODO: what is the best way to process?
}
}

// fn get_adtdef(ty: TyKind) -> (AdtDef, GenericArgs)  {
//     if let TyKind::RigidTy(RigidTy::Adt(def, args)) = ty {
//         return (def, args);
//     }
//     panic!("This should not be called on non-adt types");
// }

// fn vec_to_set<T>(v: Vec<T>) -> rustc_data_structures::fx::FxHashSet<T> where T: Eq, T: std::hash::Hash {
//     rustc_data_structures::fx::FxHashSet::<T>::from_iter(v.into_iter())
// }

// macro_rules! visualize_diff {
//     ($a:expr, $an:expr, $b:expr, $bn:expr) => {
//         if $a != $b {
//             debug!("Only {}: {:?}, Only {}: {:?}", $an, $a.difference($b).collect::<Vec<_>>(), $bn, $b.difference($a).collect::<Vec<_>>());
//         }
//     }
// }
 
// TODO: figure out behavior of def.ty() and def.ty_with_args()
//
// let has_proj = args.0.clone().into_iter().any(|x| if let GenericArgKind::Type(y) = x { matches!(y.kind(), TyKind::Alias(AliasKind::Projection, _)) } else { false } );
// AdtDef: std::Option<T>, adtdef.ty()                              => std::Option<T>
// adtdef.ty_with_args(vec![GenericArgKind::Type(RigidTy(uint32))]) => std::Option<uint32>                         #[allow(rustc::potential_query_instability)]
// adtdef.ty_with_args(vec![GenericArgKind::Type(Alias(projection, FnOnce::Output))]) => std::Option<???> // crash!#[instrument(level = "debug", skip(ser))]
//
// let ty = def.ty(); 
// let ty2 = def.ty_with_args(args);
// if ty != ty2 || ty.kind() != ty2.kind() {
//     let (d1, a1) = get_adtdef(ty.kind());
//     let (d2, a2) = get_adtdef(ty2.kind());
//     if &d1 != def || &d2 != def { panic!("def.ty() or def.ty_with_args() did not return RigidTy::Adt") }
//     let (s,s1,s2) = (vec_to_set(args.0.clone()), vec_to_set(a1.0), vec_to_set(a2.0));
//     visualize_diff!(&s, "src", &s1, "def.ty()");
//     visualize_diff!(&s, "src", &s2, "def.ty_with_args()");
// }
#[allow(rustc::potential_query_instability)]
#[instrument(level = "debug", skip(ser))]
fn serialize_adtdef<S>(def: &AdtDef, args: &GenericArgs, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut cs = ser.serialize_tuple_variant("RigidTy", 5, "AdtDef", 4)?;
    cs.serialize_field(&def.def_id())?;
    cs.serialize_field(&def.krate().name)?;
    cs.serialize_field(&def.name())?;
    cs.serialize_field(&args)?;
    cs.end()
}

#[instrument(level = "debug", skip(ser))]
fn serialize_fndef<S>(def: &FnDef, args: &GenericArgs, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut cs = ser.serialize_tuple_variant("RigidTy", 13, "FnDef", 5)?;
    cs.serialize_field(&def.def_id())?;
    cs.serialize_field(&def.krate().name)?;
    cs.serialize_field(&def.name())?;
    cs.serialize_field(&args)?;
    let sig = TyKind::RigidTy(RigidTy::FnDef(*def, args.clone()))
        .fn_sig()
        .ok_or(S::Error::custom("RigidTy::FnDef serialization failed"))?;
    cs.serialize_field(&sig)?;
    cs.end()
}

#[instrument(level = "debug", skip(ser))]
pub(crate) fn serialize_closuredef<S>(
    def: &ClosureDef,
    args: &GenericArgs,
    ser: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut cs = ser.serialize_tuple_variant("RigidTy", 14, "ClosureDef", 3)?;
    let sig = TyKind::RigidTy(RigidTy::Closure(*def, args.clone()))
        .fn_sig()
        .ok_or(S::Error::custom("RigidTy::Closure serialization failed"))?;
    cs.serialize_field(&def.def_id())?;
    cs.serialize_field(&args)?;
    cs.serialize_field(&sig)?;
    // TODO: the following might work better, if we knew how to instantiate it
    // cs.serialize_field(&with(|cx| cx.resolve_closure(*def, args, /* which closure kind??? */)))?;
    cs.end()
}

impl RigidTy {
    /// Get the discriminant type for this type.
    pub fn discriminant_ty(&self) -> Ty {
        with(|cx| cx.rigid_ty_discriminant_ty(self))
    }
}

impl From<RigidTy> for TyKind {
    fn from(value: RigidTy) -> Self {
        TyKind::RigidTy(value)
    }
}

derive_serialize! {
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntTy {
    Isize,
    I8,
    I16,
    I32,
    I64,
    I128,
}
}

impl IntTy {
    pub fn num_bytes(self) -> usize {
        match self {
            IntTy::Isize => crate::target::MachineInfo::target_pointer_width().bytes().into(),
            IntTy::I8 => 1,
            IntTy::I16 => 2,
            IntTy::I32 => 4,
            IntTy::I64 => 8,
            IntTy::I128 => 16,
        }
    }
}

derive_serialize! {
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UintTy {
    Usize,
    U8,
    U16,
    U32,
    U64,
    U128,
}
}

impl UintTy {
    pub fn num_bytes(self) -> usize {
        match self {
            UintTy::Usize => crate::target::MachineInfo::target_pointer_width().bytes().into(),
            UintTy::U8 => 1,
            UintTy::U16 => 2,
            UintTy::U32 => 4,
            UintTy::U64 => 8,
            UintTy::U128 => 16,
        }
    }
}

derive_serialize! {
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatTy {
    F32,
    F64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Movability {
    Static,
    Movable,
}
}

crate_def! {
    pub ForeignModuleDef;
}

impl Serialize for ForeignModuleDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct("ForeignModuleDef", &self.module())
    }
}

impl ForeignModuleDef {
    pub fn module(&self) -> ForeignModule {
        with(|cx| cx.foreign_module(*self))
    }
}

#[derive(Debug)]
pub struct ForeignModule {
    pub def_id: ForeignModuleDef,
    pub abi: Abi,
}

impl ForeignModule {
    pub fn items(&self) -> Vec<ForeignDef> {
        with(|cx| cx.foreign_items(self.def_id))
    }
}

impl Serialize for ForeignModule {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("ForeignModule", 2)?;
        s.serialize_field("def_id", &self.def_id.0)?;
        s.serialize_field("abi", &self.abi)?;
        s.end()
    }
}

crate_def! {
    /// Hold information about a ForeignItem in a crate.
    pub ForeignDef;
}

impl Serialize for ForeignDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut cs = serializer.serialize_tuple_struct("ForeignDef", 4)?;
        cs.serialize_field(&self.def_id())?;
        cs.serialize_field(&self.krate().name)?;
        cs.serialize_field(&self.name())?;
        cs.serialize_field(&self.kind())?;
        cs.end()
    }
}

impl ForeignDef {
    pub fn kind(&self) -> ForeignItemKind {
        with(|cx| cx.foreign_item_kind(*self))
    }
}

derive_serialize! {
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ForeignItemKind {
    Fn(FnDef),
    Static(StaticDef),
    Type(Ty),
}
}

crate_def! {
    /// Hold information about a function definition in a crate.
    pub FnDef;
}

impl Serialize for FnDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut cs = serializer.serialize_tuple_struct("FnDef", 3)?;
        cs.serialize_field(&self.def_id())?;
        cs.serialize_field(&self.krate().name)?;
        cs.serialize_field(&self.name())?;
        cs.end()
        // cs.serialize_field(&with(|cx| cx.def_ty(self.def_id())))
        // TODO: the following would be more interesting, but not sure where to recover the generic args
        // serializer.serialize_newtype_struct("FnDef", with(|cx| cx.fn_sig(*self, /* missing GenericArgs type */)))
    }
}

impl FnDef {
    // Get the function body if available.
    pub fn body(&self) -> Option<Body> {
        with(|ctx| ctx.has_body(self.0).then(|| ctx.mir_body(self.0)))
    }
}

crate_def! {
    pub ClosureDef;
}

crate_def! {
    #[derive(Serialize)]
    pub CoroutineDef;
}

crate_def! {
    #[derive(Serialize)]
    pub ParamDef;
}

crate_def! {
    #[derive(Serialize)]
    pub BrNamedDef;
}

crate_def! {
    pub AdtDef;
}

impl Serialize for AdtDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut cs = serializer.serialize_tuple_struct("AdtDef", 3)?;
        cs.serialize_field(&self.def_id())?;
        cs.serialize_field(&self.krate().name)?;
        cs.serialize_field(&self.name())?;
        cs.end()
    }
}

derive_serialize! {
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AdtKind {
    Enum,
    Union,
    Struct,
}
}

impl AdtDef {
    pub fn kind(&self) -> AdtKind {
        with(|cx| cx.adt_kind(*self))
    }

    /// Retrieve the type of this Adt.
    pub fn ty(&self) -> Ty {
        with(|cx| cx.def_ty(self.0))
    }

    /// Retrieve the type of this Adt by instantiating and normalizing it with the given arguments.
    ///
    /// This will assume the type can be instantiated with these arguments.
    pub fn ty_with_args(&self, args: &GenericArgs) -> Ty {
        with(|cx| cx.def_ty_with_args(self.0, args))
    }

    pub fn is_box(&self) -> bool {
        with(|cx| cx.adt_is_box(*self))
    }

    pub fn is_simd(&self) -> bool {
        with(|cx| cx.adt_is_simd(*self))
    }

    /// The number of variants in this ADT.
    pub fn num_variants(&self) -> usize {
        with(|cx| cx.adt_variants_len(*self))
    }

    /// Retrieve the variants in this ADT.
    pub fn variants(&self) -> Vec<VariantDef> {
        self.variants_iter().collect()
    }

    /// Iterate over the variants in this ADT.
    pub fn variants_iter(&self) -> impl Iterator<Item = VariantDef> + '_ {
        (0..self.num_variants())
            .map(|idx| VariantDef { idx: VariantIdx::to_val(idx), adt_def: *self })
    }

    pub fn variant(&self, idx: VariantIdx) -> Option<VariantDef> {
        (idx.to_index() < self.num_variants()).then_some(VariantDef { idx, adt_def: *self })
    }
}

derive_serialize! {
/// Definition of a variant, which can be either a struct / union field or an enum variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VariantDef {
    /// The variant index.
    ///
    /// ## Warning
    /// Do not access this field directly!
    pub idx: VariantIdx,
    /// The data type where this variant comes from.
    /// For now, we use this to retrieve information about the variant itself so we don't need to
    /// cache more information.
    ///
    /// ## Warning
    /// Do not access this field directly!
    pub adt_def: AdtDef,
}
}

impl VariantDef {
    pub fn name(&self) -> Symbol {
        with(|cx| cx.variant_name(*self))
    }

    /// Retrieve all the fields in this variant.
    // We expect user to cache this and use it directly since today it is expensive to generate all
    // fields name.
    pub fn fields(&self) -> Vec<FieldDef> {
        with(|cx| cx.variant_fields(*self))
    }
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FieldDef {
    /// The field definition.
    ///
    /// ## Warning
    /// Do not access this field directly! This is public for the compiler to have access to it.
    pub def: DefId,

    /// The field name.
    pub name: Symbol,
}
}

impl FieldDef {
    /// Retrieve the type of this field instantiating and normalizing it with the given arguments.
    ///
    /// This will assume the type can be instantiated with these arguments.
    pub fn ty_with_args(&self, args: &GenericArgs) -> Ty {
        with(|cx| cx.def_ty_with_args(self.def, args))
    }

    /// Retrieve the type of this field.
    pub fn ty(&self) -> Ty {
        with(|cx| cx.def_ty(self.def))
    }
}

impl Display for AdtKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AdtKind::Enum => "enum",
            AdtKind::Union => "union",
            AdtKind::Struct => "struct",
        })
    }
}

impl AdtKind {
    pub fn is_enum(&self) -> bool {
        matches!(self, AdtKind::Enum)
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, AdtKind::Struct)
    }

    pub fn is_union(&self) -> bool {
        matches!(self, AdtKind::Union)
    }
}

crate_def! {
    pub AliasDef;
}

impl Serialize for AliasDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut cs = serializer.serialize_tuple_struct("AliasDef", 2)?;
        cs.serialize_field(&self.def_id())?;
        cs.serialize_field(&self.name())?;
        cs.end()
    }
}

crate_def! {
    /// A trait's definition.
    pub TraitDef;
}

// TODO: check if more specialized expansions are needed
impl Serialize for TraitDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct("TraitDef", &TraitDef::declaration(self))
    }
}

impl TraitDef {
    pub fn declaration(trait_def: &TraitDef) -> TraitDecl {
        with(|cx| cx.trait_decl(trait_def))
    }
}

crate_def! {
    #[derive(Serialize)]
    pub GenericDef;
}

crate_def! {
    #[derive(Serialize)]
    pub ConstDef;
}

crate_def! {
    /// A trait impl definition.
    pub ImplDef;
}

impl Serialize for ImplDef {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct("ImplDef", &self.trait_impl())
    }
}

impl ImplDef {
    /// Retrieve information about this implementation.
    pub fn trait_impl(&self) -> ImplTrait {
        with(|cx| cx.trait_impl(self))
    }
}

crate_def! {
    #[derive(Serialize)]
    pub RegionDef;
}

crate_def! {
    #[derive(Serialize)]
    pub CoroutineWitnessDef;
}

/// A list of generic arguments.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GenericArgs(pub Vec<GenericArgKind>);

impl Serialize for GenericArgs {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct("GenericArgs", &self.0)
    }
}

impl std::ops::Index<ParamTy> for GenericArgs {
    type Output = Ty;

    fn index(&self, index: ParamTy) -> &Self::Output {
        self.0[index.index as usize].expect_ty()
    }
}

impl std::ops::Index<ParamConst> for GenericArgs {
    type Output = Const;

    fn index(&self, index: ParamConst) -> &Self::Output {
        self.0[index.index as usize].expect_const()
    }
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum GenericArgKind {
    Lifetime(Region),
    Type(Ty),
    Const(Const),
}
}

impl GenericArgKind {
    /// Panic if this generic argument is not a type, otherwise
    /// return the type.
    #[track_caller]
    pub fn expect_ty(&self) -> &Ty {
        match self {
            GenericArgKind::Type(ty) => ty,
            _ => panic!("{self:?}"),
        }
    }

    /// Panic if this generic argument is not a const, otherwise
    /// return the const.
    #[track_caller]
    pub fn expect_const(&self) -> &Const {
        match self {
            GenericArgKind::Const(c) => c,
            _ => panic!("{self:?}"),
        }
    }

    /// Return the generic argument type if applicable, otherwise return `None`.
    pub fn ty(&self) -> Option<&Ty> {
        match self {
            GenericArgKind::Type(ty) => Some(ty),
            _ => None,
        }
    }
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TermKind {
    Type(Ty),
    Const(Const),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AliasKind {
    Projection,
    Inherent,
    Opaque,
    Weak,
}
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AliasTy {
    pub def_id: AliasDef,
    pub args: GenericArgs,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AliasTerm {
    pub def_id: AliasDef,
    pub args: GenericArgs,
}

impl Serialize for AliasTy {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_alias_args(&self.def_id, &self.args, serializer)
    }
}

impl Serialize for AliasTerm {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_alias_args(&self.def_id, &self.args, serializer)
    }
}

#[instrument(level = "debug", skip(serializer))]
fn serialize_alias_args<S>(
    def_id: &AliasDef,
    args: &GenericArgs,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut cs = serializer.serialize_struct("Alias", 2)?;
    cs.serialize_field("def_id", &with(|cx| cx.def_ty_with_args(def_id.def_id(), args)))?;
    cs.serialize_field("args", args)?;
    cs.end()
}

pub type PolyFnSig = Binder<FnSig>;

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FnSig {
    pub inputs_and_output: Vec<Ty>,
    pub c_variadic: bool,
    pub safety: Safety,
    pub abi: Abi,
}
}

impl FnSig {
    pub fn output(&self) -> Ty {
        self.inputs_and_output[self.inputs_and_output.len() - 1]
    }

    pub fn inputs(&self) -> &[Ty] {
        &self.inputs_and_output[..self.inputs_and_output.len() - 1]
    }
}

derive_serialize! {
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum Abi {
    Rust,
    C { unwind: bool },
    Cdecl { unwind: bool },
    Stdcall { unwind: bool },
    Fastcall { unwind: bool },
    Vectorcall { unwind: bool },
    Thiscall { unwind: bool },
    Aapcs { unwind: bool },
    Win64 { unwind: bool },
    SysV64 { unwind: bool },
    PtxKernel,
    Msp430Interrupt,
    X86Interrupt,
    EfiApi,
    AvrInterrupt,
    AvrNonBlockingInterrupt,
    CCmseNonSecureCall,
    Wasm,
    System { unwind: bool },
    RustIntrinsic,
    RustCall,
    Unadjusted,
    RustCold,
    RiscvInterruptM,
    RiscvInterruptS,
}
}

/// A binder represents a possibly generic type and its bound vars.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Hash)]
pub struct Binder<T> {
    pub value: T,
    pub bound_vars: Vec<BoundVariableKind>,
}

impl<T> Binder<T> {
    /// Create a new binder with the given bound vars.
    pub fn bind_with_vars(value: T, bound_vars: Vec<BoundVariableKind>) -> Self {
        Binder { value, bound_vars }
    }

    /// Create a new binder with no bounded variable.
    pub fn dummy(value: T) -> Self {
        Binder { value, bound_vars: vec![] }
    }

    pub fn skip_binder(self) -> T {
        self.value
    }

    pub fn map_bound_ref<F, U>(&self, f: F) -> Binder<U>
    where
        F: FnOnce(&T) -> U,
    {
        let Binder { value, bound_vars } = self;
        let new_value = f(value);
        Binder { value: new_value, bound_vars: bound_vars.clone() }
    }

    pub fn map_bound<F, U>(self, f: F) -> Binder<U>
    where
        F: FnOnce(T) -> U,
    {
        let Binder { value, bound_vars } = self;
        let new_value = f(value);
        Binder { value: new_value, bound_vars }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Hash)]
pub struct EarlyBinder<T> {
    pub value: T,
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum BoundVariableKind {
    Ty(BoundTyKind),
    Region(BoundRegionKind),
    Const,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum BoundTyKind {
    Anon,
    Param(ParamDef, String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum BoundRegionKind {
    BrAnon,
    BrNamed(BrNamedDef, String),
    BrEnv,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum DynKind {
    Dyn,
    DynStar,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ExistentialPredicate {
    Trait(ExistentialTraitRef),
    Projection(ExistentialProjection),
    AutoTrait(TraitDef),
}

/// An existential reference to a trait where `Self` is not included.
///
/// The `generic_args` will include any other known argument.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ExistentialTraitRef {
    pub def_id: TraitDef,
    pub generic_args: GenericArgs,
}
}

impl Binder<ExistentialTraitRef> {
    pub fn with_self_ty(&self, self_ty: Ty) -> Binder<TraitRef> {
        self.map_bound_ref(|trait_ref| trait_ref.with_self_ty(self_ty))
    }
}

impl ExistentialTraitRef {
    pub fn with_self_ty(&self, self_ty: Ty) -> TraitRef {
        TraitRef::new(self.def_id, self_ty, &self.generic_args)
    }
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ExistentialProjection {
    #[serde(serialize_with = "serialize_cratedef")]
    pub def_id: TraitDef,
    pub generic_args: GenericArgs,
    pub term: TermKind,
}
}

#[instrument(level = "debug", skip(serializer))]
fn serialize_cratedef<S>(def: &(impl CrateDef + std::fmt::Debug), serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_newtype_struct("DefId", &def.def_id())
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ParamTy {
    pub index: u32,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BoundTy {
    pub var: usize,
    pub kind: BoundTyKind,
}
}

pub type Bytes = Vec<Option<u8>>;

/// Size in bytes.
pub type Size = usize;

derive_serialize! {
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Prov(pub AllocId);
}

pub type Align = u64;
pub type Promoted = u32;
pub type InitMaskMaterialized = Vec<u64>;

derive_serialize! {
/// Stores the provenance information of pointers stored in memory.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProvenanceMap {
    /// Provenance in this map applies from the given offset for an entire pointer-size worth of
    /// bytes. Two entries in this map are always at least a pointer size apart.
    pub ptrs: Vec<(Size, Prov)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Allocation {
    pub bytes: Bytes,
    pub provenance: ProvenanceMap,
    pub align: Align,
    pub mutability: Mutability,
}
}

impl Allocation {
    /// Get a vector of bytes for an Allocation that has been fully initialized
    pub fn raw_bytes(&self) -> Result<Vec<u8>, Error> {
        self.bytes
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error!("Found uninitialized bytes: `{:?}`", self.bytes))
    }

    /// Read a uint value from the specified range.
    pub fn read_partial_uint(&self, range: Range<usize>) -> Result<u128, Error> {
        if range.end - range.start > 16 {
            return Err(error!("Allocation is bigger than largest integer"));
        }
        if range.end > self.bytes.len() {
            return Err(error!(
                "Range is out of bounds. Allocation length is `{}`, but requested range `{:?}`",
                self.bytes.len(),
                range
            ));
        }
        let raw = self.bytes[range]
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error!("Found uninitialized bytes: `{:?}`", self.bytes))?;
        read_target_uint(&raw)
    }

    /// Read this allocation and try to convert it to an unassigned integer.
    pub fn read_uint(&self) -> Result<u128, Error> {
        if self.bytes.len() > 16 {
            return Err(error!("Allocation is bigger than largest integer"));
        }
        let raw = self.raw_bytes()?;
        read_target_uint(&raw)
    }

    /// Read this allocation and try to convert it to a signed integer.
    pub fn read_int(&self) -> Result<i128, Error> {
        if self.bytes.len() > 16 {
            return Err(error!("Allocation is bigger than largest integer"));
        }
        let raw = self.raw_bytes()?;
        read_target_int(&raw)
    }

    /// Read this allocation and try to convert it to a boolean.
    pub fn read_bool(&self) -> Result<bool, Error> {
        match self.read_int()? {
            0 => Ok(false),
            1 => Ok(true),
            val @ _ => Err(error!("Unexpected value for bool: `{val}`")),
        }
    }

    /// Read this allocation as a pointer and return whether it represents a `null` pointer.
    pub fn is_null(&self) -> Result<bool, Error> {
        let len = self.bytes.len();
        let ptr_len = MachineInfo::target_pointer_width().bytes();
        if len != ptr_len {
            return Err(error!("Expected width of pointer (`{ptr_len}`), but found: `{len}`"));
        }
        Ok(self.read_uint()? == 0 && self.provenance.ptrs.is_empty())
    }
}

derive_serialize! {
// stable_mir::ty::ConstantKind collapses two variants of Rust
//
// 1.  mir::Const::Ty(ty::Const)                      -> convert(...)
//     mir::Const::Unevaluated(mir::UnevaluatedConst) -> ConstantKind::Unevaluated(...)
//     mir::Const::Val(mir::ConstValue)               ->
//       match(mir::ConstValue)
//         mir::ConstValue::ZeroSized -> ConstantKind::ZeroSized
//         v                          -> ConstandKind::Allocated(v)
//
// 2.  ty::Const::Param(ParamConst)                 -> ConstantKind(Param(...))
//     ty::Const::Unevaluated(ty::UnevaluatedConst) -> ConstantKind(Unevaluated(...))
//     ty::Const::Value(ValTree)                    ->
//       match(tcx.valtree_to_const_val(ValTree))
//         mir::ConstValue::ZeroSized -> ConstantKind::ZeroSized
//         v                          -> ConstandKind::Allocated(v)
//
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ConstantKind {
    Allocated(Allocation), // collasped ConstAllocation/Allocation
    // from mir::Const::Val
    // from ty::Const::Value which does _not_ convert to mir::Const::Val(ZeroSized)
    Unevaluated(UnevaluatedConst), // lifted from mir::Const::Unevaluated or ty::Const::Unevaluated
    Param(ParamConst),             // lifted from ty::Const::Param
    /// Store ZST constants.
    /// We have to special handle these constants since its type might be generic.
    ZeroSized, // lifted from mir::Const::Val(ZeroSized)
                                   // and from ty::Const::Value that converts to mir::Const::Val(ZeroSized)
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ParamConst {
    pub index: u32,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct UnevaluatedConst {
    pub def: ConstDef,
    pub args: GenericArgs,
    pub promoted: Option<Promoted>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TraitSpecializationKind {
    None,
    Marker,
    AlwaysApplicable,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TraitDecl {
    #[serde(skip)] // skip serializing this field to avoid recursive cycles
    pub def_id: TraitDef,
    pub safety: Safety,
    pub paren_sugar: bool,
    pub has_auto_impl: bool,
    pub is_marker: bool,
    pub is_coinductive: bool,
    pub skip_array_during_method_dispatch: bool,
    pub skip_boxed_slice_during_method_dispatch: bool,
    pub specialization_kind: TraitSpecializationKind,
    pub must_implement_one_of: Option<Vec<Ident>>,
    pub implement_via_object: bool,
    pub deny_explicit_impl: bool,
}
}

impl TraitDecl {
    pub fn generics_of(&self) -> Generics {
        with(|cx| cx.generics_of(self.def_id.0))
    }

    pub fn predicates_of(&self) -> GenericPredicates {
        with(|cx| cx.predicates_of(self.def_id.0))
    }

    pub fn explicit_predicates_of(&self) -> GenericPredicates {
        with(|cx| cx.explicit_predicates_of(self.def_id.0))
    }
}

pub type ImplTrait = EarlyBinder<TraitRef>;

derive_serialize! {
/// A complete reference to a trait, i.e., one where `Self` is known.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TraitRef {
    pub def_id: TraitDef,
    /// The generic arguments for this definition.
    /// The first element must always be type, and it represents `Self`.
    args: GenericArgs,
}
}

impl TraitRef {
    pub fn new(def_id: TraitDef, self_ty: Ty, gen_args: &GenericArgs) -> TraitRef {
        let mut args = vec![GenericArgKind::Type(self_ty)];
        args.extend_from_slice(&gen_args.0);
        TraitRef { def_id, args: GenericArgs(args) }
    }

    pub fn try_new(def_id: TraitDef, args: GenericArgs) -> Result<TraitRef, ()> {
        match &args.0[..] {
            [GenericArgKind::Type(_), ..] => Ok(TraitRef { def_id, args }),
            _ => Err(()),
        }
    }

    pub fn args(&self) -> &GenericArgs {
        &self.args
    }

    pub fn self_ty(&self) -> Ty {
        let GenericArgKind::Type(self_ty) = self.args.0[0] else {
            panic!("Self must be a type, but found: {:?}", self.args.0[0])
        };
        self_ty
    }
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Generics {
    pub parent: Option<GenericDef>,
    pub parent_count: usize,
    pub params: Vec<GenericParamDef>,
    pub param_def_id_to_index: Vec<(GenericDef, u32)>,
    pub has_self: bool,
    pub has_late_bound_regions: Option<Span>,
    pub host_effect_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum GenericParamDefKind {
    Lifetime,
    Type { has_default: bool, synthetic: bool },
    Const { has_default: bool },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GenericParamDef {
    pub name: super::Symbol,
    pub def_id: GenericDef,
    pub index: u32,
    pub pure_wrt_drop: bool,
    pub kind: GenericParamDefKind,
}
}

pub struct GenericPredicates {
    pub parent: Option<TraitDef>,
    pub predicates: Vec<(PredicateKind, Span)>,
}

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PredicateKind {
    Clause(ClauseKind),
    ObjectSafe(TraitDef),
    SubType(SubtypePredicate),
    Coerce(CoercePredicate),
    ConstEquate(Const, Const),
    Ambiguous,
    AliasRelate(TermKind, TermKind, AliasRelationDirection),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClauseKind {
    Trait(TraitPredicate),
    RegionOutlives(RegionOutlivesPredicate),
    TypeOutlives(TypeOutlivesPredicate),
    Projection(ProjectionPredicate),
    ConstArgHasType(Const, Ty),
    WellFormed(GenericArgKind),
    ConstEvaluatable(Const),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClosureKind {
    Fn,
    FnMut,
    FnOnce,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SubtypePredicate {
    pub a: Ty,
    pub b: Ty,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CoercePredicate {
    pub a: Ty,
    pub b: Ty,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AliasRelationDirection {
    Equate,
    Subtype,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TraitPredicate {
    pub trait_ref: TraitRef,
    pub polarity: PredicatePolarity,
}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Hash)]
pub struct OutlivesPredicate<A, B>(pub A, pub B);

pub type RegionOutlivesPredicate = OutlivesPredicate<Region, Region>;
pub type TypeOutlivesPredicate = OutlivesPredicate<Ty, Region>;

derive_serialize! {
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProjectionPredicate {
    pub projection_term: AliasTerm,
    pub term: TermKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ImplPolarity {
    Positive,
    Negative,
    Reservation,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PredicatePolarity {
    Positive,
    Negative,
}
}

pub trait IndexedVal {
    fn to_val(index: usize) -> Self;

    fn to_index(&self) -> usize;
}

macro_rules! index_impl {
    ($name:ident) => {
        impl IndexedVal for $name {
            fn to_val(index: usize) -> Self {
                $name(index)
            }
            fn to_index(&self) -> usize {
                self.0
            }
        }
    };
}

index_impl!(ConstId);
index_impl!(Ty);
index_impl!(Span);

derive_serialize! {
/// The source-order index of a variant in a type.
///
/// For example, in the following types,
/// ```ignore(illustrative)
/// enum Demo1 {
///    Variant0 { a: bool, b: i32 },
///    Variant1 { c: u8, d: u64 },
/// }
/// struct Demo2 { e: u8, f: u16, g: u8 }
/// ```
/// `a` is in the variant with the `VariantIdx` of `0`,
/// `c` is in the variant with the `VariantIdx` of `1`, and
/// `g` is in the variant with the `VariantIdx` of `0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VariantIdx(usize);
}

index_impl!(VariantIdx);
