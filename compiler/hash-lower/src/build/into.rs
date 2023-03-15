//! Implementation for lowering [ast::Expr]s into Hash IR. This module contains
//! the core logic of converting expressions into IR, other auxiliary conversion
//! `strategies` can be found in [`crate::build::rvalue`] and
//! [crate::build::temp].

use hash_ir::{
    intrinsics::Intrinsic,
    ir::{
        self, AggregateKind, BasicBlock, Const, ConstKind, LogicalBinOp, Operand, Place, RValue,
        Statement, StatementKind, TerminatorKind, UnevaluatedConst,
    },
    ty::{AdtId, IrTy, Mutability, RefKind, VariantIdx},
};
use hash_reporting::macros::panic_on_span;
use hash_source::{
    constant::CONSTANT_MAP,
    identifier::{Identifier, IDENTS},
    location::Span,
};
use hash_tir::{
    args::ArgsId,
    arrays::ArrayTerm,
    atom_info::ItemInAtomInfo,
    control::{LoopControlTerm, ReturnTerm},
    data::CtorTerm,
    environment::{
        context::{BindingKind, Context},
        env::AccessToEnv,
    },
    fns::FnCallTerm,
    params::ParamIndex,
    refs::{self, RefTerm},
    scopes::AssignTerm,
    terms::{Term, TermId, UnsafeTerm},
    tuples::TupleTerm,
    ty_as_variant,
    utils::common::CommonUtils,
};
use hash_utils::store::{CloneStore, SequenceStore, SequenceStoreKey, Store};

use super::{
    ty::FnCallTermKind, unpack, BlockAnd, BlockAndExtend, Builder, LocalKey, LoopBlockInfo,
};

impl<'tcx> Builder<'tcx> {
    /// Compile the given [Term] and place the value of the [Term]
    /// into the specified destination [Place].
    pub(crate) fn term_into_dest(
        &mut self,
        destination: Place,
        mut block: BasicBlock,
        term_id: TermId,
    ) -> BlockAnd<()> {
        let term = self.stores().term().get(term_id);
        let span = self.span_of_term(term_id);

        let block_and = match &term {
            // // This includes `loop { ... } `, `{ ... }`, `match { ... }`
            Term::Block(_) | Term::Match(_) | Term::Loop(_) => {
                self.block_into_dest(destination, block, term_id)
            }

            Term::Tuple(TupleTerm { data }) => {
                let ty = self.ty_id_from_tir_term(term_id);
                let adt = self.ctx().map_ty_as_adt(ty, |_, id| id);
                let aggregate_kind = AggregateKind::Tuple(adt);

                let args = self.stores().args().map_fast(*data, |args| {
                    args.iter()
                        .map(|element| {
                            let name = match element.target {
                                ParamIndex::Name(name) => name,
                                ParamIndex::Position(pos) => pos.into(),
                            };

                            (name, element.value)
                        })
                        .collect::<Vec<_>>()
                });

                self.aggregate_into_dest(destination, block, aggregate_kind, &args, span)
            }
            Term::Lit(ref lit) => {
                // We lower primitive (integrals, strings, etc) literals as constants
                let constant = self.as_constant(lit);
                self.control_flow_graph.push_assign(block, destination, constant.into(), span);

                block.unit()
            }
            Term::Array(ArrayTerm { elements }) => {
                // We lower literal arrays and tuples as aggregates.
                let ty_id = self.ty_id_from_tir_term(term_id);
                let ty = self.ctx().tys().get(ty_id);

                let aggregate_kind = AggregateKind::Array(ty_id);
                let args = self.stores().term_list().map_fast(*elements, |elements| {
                    elements
                        .iter()
                        .enumerate()
                        .map(|(index, element)| (index.into(), *element))
                        .collect::<Vec<_>>()
                });

                // If it is a list, we have to initialise it with the array elements...
                if !ty.is_array() {
                    self.lower_list_initialisation(
                        destination,
                        block,
                        &ty,
                        aggregate_kind,
                        &args,
                        span,
                    )
                } else {
                    self.aggregate_into_dest(destination, block, aggregate_kind, &args, span)
                }
            }

            Term::Ctor(ref ctor) => {
                let id = self.ty_id_from_tir_term(term_id);
                let ty = self.ctx().tys().get(id);

                match ty {
                    IrTy::Adt(adt) => {
                        // This is a constructor call, so we need to handle it as such.
                        self.constructor_into_dest(destination, block, ctor, adt, span)
                    }
                    IrTy::Bool => {
                        // @@Hack: check which constructor is being called to determine whether
                        // it is a `true` or `false` value.
                        let constant =
                            if ctor.ctor.1 == 0 { Const::Bool(true) } else { Const::Bool(false) };

                        self.control_flow_graph.push_assign(
                            block,
                            destination,
                            constant.into(),
                            span,
                        );

                        block.unit()
                    }
                    _ => {
                        panic!("Expected an ADT type for the constructor");
                    }
                }
            }
            Term::FnCall(term @ FnCallTerm { subject, args, .. }) => {
                match self.classify_fn_call_term(term) {
                    FnCallTermKind::Call(_) => {
                        // Get the type of the function into or to to get the
                        // fn-type so that we can enter the scope.
                        let ty = self.get_inferred_ty(*subject);
                        let fn_ty = ty_as_variant!(self, self.get_ty(ty), Fn);

                        // Try and create the ir_type from a function definition, otherwise
                        // if it is just a function, then we make the the type from the function.

                        Context::enter_scope_mut(self, fn_ty.into(), |this| {
                            this.fn_call_into_dest(destination, block, *subject, *args, span)
                        })
                    }
                    FnCallTermKind::Cast(..)
                    | FnCallTermKind::UnaryOp(_, _)
                    | FnCallTermKind::BinaryOp(_, _, _) => {
                        let rvalue = unpack!(block = self.as_rvalue(block, term_id));
                        self.control_flow_graph.push_assign(block, destination, rvalue, span);
                        block.unit()
                    }

                    // We deal with logical
                    // binary expressions differently than other binary operators.
                    // In order to preserve the short-circuiting behaviour of
                    // these operators, we need to create the following schemes:
                    //
                    // AND operator:
                    // ```text
                    //  +-----+  true   +------------+
                    //  | lhs |-------->| dest = rhs |--+
                    //  +--+--+         +------------+  |
                    //     |                            |
                    //     | false                      |
                    //     v                            |
                    //  +--+-----------+                |   +------+
                    //  | dest = false |----------------+-->| join |
                    //  +--------------+                    +------+
                    // ```
                    //
                    // OR operator:
                    //
                    // ```text
                    //  +-----+  false  +------------+
                    //  | lhs |-------->| dest = rhs |--+
                    //  +--+--+         +------------+  |
                    //     |                            |
                    //     | true                       |
                    //     v                            |
                    //  +--+-----------+                |   +------+
                    //  | dest = true  |----------------+-->| join |
                    //  +--------------+                    +------+
                    // ```
                    FnCallTermKind::LogicalBinOp(op, _, _) => {
                        let (short_circuiting_block, mut else_block, join_block) = (
                            self.control_flow_graph.start_new_block(),
                            self.control_flow_graph.start_new_block(),
                            self.control_flow_graph.start_new_block(),
                        );

                        let lhs_term = self.stores().args().get_at_index(*args, 1).value;
                        let rhs_term = self.stores().args().get_at_index(*args, 2).value;

                        let lhs =
                            unpack!(block = self.as_operand(block, lhs_term, Mutability::Mutable));

                        let blocks = match op {
                            LogicalBinOp::And => (else_block, short_circuiting_block),
                            LogicalBinOp::Or => (short_circuiting_block, else_block),
                        };

                        let term = TerminatorKind::make_if(lhs, blocks.0, blocks.1, self.ctx());
                        self.control_flow_graph.terminate(block, span, term);

                        // Create the constant that we will assign in the `short_circuiting` block.
                        // let constant =
                        let constant = match op {
                            LogicalBinOp::And => Const::Bool(false),
                            LogicalBinOp::Or => Const::Bool(true),
                        };

                        self.control_flow_graph.push_assign(
                            short_circuiting_block,
                            destination,
                            constant.into(),
                            span,
                        );

                        // Join the branch to the joining block
                        self.control_flow_graph.goto(short_circuiting_block, join_block, span);

                        // Now deal with the non-short-circuiting block
                        let rhs = unpack!(
                            else_block = self.as_operand(else_block, rhs_term, Mutability::Mutable)
                        );

                        self.control_flow_graph.push_assign(
                            else_block,
                            destination,
                            rhs.into(),
                            span,
                        );
                        self.control_flow_graph.goto(else_block, join_block, span);

                        join_block.unit()
                    }
                }
            }
            Term::Var(symbol) => {
                let binding = self.context().get_binding(*symbol);

                // Here, if the scope is not variable, i.e. constant, then we essentially need
                // to denote that this a constant that comes from outside of the function body.
                if !matches!(binding.kind, BindingKind::Decl(..)) {
                    let name = self.get_symbol(binding.name).name.unwrap_or(IDENTS.underscore);

                    // here, we emit an un-evaluated constant kind which will be resolved later
                    // during IR simplification.
                    let unevaluated_const = UnevaluatedConst { name };
                    let rvalue = (ConstKind::Unevaluated(unevaluated_const)).into();

                    // we also need to save this un-evaluated const in the builder
                    // so we can easily know what should and shouldn't be resolved.
                    self.needed_constants.push(unevaluated_const);
                    self.control_flow_graph.push_assign(block, destination, rvalue, span);
                } else {
                    let local_key = LocalKey::from(binding.kind);
                    let local = *(self.declaration_map.get(&local_key).unwrap());

                    let place = Place::from_local(local, self.ctx());
                    self.control_flow_graph.push_assign(block, destination, place.into(), span);
                }

                block.unit()
            }
            Term::LoopControl(control) => {
                // Specify that we have reached the terminator of this block...
                self.reached_terminator = true;

                // When this is a continue, we need to **jump** back to the
                // start of the loop block, and when this is a break, we need to
                // **jump** to the proceeding block of the loop block
                let Some(LoopBlockInfo { loop_body, next_block }) = self.loop_block_info else {
                    panic_on_span!(
                        span.into_location(self.source_id),
                        self.env().source_map(),
                        "`continue` or `break` outside of loop"
                    );
                };

                // Add terminators to this block to specify where this block will jump...
                match control {
                    LoopControlTerm::Continue => {
                        self.control_flow_graph.goto(block, loop_body, span);
                    }
                    LoopControlTerm::Break => {
                        self.control_flow_graph.goto(block, next_block, span);
                    }
                }

                block.unit()
            }
            Term::Return(ReturnTerm { expression }) => {
                unpack!(
                    block =
                        self.term_into_dest(Place::return_place(self.ctx()), block, *expression)
                );

                // In either case, we want to mark that the function has reached the
                // **terminating** statement of this block and we needn't continue looking
                // for more statements beyond this point.
                self.reached_terminator = true;

                // Create a new block for the `return` statement and make this block
                // go to the return whilst also starting a new block.
                //
                // @@Note: during CFG simplification, this edge will be removed and unified with
                // the `exit` block.
                let return_block = self.control_flow_graph.make_return_block();
                self.control_flow_graph.goto(block, return_block, span);
                self.control_flow_graph.start_new_block().unit()
            }
            // For declarations, we have to perform some bookkeeping in regards
            // to locals..., but this expression should never return any value
            // so we should just return a unit block here
            Term::Decl(ref decl) => self.lower_declaration(block, decl, span),
            Term::Assign(_) => {
                // Deal with the actual assignment
                block = unpack!(self.lower_assign_term(block, &term, span));

                // Assign the `value` of the assignment into the `tmp_place`
                let const_value = ir::Const::zero(self.ctx());
                self.control_flow_graph.push_assign(block, destination, const_value.into(), span);

                block.unit()
            }
            Term::Unsafe(UnsafeTerm { inner }) => self.term_into_dest(destination, block, *inner),
            Term::Ref(RefTerm { kind, mutable, subject }) => {
                let mutability = if *mutable { Mutability::Mutable } else { Mutability::Immutable };

                // @@Todo: This is not the full picture here, if the user only specifies a
                // `Normal` ref, this still might become a `SmartRef` based on
                // the type of the expression, and where the expression comes
                // from.
                let kind = match kind {
                    refs::RefKind::Local => RefKind::Normal,
                    refs::RefKind::Raw => RefKind::Raw,
                    refs::RefKind::Rc => RefKind::Rc,
                };

                let place = unpack!(block = self.as_place(block, *subject, mutability));

                // Create an RValue for this reference
                let addr_of = RValue::Ref(mutability, place, kind);
                self.control_flow_graph.push_assign(block, destination, addr_of, span);
                block.unit()
            }
            Term::Index(_) | Term::Deref(_) | Term::Access(_) => {
                let place = unpack!(block = self.as_place(block, term_id, Mutability::Immutable));
                self.control_flow_graph.push_assign(block, destination, place.into(), span);

                block.unit()
            }

            Term::Cast(_) | Term::TypeOf(_) | Term::Ty(_) | Term::Hole(_) | Term::FnRef(_) => {
                block.unit()
            }
        };

        block_and
    }

    /// Convert a [`Term::Assign`] into IR by first lowering the right-hand side
    /// and then the left.
    pub(crate) fn lower_assign_term(
        &mut self,
        mut block: BasicBlock,
        statement: &Term,
        span: Span,
    ) -> BlockAnd<()> {
        let Term::Assign(AssignTerm { subject, value }) = statement else {
            unreachable!()
        };

        // Lower the subject and the value of the assignment in RTL
        // and then assign the value into the subject
        let value = unpack!(block = self.as_rvalue(block, *value));
        let place = unpack!(block = self.as_place(block, *subject, Mutability::Mutable));

        self.control_flow_graph.push_assign(block, place, value, span);
        block.unit()
    }

    /// Lower a function call and place the result into the provided
    /// destination.
    pub fn fn_call_into_dest(
        &mut self,
        destination: Place,
        mut block: BasicBlock,
        subject: TermId,
        args: ArgsId,
        span: Span,
    ) -> BlockAnd<()> {
        // First we want to lower the subject of the function call
        let func = unpack!(block = self.as_operand(block, subject, Mutability::Immutable));

        // lower the arguments of the function...
        //
        // @@Todo: we need to deal with default arguments here, we compute the missing
        // arguments, and then insert a lowered copy of the default value for
        // the argument.
        // if let IrTy::Fn { params, .. } = fn_ty {
        //     if args.len() != params.len() {
        //         panic_on_span!(
        //             span.into_location(self.source_id),
        //             self.env().source_map(),
        //             "default arguments on functions are not currently supported",
        //         );
        //     }
        // }

        let args = self
            .stores()
            .args()
            .get_vec(args)
            .iter()
            .map(|arg| unpack!(block = self.as_operand(block, arg.value, Mutability::Immutable)))
            .collect::<Vec<_>>();

        self.build_fn_call(destination, block, func, args, span)
    }

    /// Build a function call from the provided subject and arguments. This
    /// function simply terminates the current [BasicBlock] with a
    /// [`TerminatorKind::Call`] and returns the block that is used for the
    /// `success` case.
    pub fn build_fn_call(
        &mut self,
        destination: Place,
        block: BasicBlock,
        subject: Operand,
        args: Vec<Operand>,
        span: Span,
    ) -> BlockAnd<()> {
        // This is the block that is used when resuming from the function..
        let success = self.control_flow_graph.start_new_block();

        // Terminate the current block with a `Call` terminator
        self.control_flow_graph.terminate(
            block,
            span,
            TerminatorKind::Call { op: subject, args, destination, target: Some(success) },
        );

        success.unit()
    }

    /// Function that deals with lowering a constructor call which might involve
    /// either a `struct` or an `enum` constructor. This function constructs an
    /// [RValue::Aggregate] and assigns it to the specified destination.
    ///
    /// However, due to the fact that we haven't decided whether it is easier to
    /// deal with aggregate values or direct fields assignments, we might have
    /// to end up de-aggregating the aggregate values into a series of
    /// assignments as they are specified within their declaration order.
    pub fn constructor_into_dest(
        &mut self,
        destination: Place,
        block: BasicBlock,
        subject: &CtorTerm,
        adt_id: AdtId,
        span: Span,
    ) -> BlockAnd<()> {
        let CtorTerm { ctor, ctor_args, .. } = subject;

        let aggregate_kind = self.ctx().adts().map_fast(adt_id, |adt| {
            if adt.flags.is_enum() || adt.flags.is_union() {
                AggregateKind::Enum(adt_id, VariantIdx::from_usize(ctor.1))
            } else {
                debug_assert!(adt.flags.is_struct());
                AggregateKind::Struct(adt_id)
            }
        });

        // If a subject is an enum with a specified variant, we then need
        // to "discriminate the subject"
        if let AggregateKind::Enum(_, index) = aggregate_kind {
            self.control_flow_graph.push(
                block,
                Statement { kind: StatementKind::Discriminate(destination, index), span },
            );

            // We don't need to do anything else if it is just the discriminant.
            if ctor_args.len() == 0 {
                return block.unit();
            }
        }

        let args = self.stores().args().map_fast(*ctor_args, |args| {
            args.iter()
                .map(|arg| {
                    let name = match arg.target {
                        ParamIndex::Name(name) => name,
                        ParamIndex::Position(pos) => pos.into(),
                    };

                    (name, arg.value)
                })
                .collect::<Vec<_>>()
        });

        self.aggregate_into_dest(destination, block, aggregate_kind, &args, span)
    }

    /// Place any aggregate value into the specified destination. This does not
    /// currently deal with default arguments to a specified ADT, so it will
    /// panic if the number of arguments provided is not equal to the number of
    /// fields in the ADT.
    fn aggregate_into_dest(
        &mut self,
        destination: Place,
        mut block: BasicBlock,
        aggregate_kind: AggregateKind,
        args: &[(Identifier, TermId)],
        span: Span,
    ) -> BlockAnd<()> {
        // We don't need to perform this check for arrays since they don't need
        // to have a specific amount of arguments to the constructor.
        let fields: Vec<_> = if aggregate_kind.is_adt() {
            let adt_id = aggregate_kind.adt_id();

            // We have to evaluate each field in the specified source
            // order despite the aggregate potentially having a different
            // field order.
            let mut field_map = Vec::with_capacity(args.len());
            let mut field_names = Vec::with_capacity(args.len());

            // @@Todo: deal with the situation where we need to fill in default
            //  values for various parameters. For now, we ensure that all
            //  values are specified for the particular definition, and ensure
            // that the provided fields are equal. When we do add support for
            // default field values, it should be that the type checker
            // emits information about what fields need to be added to this
            // aggregate value.
            for (name, field) in args {
                let value = unpack!(block = self.as_operand(block, *field, Mutability::Immutable));
                field_map.push(value);
                field_names.push(*name);
            }

            self.ctx().adts().map_fast(adt_id, |adt| {
                let variant = if let AggregateKind::Enum(_, index) = aggregate_kind {
                    &adt.variants[index]
                } else {
                    &adt.variants[0]
                };

                let field_count = variant.fields.len();

                // Ensure we have the exact amount of arguments as the definition expects.
                if args.len() != field_count {
                    panic_on_span!(
                        span.into_location(self.source_id),
                        self.source_map(),
                        "default arguments on constructors are not currently supported",
                    );
                }

                // now we re-order the field_map in the "expected" order
                // of the aggregate initialisation...
                for (index, name) in field_names.iter().enumerate() {
                    let field_index = variant.field_idx(*name).unwrap();

                    if field_index == index {
                        continue;
                    }

                    field_map.swap(index, field_index);
                }

                field_map
            })
        } else {
            // This aggregate kind is an array, so all that we do is just
            // lower each of the arguments into the array.
            args.iter()
                .map(|(_, arg)| {
                    unpack!(block = self.as_operand(block, *arg, Mutability::Immutable))
                })
                .collect()
        };

        let aggregate = RValue::Aggregate(aggregate_kind, fields);
        self.control_flow_graph.push_assign(block, destination, aggregate, span);

        block.unit()
    }

    /// This function will generate the necessary code to initialise a slice on
    /// the stack. A slice differs from an array by the fact that it is an
    /// aggregate value that contains the length of the slice, and the
    /// pointer to the data itself.
    ///
    /// The procedure to initialise the list is as follows:
    ///
    /// 1. Allocate enough bytes for the literal `[T]` by computing
    ///    `size_of(T)` * `len(elements)`.
    ///
    /// 2. Write the bytes to the allocation.
    ///
    /// 3. Create a `SizedPtr(ref, len)` from the pointer that we get
    ///    from the call to malloc and assign it to a temporary.
    ///
    /// 4. Transmute the `SizedPointer` into a `&[T]` and assign it to the
    ///    destination.
    ///
    /// For example, the following snippet:
    /// ```ignore
    /// t := [1, 2, 3];
    /// ```
    /// Is lowered into the following sequence:
    /// ```ignore
    /// _1: &[i32]; // parameter `t`
    /// _2: &raw u8;
    /// _3: &[i32; 4];
    /// _4: SizedPointer;
    /// _5: ();
    ///
    /// bb0 {
    ///     _2 = malloc(const 16_u64) -> bb1;
    /// }
    ///
    /// bb1 {
    ///     _3 = _2;
    ///     (*_3) = [const 1_i32, const 2_i32, const 3_i32, const 5_i32];
    ///     _4 = SizedPointer(_3, const 4_u64);
    ///     _1 = transmute((), (), _4) -> bb2;
    /// }
    /// ```
    ///
    /// @@Future: this should probably be done within the language rather
    /// than the build initialising the list like this.
    fn lower_list_initialisation(
        &mut self,
        destination: Place,
        mut block: BasicBlock,
        ty: &IrTy,
        aggregate_kind: AggregateKind,
        args: &[(Identifier, TermId)],
        span: Span,
    ) -> BlockAnd<()> {
        let ptr_width = self.settings.target().ptr_size();
        let element_ty = ty.element_ty(self.ctx()).unwrap();
        let size = self.ctx.size_of(element_ty).unwrap() * args.len();
        let const_size = CONSTANT_MAP.create_usize_int(size, ptr_width);
        let size_op = Operand::Const(Const::Int(const_size).into());

        // find the `malloc` function which is defined in the prelude
        // and within the `libc` module
        let item = self.lookup_libc_fn("malloc").expect("`malloc` not found");
        let subject = Operand::Const(Const::Zero(item).into());

        // 1).
        //
        // Make the call to `malloc`, and then assign the result to a
        // temporary.
        let ptr = self.temp_place(self.ctx().tys().common_tys.raw_ptr);
        unpack!(block = self.build_fn_call(ptr, block, subject, vec![size_op], span));

        // we make a new temporary which is a pointer to the array and assign `ptr`
        // to it.
        let ty = IrTy::make_ref(IrTy::Array { ty: element_ty, length: args.len() }, self.ctx());
        let array_ptr = self.temp_place(self.ctx().tys().create(ty));
        self.control_flow_graph.push_assign(block, array_ptr, Operand::Place(ptr).into(), span);

        // 2). Write data to allocation.
        self.aggregate_into_dest(array_ptr.deref(self.ctx()), block, aggregate_kind, args, span);

        // 3).
        //
        // Create the SizedPointer from the received pointer and length.
        let sized_ptr_ty =
            self.lookup_prelude_item("SizedPointer").expect("`SizedPointer` not found");
        let sized_ptr = self.temp_place(sized_ptr_ty);
        let value =
            self.create_ptr_with_metadata(sized_ptr_ty, Operand::Place(array_ptr), args.len());

        self.control_flow_graph.push_assign(block, sized_ptr, value, span);

        // Finally, transmute the SizedPointer into a `&[T]` and assign it to the
        // destination.
        let transmute_fn = self.ctx().intrinsics().get_ty(Intrinsic::Transmute).unwrap();
        let subject = Operand::Const(Const::Zero(transmute_fn).into());

        unpack!(
            block = self.build_fn_call(
                destination,
                block,
                subject,
                // The first two arguments are the fill-ins for the generic parameters.
                vec![
                    Operand::Const(Const::Zero(self.ctx().tys().common_tys.unit).into()),
                    Operand::Const(Const::Zero(self.ctx().tys().common_tys.unit).into()),
                    Operand::Place(sized_ptr)
                ],
                span
            )
        );

        block.unit()
    }
}
