//! # The Stable MIR Visitor
//!
//! ## Overview
//!
//! We currently only support an immutable visitor.
//! The structure of this visitor is similar to the ones internal to `rustc`,
//! and it follows the following conventions:
//!
//! For every mir item, the trait has a `visit_<item>` and a `super_<item>` method.
//! - `visit_<item>`, by default, calls `super_<item>`
//! - `super_<item>`, by default, destructures the `<item>` and calls `visit_<sub_item>` for
//!   all sub-items that compose the original item.
//!
//! In order to implement a visitor, override the `visit_*` methods for the types you are
//! interested in analyzing, and invoke (within that method call)
//! `self.super_*` to continue to the traverse.
//! Avoid calling `super` methods in other circumstances.
//!
//! For the most part, we do not destructure things external to the
//! MIR, e.g., types, spans, etc, but simply visit them and stop.
//! This avoids duplication with other visitors like `TypeFoldable`.
//!
//! ## Updating
//!
//! The code is written in a very deliberate style intended to minimize
//! the chance of things being overlooked.
//!
//! Use pattern matching to reference fields and ensure that all
//! matches are exhaustive.
//!
//! For this to work, ALL MATCHES MUST BE EXHAUSTIVE IN FIELDS AND VARIANTS.
//! That means you never write `..` to skip over fields, nor do you write `_`
//! to skip over variants in a `match`.
//!
//! The only place that `_` is acceptable is to match a field (or
//! variant argument) that does not require visiting.

use crate::mir::*;
use crate::ty::*;
use crate::{Error, Opaque, Span};

pub trait MirVisitor {
    fn visit_body(&mut self, body: &Body) {
        self.super_body(body)
    }

    fn visit_basic_block(&mut self, bb: &BasicBlock) {
        self.super_basic_block(bb)
    }

    fn visit_ret_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.super_ret_decl(local, decl)
    }

    fn visit_arg_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.super_arg_decl(local, decl)
    }

    fn visit_local_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.super_local_decl(local, decl)
    }

    fn visit_statement(&mut self, stmt: &Statement, location: Location) {
        self.super_statement(stmt, location)
    }

    fn visit_terminator(&mut self, term: &Terminator, location: Location) {
        self.super_terminator(term, location)
    }

    fn visit_span(&mut self, span: &Span) {
        self.super_span(span)
    }

    fn visit_place(&mut self, place: &Place, ptx: PlaceContext, location: Location) {
        self.super_place(place, ptx, location)
    }

    fn visit_projection_elem<'a>(
        &mut self,
        place_ref: PlaceRef<'a>,
        elem: &ProjectionElem,
        ptx: PlaceContext,
        location: Location,
    ) {
        let _ = place_ref;
        self.super_projection_elem(elem, ptx, location);
    }

    fn visit_local(&mut self, local: &Local, ptx: PlaceContext, location: Location) {
        let _ = (local, ptx, location);
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue, location: Location) {
        self.super_rvalue(rvalue, location)
    }

    fn visit_operand(&mut self, operand: &Operand, location: Location) {
        self.super_operand(operand, location)
    }

    fn visit_user_type_projection(&mut self, projection: &UserTypeProjection) {
        self.super_user_type_projection(projection)
    }

    fn visit_ty(&mut self, ty: &Ty, location: Location) {
        let _ = location;
        self.super_ty(ty);
        self.visit_ty_kind(&ty.kind(), location)
    }

    fn visit_ty_kind(&mut self, kind: &TyKind, location: Location) { // Not sure if this is the correct location for the region
        self.super_ty_kind(kind, location)
    }

    fn visit_binder<T>(&mut self, binder: &Binder<T>) {
        self.super_binder(binder)
    }

    fn visit_constant(&mut self, constant: &Constant, location: Location) {
        self.super_constant(constant, location)
    }

    fn visit_const(&mut self, constant: &Const, location: Location) {
        self.super_const(constant, location)
    }

    fn visit_region(&mut self, region: &Region, location: Location) {
        let _ = location;
        self.super_region(region)
    }

    fn visit_args(&mut self, args: &GenericArgs, location: Location) {
        let _ = location;
        self.super_args(args)
    }

    fn visit_assert_msg(&mut self, msg: &AssertMessage, location: Location) {
        self.super_assert_msg(msg, location)
    }

    fn visit_var_debug_info(&mut self, var_debug_info: &VarDebugInfo) {
        self.super_var_debug_info(var_debug_info);
    }

    fn super_body(&mut self, body: &Body) {
        let Body { blocks, locals: _, arg_count, var_debug_info, spread_arg: _, span } = body;

        for bb in blocks {
            self.visit_basic_block(bb);
        }

        self.visit_ret_decl(RETURN_LOCAL, body.ret_local());

        for (idx, arg) in body.arg_locals().iter().enumerate() {
            self.visit_arg_decl(idx + 1, arg)
        }

        let local_start = arg_count + 1;
        for (idx, arg) in body.inner_locals().iter().enumerate() {
            self.visit_local_decl(idx + local_start, arg)
        }

        for info in var_debug_info.iter() {
            self.visit_var_debug_info(info);
        }

        self.visit_span(span)
    }

    fn super_basic_block(&mut self, bb: &BasicBlock) {
        let BasicBlock { statements, terminator } = bb;
        for stmt in statements {
            self.visit_statement(stmt, Location(stmt.span));
        }
        self.visit_terminator(terminator, Location(terminator.span));
    }

    fn super_local_decl(&mut self, local: Local, decl: &LocalDecl) {
        let _ = local;
        let LocalDecl { ty, span, .. } = decl;
        self.visit_ty(ty, Location(*span));
    }

    fn super_ret_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.super_local_decl(local, decl)
    }

    fn super_arg_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.super_local_decl(local, decl)
    }

    fn super_statement(&mut self, stmt: &Statement, location: Location) {
        let Statement { kind, span } = stmt;
        self.visit_span(span);
        match kind {
            StatementKind::Assign(place, rvalue) => {
                self.visit_place(place, PlaceContext::MUTATING, location);
                self.visit_rvalue(rvalue, location);
            }
            StatementKind::FakeRead(_, place) => {
                self.visit_place(place, PlaceContext::NON_MUTATING, location);
            }
            StatementKind::SetDiscriminant { place, .. } => {
                self.visit_place(place, PlaceContext::MUTATING, location);
            }
            StatementKind::Deinit(place) => {
                self.visit_place(place, PlaceContext::MUTATING, location);
            }
            StatementKind::StorageLive(local) => {
                self.visit_local(local, PlaceContext::NON_USE, location);
            }
            StatementKind::StorageDead(local) => {
                self.visit_local(local, PlaceContext::NON_USE, location);
            }
            StatementKind::Retag(_, place) => {
                self.visit_place(place, PlaceContext::MUTATING, location);
            }
            StatementKind::PlaceMention(place) => {
                self.visit_place(place, PlaceContext::NON_MUTATING, location);
            }
            StatementKind::AscribeUserType { place, projections, variance: _ } => {
                self.visit_place(place, PlaceContext::NON_USE, location);
                self.visit_user_type_projection(projections);
            }
            StatementKind::Coverage(coverage) => visit_opaque(coverage),
            StatementKind::Intrinsic(intrisic) => match intrisic {
                NonDivergingIntrinsic::Assume(operand) => {
                    self.visit_operand(operand, location);
                }
                NonDivergingIntrinsic::CopyNonOverlapping(CopyNonOverlapping {
                    src,
                    dst,
                    count,
                }) => {
                    self.visit_operand(src, location);
                    self.visit_operand(dst, location);
                    self.visit_operand(count, location);
                }
            },
            StatementKind::ConstEvalCounter => {}
            StatementKind::Nop => {}
        }
    }

    fn super_terminator(&mut self, term: &Terminator, location: Location) {
        let Terminator { kind, span } = term;
        self.visit_span(span);
        match kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::Resume
            | TerminatorKind::Abort
            | TerminatorKind::Unreachable => {}
            TerminatorKind::Assert { cond, expected: _, msg, target: _, unwind: _ } => {
                self.visit_operand(cond, location);
                self.visit_assert_msg(msg, location);
            }
            TerminatorKind::Drop { place, target: _, unwind: _ } => {
                self.visit_place(place, PlaceContext::MUTATING, location);
            }
            TerminatorKind::Call { func, args, destination, target: _, unwind: _ } => {
                self.visit_operand(func, location);
                for arg in args {
                    self.visit_operand(arg, location);
                }
                self.visit_place(destination, PlaceContext::MUTATING, location);
            }
            TerminatorKind::InlineAsm { operands, .. } => {
                for op in operands {
                    let InlineAsmOperand { in_value, out_place, raw_rpr: _ } = op;
                    if let Some(input) = in_value {
                        self.visit_operand(input, location);
                    }
                    if let Some(output) = out_place {
                        self.visit_place(output, PlaceContext::MUTATING, location);
                    }
                }
            }
            TerminatorKind::Return => {
                let local = RETURN_LOCAL;
                self.visit_local(&local, PlaceContext::NON_MUTATING, location);
            }
            TerminatorKind::SwitchInt { discr, targets: _ } => {
                self.visit_operand(discr, location);
            }
        }
    }

    fn super_span(&mut self, span: &Span) {
        let _ = span;
    }

    fn super_place(&mut self, place: &Place, ptx: PlaceContext, location: Location) {
        let _ = location;
        let _ = ptx;
        self.visit_local(&place.local, ptx, location);

        for (idx, elem) in place.projection.iter().enumerate() {
            let place_ref = PlaceRef { local: place.local, projection: &place.projection[..idx] };
            self.visit_projection_elem(place_ref, elem, ptx, location);
        }
    }

    fn super_projection_elem(
        &mut self,
        elem: &ProjectionElem,
        ptx: PlaceContext,
        location: Location,
    ) {
        match elem {
            ProjectionElem::Deref => {}
            ProjectionElem::Field(_idx, ty) => self.visit_ty(ty, location),
            ProjectionElem::Index(local) => self.visit_local(local, ptx, location),
            ProjectionElem::ConstantIndex { offset: _, min_length: _, from_end: _ } => {}
            ProjectionElem::Subslice { from: _, to: _, from_end: _ } => {}
            ProjectionElem::Downcast(_idx) => {}
            ProjectionElem::OpaqueCast(ty) => self.visit_ty(ty, location),
            ProjectionElem::Subtype(ty) => self.visit_ty(ty, location),
        }
    }

    fn super_rvalue(&mut self, rvalue: &Rvalue, location: Location) {
        match rvalue {
            Rvalue::AddressOf(mutability, place) => {
                let pcx = PlaceContext { is_mut: *mutability == Mutability::Mut };
                self.visit_place(place, pcx, location);
            }
            Rvalue::Aggregate(_, operands) => {
                for op in operands {
                    self.visit_operand(op, location);
                }
            }
            Rvalue::BinaryOp(_, lhs, rhs) | Rvalue::CheckedBinaryOp(_, lhs, rhs) => {
                self.visit_operand(lhs, location);
                self.visit_operand(rhs, location);
            }
            Rvalue::Cast(_, op, ty) => {
                self.visit_operand(op, location);
                self.visit_ty(ty, location);
            }
            Rvalue::CopyForDeref(place) | Rvalue::Discriminant(place) | Rvalue::Len(place) => {
                self.visit_place(place, PlaceContext::NON_MUTATING, location);
            }
            Rvalue::Ref(region, kind, place) => {
                self.visit_region(region, location);
                let pcx = PlaceContext { is_mut: matches!(kind, BorrowKind::Mut { .. }) };
                self.visit_place(place, pcx, location);
            }
            Rvalue::Repeat(op, constant) => {
                self.visit_operand(op, location);
                self.visit_const(constant, location);
            }
            Rvalue::ShallowInitBox(op, ty) => {
                self.visit_ty(ty, location);
                self.visit_operand(op, location)
            }
            Rvalue::ThreadLocalRef(_) => {}
            Rvalue::NullaryOp(_, ty) => {
                self.visit_ty(ty, location);
            }
            Rvalue::UnaryOp(_, op) | Rvalue::Use(op) => {
                self.visit_operand(op, location);
            }
        }
    }

    fn super_operand(&mut self, operand: &Operand, location: Location) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.visit_place(place, PlaceContext::NON_MUTATING, location)
            }
            Operand::Constant(constant) => {
                self.visit_constant(constant, location);
            }
        }
    }

    fn super_user_type_projection(&mut self, projection: &UserTypeProjection) {
        // This is a no-op on mir::Visitor.
        let _ = projection;
    }

    fn super_ty(&mut self, ty: &Ty) {
        let _ = ty;
    }

    fn super_ty_kind(&mut self, kind: &TyKind, location: Location) {
        let _ = kind;
        match kind {
            TyKind::RigidTy(rigid_ty) => self.visit_rigid_ty(rigid_ty, location),
            TyKind::Alias(kind, ty) => self.visit_alias_ty(kind, ty, location), // Best focus 
            TyKind::Param(param_ty) => self.visit_param_ty(param_ty),
            TyKind::Bound(debruijn, bound_ty) => self.visit_bound_ty(debruijn, bound_ty),
        }
    }

    fn super_binder<T>(&mut self, binder: &Binder<T>) {
        // TODO: binder.value: T

        for bound_var in &binder.bound_vars {
            match bound_var {
                crate::ty::BoundVariableKind::Ty(_ty_kind) => {},
                crate::ty::BoundVariableKind::Region(_region_kind) => {},
                crate::ty::BoundVariableKind::Const => {},
            }
        } 
    }

    fn super_constant(&mut self, constant: &Constant, location: Location) {
        let Constant { span, user_ty: _, literal } = constant;
        self.visit_span(span);
        self.visit_const(literal, location);
    }

    fn super_const(&mut self, constant: &Const, location: Location) {
        let Const { kind: _, ty, id: _ } = constant;
        self.visit_ty(ty, location);
    }

    fn super_region(&mut self, region: &Region) {
        let _ = region;
    }

    fn super_args(&mut self, args: &GenericArgs) {
        let _ = args;
    }

    fn super_var_debug_info(&mut self, var_debug_info: &VarDebugInfo) {
        let VarDebugInfo { source_info, composite, value, name: _, argument_index: _ } =
            var_debug_info;
        self.visit_span(&source_info.span);
        let location = Location(source_info.span);
        if let Some(composite) = composite {
            self.visit_ty(&composite.ty, location);
        }
        match value {
            VarDebugInfoContents::Place(place) => {
                self.visit_place(place, PlaceContext::NON_USE, location);
            }
            VarDebugInfoContents::Const(constant) => {
                self.visit_const(&constant.const_, location);
            }
        }
    }

    fn super_assert_msg(&mut self, msg: &AssertMessage, location: Location) {
        match msg {
            AssertMessage::BoundsCheck { len, index } => {
                self.visit_operand(len, location);
                self.visit_operand(index, location);
            }
            AssertMessage::Overflow(_, left, right) => {
                self.visit_operand(left, location);
                self.visit_operand(right, location);
            }
            AssertMessage::OverflowNeg(op)
            | AssertMessage::DivisionByZero(op)
            | AssertMessage::RemainderByZero(op) => {
                self.visit_operand(op, location);
            }
            AssertMessage::ResumedAfterReturn(_) | AssertMessage::ResumedAfterPanic(_) => { //nothing to visit
            }
            AssertMessage::MisalignedPointerDereference { required, found } => {
                self.visit_operand(required, location);
                self.visit_operand(found, location);
            }
        }
    }

    // RigidTy
    fn visit_rigid_ty(&mut self, rigid_ty: &RigidTy, location: Location) {
        self.super_rigid_ty(rigid_ty, location)
    }

    fn super_rigid_ty(&mut self, rigid_ty: &RigidTy, location: Location) {
        let _ = rigid_ty;
        match rigid_ty {
            RigidTy::Bool => self.visit_bool(rigid_ty),
            RigidTy::Char => self.visit_char(rigid_ty),
            RigidTy::Int(int) => self.visit_int(int),
            RigidTy::Uint(uint) => self.visit_uint(uint),
            RigidTy::Float(float) => self.visit_float(float),
            RigidTy::Adt(def, args) => self.visit_adt(def, args), // Tricky (remember recursive case)
            RigidTy::Foreign(def) => self.visit_foreign(def),
            RigidTy::Str => self.visit_str(rigid_ty),
            RigidTy::Array(ty, constant) => self.visit_array(ty, constant),
            RigidTy::Pat(ty, pattern) => self.visit_pat(ty, pattern),
            RigidTy::Slice(ty) => self.visit_slice(ty),
            RigidTy::RawPtr(ty, mutability) => self.visit_raw_ptr(ty, mutability),
            RigidTy::Ref(region, ty, mutability) => self.visit_ref(region, ty, mutability),
            RigidTy::FnDef(def, args) => self.visit_fn_def(def, args),
            RigidTy::FnPtr(sig) => self.visit_fn_ptr(sig),
            RigidTy::Closure(def, args) => self.visit_closure(def, args),
            RigidTy::Coroutine(def, args, movability) => self.visit_coroutine(def, args, movability),
            RigidTy::Dynamic(binders, region, kind) => self.visit_dynamic(binders, region, kind, location),
            RigidTy::Never => self.visit_never(),
            RigidTy::Tuple(tys) => self.visit_tuple(tys),
            RigidTy::CoroutineWitness(def, args) => self.visit_coroutine_witness(def, args),
        }
    }

    fn visit_bool(&mut self, bool: &RigidTy) { // TODO: RigidTy::Bool
        let _ = bool;
    }

    fn visit_char(&mut self, char: &RigidTy) { // TODO: RigidTy::Char
        let _ = char;
    }

    fn visit_int(&mut self, int: &IntTy) {
        let _ = int;
    }

    fn visit_uint(&mut self, uint: &UintTy) {
        let _ = uint;
    }

    fn visit_float(&mut self, float: &FloatTy) {
        let _ = float;
    }

    fn visit_adt(&mut self, def: &AdtDef, args: &GenericArgs) {
        self.super_adt(def, args)
    }

    fn visit_foreign(&mut self, def: &ForeignDef) {
        let _ = def;
    }

    fn visit_str(&mut self, str: &RigidTy) {
        let _ = str;
    }

    fn visit_array(&mut self, ty: &Ty, constant: &Const) {
        self.super_array(ty, constant);
    }

    fn visit_pat(&mut self, ty: &Ty, pattern: &Pattern) {
        self.super_pat(ty, pattern)
    }

    fn visit_slice(&mut self, ty: &Ty) {
        self.super_slice(ty)
    }

    fn visit_raw_ptr(&mut self, ty: &Ty, mutability: &Mutability) {
        self.super_raw_ptr(ty, mutability)
    }

    fn visit_ref(&mut self, region: &Region, ty: &Ty, mutability: &Mutability) {
        self.super_ref(region, ty, mutability)
    }

    fn visit_fn_def(&mut self, def: &FnDef, args: &GenericArgs) {
        self.super_fn_def(def, args)
    }

    fn visit_fn_ptr(&mut self, sig: &PolyFnSig) {
        self.super_fn_ptr(sig)
    }

    fn visit_closure(&mut self, def: &ClosureDef, args: &GenericArgs) {
        self.super_closure(def, args)
    }

    fn visit_coroutine(&mut self, def: &CoroutineDef, args: &GenericArgs, movability: &Movability) {
        self.super_coroutine(def, args, movability)
    }

    fn visit_dynamic(&mut self, binders: &Vec<Binder<ExistentialPredicate>>, region: &Region, kind: &DynKind, location: Location) {
        self.super_dynamic(binders, region, kind, location)
    }

    fn visit_never(&mut self) { }

    fn visit_tuple(&mut self, tys: &Vec<Ty>) {
        self.super_tuple(tys)
    }

    fn visit_coroutine_witness(&mut self, def: &CoroutineWitnessDef, args: &GenericArgs) {
        self.super_coroutine_witness(def, args)
    }

    fn super_adt(&mut self, def: &AdtDef, args: &GenericArgs) {
        let _ = def;
        let _ = args;
        // todo!()
    }

    fn super_array(&mut self, ty: &Ty, constant: &Const) {
        let _ = ty;
        let _ = constant;
        // todo!()
    }

    fn super_pat(&mut self, ty: &Ty, pattern: &Pattern) {
        let _ = ty;
        let _ = pattern;
        todo!()
    }

    fn super_slice(&mut self, ty: &Ty) {
        let _ = ty;
        todo!()
    }

    fn super_raw_ptr(&mut self, ty: &Ty, mutability: &Mutability) {
        let _ = ty;
        let _ = mutability;
        // todo!()
    }

    fn super_ref(&mut self, region: &Region, ty: &Ty, mutability: &Mutability) {
        let _ = region;
        let _ = ty;
        let _ = mutability;
        // todo!()
    }

    fn super_fn_def(&mut self, def: &FnDef, args: &GenericArgs) {
        let _ = def;
        let _ = args;
        // todo!()
    }

    fn super_fn_ptr(&mut self, sig: &PolyFnSig) {
        let _ = sig;
        todo!()
    }

    fn super_closure(&mut self, def: &ClosureDef, args: &GenericArgs) {
        let _ = def;
        let _ = args;
        todo!()
    }

    fn super_coroutine(&mut self, def: &CoroutineDef, args: &GenericArgs, movability: &Movability) {
        let _ = def;
        let _ = args;
        let _ = movability;
        todo!()
    }

    fn super_dynamic(&mut self, binders: &Vec<Binder<ExistentialPredicate>>, region: &Region, kind: &DynKind, location: Location) {
        let _ = kind; // kind:&DynKind
        for binder in binders {
            self.visit_binder(binder);
        }
        self.visit_region(region, location);
    }

    fn super_tuple(&mut self, tys: &Vec<Ty>) {
        let _ = tys;
        // todo!()
    }

    fn super_coroutine_witness(&mut self, def: &CoroutineWitnessDef, args: &GenericArgs){
        let _ = def;
        let _ = args;
        todo!()
    }

    // Alias
    fn visit_alias_ty(&mut self, kind: &AliasKind, ty: &AliasTy, location: Location) {
        self.super_alias_ty(kind, ty, location)
    }

    fn visit_alias_projection(&mut self, ty: &AliasTy) {
        self.super_alias_projection(ty)
    }

    fn visit_alias_inherent(&mut self, ty: &AliasTy) {
        self.super_alias_inherent(ty)
    }

    fn visit_alias_opaque(&mut self, ty: &AliasTy) {
        self.super_alias_opaque(ty)
    }

    fn visit_alias_weak(&mut self, ty: &AliasTy) {
        self.super_alias_weak(ty)
    }

    fn super_alias_ty(&mut self, kind: &AliasKind, ty: &AliasTy, location: Location) {
        let _ = location;

        match kind {
            AliasKind::Projection => self.visit_alias_projection(ty),
            AliasKind::Inherent => self.visit_alias_inherent(ty),
            AliasKind::Opaque => self.visit_alias_opaque(ty),
            AliasKind::Weak => self.visit_alias_weak(ty),
        }
    }

    fn super_alias_projection(&mut self, ty: &AliasTy) {
        let _ = ty;
        todo!()
    }

    fn super_alias_inherent(&mut self, ty: &AliasTy) {
        let _ = ty;
        todo!()
    }
    fn super_alias_opaque(&mut self, ty: &AliasTy) {
        let _ = ty;
        todo!()
    }

    fn super_alias_weak(&mut self, ty: &AliasTy) {
        let _ = ty;
        todo!()
    }

    // Param
    fn visit_param_ty(&mut self, param_ty: &ParamTy) {
        self.super_param_ty(param_ty)
    }

    fn super_param_ty(&mut self, param_ty: &ParamTy) {
        let ParamTy {index, name} = param_ty;
        let _ = index;
        let _ = name;
    }

    // Bound
    fn visit_bound_ty(&mut self, debruijn: &usize, bound_ty: &BoundTy) {
        self.super_bound_ty(debruijn, bound_ty)
    }

    fn visit_bound_ty_kind(&mut self, kind: &BoundTyKind) {
        self.super_bound_ty_kind(kind)
    }

    fn visit_bound_ty_anon(&mut self) {}

    fn visit_bound_ty_param(&mut self, def: &ParamDef, symbol: &String) {
        self.super_bound_ty_param(def, symbol)
    }

    fn super_bound_ty(&mut self, debruijn: &usize, bound_ty: &BoundTy) {
        let _ = debruijn;
        let BoundTy {var, kind} = bound_ty;
        let _ = var;
        self.visit_bound_ty_kind(kind)
    }

    fn super_bound_ty_kind(&mut self, kind: &BoundTyKind) {
        match kind {
            BoundTyKind::Anon => self.visit_bound_ty_anon(),
            BoundTyKind::Param(def, symbol) => self.visit_bound_ty_param(def, symbol),
        }
    }

    fn super_bound_ty_param(&mut self, def: &ParamDef, symbol: &String) {
        let _ = def;
        let _ = symbol;
    }
}

/// This function is a no-op that gets used to ensure this visitor is kept up-to-date.
///
/// The idea is that whenever we replace an Opaque type by a real type, the compiler will fail
/// when trying to invoke `visit_opaque`.
///
/// If you are here because your compilation is broken, replace the failing call to `visit_opaque()`
/// by a `visit_<CONSTRUCT>` for your construct.
fn visit_opaque(_: &Opaque) {}

/// The location of a statement / terminator in the code and the CFG.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Location(Span);

impl Location {
    pub fn span(&self) -> Span {
        self.0
    }
}

/// Reference to a place used to represent a partial projection.
pub struct PlaceRef<'a> {
    pub local: Local,
    pub projection: &'a [ProjectionElem],
}

impl<'a> PlaceRef<'a> {
    /// Get the type of this place.
    pub fn ty(&self, locals: &[LocalDecl]) -> Result<Ty, Error> {
        self.projection.iter().fold(Ok(locals[self.local].ty), |place_ty, elem| elem.ty(place_ty?))
    }
}

/// Information about a place's usage.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaceContext {
    /// Whether the access is mutable or not. Keep this private so we can increment the type in a
    /// backward compatible manner.
    is_mut: bool,
}

impl PlaceContext {
    const MUTATING: Self = PlaceContext { is_mut: true };
    const NON_MUTATING: Self = PlaceContext { is_mut: false };
    const NON_USE: Self = PlaceContext { is_mut: false };

    pub fn is_mutating(&self) -> bool {
        self.is_mut
    }
}
