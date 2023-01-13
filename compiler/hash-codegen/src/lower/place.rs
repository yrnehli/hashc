//! Defines logic for lowering Hash IR places into the target backend
//! IR.

use hash_ir::{
    ir,
    ty::{IrTy, PlaceTy, VariantIdx},
};
use hash_layout::{LayoutShape, Variants};
use hash_target::{
    abi::{AbiRepresentation, ScalarKind},
    alignment::Alignment,
};
use hash_utils::store::{SequenceStore, Store};

use super::{locals::LocalRef, FnBuilder};
use crate::{
    layout::TyInfo,
    traits::{
        builder::BlockBuilderMethods, constants::BuildConstValueMethods, ctx::HasCtxMethods,
        ty::BuildTypeMethods, CodeGenObject,
    },
};

/// A [PlaceRef] is the equivalent of an IR [Place], but within the code
/// generation context. The place holds a value that is backend dependent, type,
/// layout, and alignment information
#[derive(Debug, Clone, Copy)]
pub struct PlaceRef<V> {
    /// The value of the place.
    pub value: V,

    /// The type and layout of the value.
    pub info: TyInfo,

    // The alignment of the value.
    pub alignment: Alignment,
}

impl<'b, V: CodeGenObject> PlaceRef<V> {
    /// Create a new [PlaceRef] from an existant value.
    pub fn new<Builder: BlockBuilderMethods<'b, Value = V>>(
        builder: &mut Builder,
        value: V,
        info: TyInfo,
    ) -> Self {
        let layout = builder.ctx().layout_info(info.layout);
        let alignment = layout.alignment.abi;

        Self { value, info, alignment }
    }

    /// Create a new [PlaceRef] which refers to a value allocated on the
    /// function stack.
    pub fn new_stack<Builder: BlockBuilderMethods<'b, Value = V>>(
        builder: &mut Builder,
        info: TyInfo,
    ) -> Self {
        let layout = builder.ctx().layout_info(info.layout);
        let alignment = layout.alignment.abi;

        let temp = builder.alloca(builder.ctx().backend_type(info), alignment);

        Self::new(builder, temp, info)
    }

    /// Given that the underlying [PlaceRef] refers to an array
    /// being stored on the stack, we lookup the layout of the
    /// array and access the `size` stored on it to get the
    /// `len` of the place.
    pub fn len<Builder: BlockBuilderMethods<'b, Value = V>>(&self, builder: &Builder) -> V {
        let layout = builder.ctx().layout_info(self.info.layout);

        if let LayoutShape::Array { elements, .. } = layout.shape {
            builder.const_usize(elements)
        } else {
            panic!("PlaceRef::len called on non-array type");
        }
    }
}

impl<'b, V: CodeGenObject> PlaceRef<V> {
    /// Apply a "discriminant" onto the [PlaceRef].
    pub fn codegen_set_discriminant<Builder: BlockBuilderMethods<'b, Value = V>>(
        &self,
        builder: &mut Builder,
        discriminant: VariantIdx,
    ) {
        let variant_info = self.info.for_variant(builder.layout_ctx(), discriminant);
        let variant_layout = builder.layout_info(variant_info.layout);

        // If an attempt is made to set the discriminant for a variant type
        // that is un-inhabited, this is a panic.
        if variant_layout.abi.is_uninhabited() {
            builder.codegen_abort_intrinsic();
            return;
        }

        match variant_layout.variants {
            Variants::Single { index } => {
                debug_assert_eq!(index, discriminant);
            }
            Variants::Multiple { field, .. } => {
                let ptr = self.project_field(builder, field);
                let (_, value) = builder.ir_ctx().tys().map_fast(self.info.ty, |ty| {
                    ty.discriminant_for_variant(builder.ir_ctx(), discriminant).unwrap()
                });

                builder.store(
                    builder.const_uint_big(builder.backend_type(ptr.info), value),
                    ptr.value,
                    ptr.alignment,
                );
            }
        }
    }

    /// Get the "discriminant" of the [PlaceRef] and cast it
    /// to a specified type (which must be an integer type).
    pub fn codegen_get_discriminant<Builder: BlockBuilderMethods<'b, Value = V>>(
        self,
        builder: &mut Builder,
        cast_to: IrTy,
    ) -> V {
        let cast_info = builder.layout_of(cast_to);
        let cast_to_ty = builder.immediate_backend_type(cast_info);

        let (variants, is_uninhabited) = builder.map_layout(self.info.layout, |layout| {
            (layout.variants.clone(), layout.abi.is_uninhabited())
        });

        // Check if this place is represented as "uninhabited" then we
        // simply set the result of this as an undefined value of the `cast_to`
        // type...
        if is_uninhabited {
            return builder.const_undef(cast_to_ty);
        }

        match variants {
            Variants::Single { index } => {
                let value = builder.ir_ctx().tys().map_fast(self.info.ty, |ty| {
                    ty.discriminant_for_variant(builder.ir_ctx(), index)
                        .map_or(index.raw() as u128, |(_, value)| value)
                });

                builder.const_uint_big(cast_to_ty, value)
            }
            Variants::Multiple { field, tag, .. } => {
                let tag_ptr = self.project_field(builder, field);
                let tag_operand = builder.load_operand(tag_ptr);
                let tag_immediate = tag_operand.immediate_value();

                // We use `i1` for bytes that only have a valid range of
                // `0` or `1`, but it shouldn't interpret the `i1` as signed
                // because the `1_i1` would then actually be `-1_i8`.
                let signed = match tag.kind() {
                    ScalarKind::Int { signed, .. } => !tag.is_bool() && signed,
                    _ => false,
                };

                builder.int_cast(tag_immediate, cast_to_ty, signed)
            }
        }
    }

    /// Apply a downcasting (selecting an `enum` variant on a place) projection
    /// onto the [PlaceRef].
    pub fn project_downcast<Builder: BlockBuilderMethods<'b, Value = V>>(
        &self,
        builder: &mut Builder,
        variant: VariantIdx,
    ) -> Self {
        let mut downcast = *self;
        downcast.info = self.info.for_variant(builder.layout_ctx(), variant);

        // Cast the downcast value to the appropriate type
        let variant_ty = builder.backend_type(downcast.info);
        downcast.value = builder.pointer_cast(downcast.value, builder.type_ptr_to(variant_ty));
        downcast
    }

    /// Apply a indexing projection onto the [PlaceRef].
    pub fn project_index<Builder: BlockBuilderMethods<'b, Value = V>>(
        &self,
        builder: &mut Builder,
        index: V,
    ) -> Self {
        // compute the offset if possible, or just use the element
        // size as it will yield the lowest alignment.
        let field_info = self.info.field(builder.layout_ctx(), 0);
        let field_size = builder.map_layout(field_info.layout, |layout| layout.size);

        let offset = if let Some(index) = builder.const_to_optional_uint(index) {
            field_size.checked_mul(index, builder).unwrap_or(field_size)
        } else {
            field_size
        };

        Self {
            value: builder.bounded_get_element_pointer(
                builder.backend_type(self.info),
                self.value,
                &[builder.const_usize(0), index],
            ),
            info: field_info,
            alignment: self.alignment.restrict_to(offset),
        }
    }

    /// Apply a field projection on a [PlaceRef].
    pub fn project_field<Builder: BlockBuilderMethods<'b, Value = V>>(
        &self,
        builder: &mut Builder,
        field: usize,
    ) -> Self {
        let abi = builder.map_layout(self.info.layout, |layout| layout.abi);

        let field_info = self.info.field(builder.layout_ctx(), field);
        let (field_offset, is_zst) = builder
            .map_layout(field_info.layout, |layout| (layout.shape.offset(field), layout.is_zst()));

        let field_alignment = self.alignment.restrict_to(field_offset);

        let value = match abi {
            _ if field_offset.bytes() == 0 => self.value,

            // If the offset matches the second field, then we can
            // just get the `get_element_ptr` of the second field
            AbiRepresentation::Pair(scalar_a, scalar_b)
                if field_offset == scalar_a.size(builder).align_to(scalar_b.align(builder).abi) =>
            {
                let ty = builder.backend_type(self.info);
                builder.structural_get_element_pointer(ty, self.value, 1)
            }
            AbiRepresentation::Scalar(_)
            | AbiRepresentation::Pair(..)
            | AbiRepresentation::Vector { .. }
                if is_zst =>
            {
                // If this is a zst field, we have to manually offset the pointer.
                let byte_ptr = builder.pointer_cast(self.value, builder.type_i8p());
                builder.get_element_pointer(
                    builder.type_i8(),
                    byte_ptr,
                    &[builder.const_usize(field_offset.bytes())],
                )
            }
            AbiRepresentation::Scalar(_) | AbiRepresentation::Pair(..) => {
                // @@Todo: implement `ForFormatting` equivalent for `info` and `layout`.
                panic!(
                    "offset of non-ZST field `{:?}` which does not match `{:?}`",
                    field_info, self.info
                )
            }
            // This must be a struct..
            _ => {
                let ty = builder.backend_type(self.info);
                builder.structural_get_element_pointer(
                    ty,
                    self.value,
                    builder.backend_field_index(self.info, field),
                )
            }
        };

        // @@PointerCasts: this can be removed if we use LLVM 15 where it is
        // not needed to pointer cast.
        let value =
            builder.pointer_cast(value, builder.type_ptr_to(builder.backend_type(field_info)));

        PlaceRef { value, info: field_info, alignment: field_alignment }
    }

    /// Emit a hint to the code generation backend that this [PlaceRef] is
    /// alive after this point.
    pub fn storage_live<Builder: BlockBuilderMethods<'b, Value = V>>(&self, builder: &mut Builder) {
        let layout = builder.ctx().layout_info(self.info.layout);
        builder.lifetime_start(self.value, layout.size);
    }

    /// Emit a hint to the code generation backend that this [PlaceRef] is
    /// now dead after this point and can be discarded.
    pub fn storage_dead<Builder: BlockBuilderMethods<'b, Value = V>>(&self, builder: &mut Builder) {
        let layout = builder.ctx().layout_info(self.info.layout);
        builder.lifetime_end(self.value, layout.size);
    }
}

impl<'b, Builder: BlockBuilderMethods<'b>> FnBuilder<'b, Builder> {
    /// Compute the type and layout of a [Place]. This deals with
    /// all projections that occur on the [Place].
    pub fn compute_place_ty_info(&self, builder: &mut Builder, place: ir::Place) -> TyInfo {
        let place_ty = PlaceTy::from_place(place, self.body, self.ctx.ir_ctx());
        builder.layout_of_id(place_ty.ty)
    }

    /// Emit backend specific code for handling a [Place].
    ///
    /// This function will return a [PlaceRef] which can be used to
    /// store a value into the place which can be used by the called
    /// to `store` a value into the place.
    pub fn codegen_place(
        &mut self,
        builder: &mut Builder,
        place: ir::Place,
    ) -> PlaceRef<Builder::Value> {
        // copy the projections from the place.
        let projections = builder.ir_ctx().projections().get_vec(place.projections);

        let mut base = 0;

        let mut codegen_base = match self.locals[place.local] {
            LocalRef::Place(place) => place,
            LocalRef::Operand(..) => {
                if projections.first() == Some(&ir::PlaceProjection::Deref) {
                    base = 1;

                    // we have to copy a slice of the projections where
                    // we omit the first projection (which is a deref).
                    let projections =
                        builder.ir_ctx().projections().create_from_slice(&projections[1..]);

                    let codegen_base = self.codegen_consume_operand(
                        builder,
                        ir::Place { local: place.local, projections },
                    );

                    codegen_base.deref(builder)
                } else {
                    panic!("using operand local `{place:?}` as place")
                }
            }
        };

        // Apply all of the projections on the initial base
        // producing a modified place reference.
        for projection in projections[base..].iter() {
            codegen_base = match *projection {
                ir::PlaceProjection::Downcast(variant) => {
                    codegen_base.project_downcast(builder, variant)
                }
                ir::PlaceProjection::Field(index) => codegen_base.project_field(builder, index),
                ir::PlaceProjection::Index(index) => {
                    let index_operand: ir::Operand =
                        ir::Place::from_local(index, builder.ir_ctx()).into();
                    let index = self.codegen_operand(builder, &index_operand);

                    let value = index.immediate_value();
                    codegen_base.project_index(builder, value)
                }
                ir::PlaceProjection::ConstantIndex { from_end: false, offset, .. } => {
                    let offset_value = builder.const_usize(offset as u64);
                    codegen_base.project_index(builder, offset_value)
                }
                ir::PlaceProjection::ConstantIndex { from_end: true, offset, .. } => {
                    let offset_value = builder.const_usize(offset as u64);
                    let len_value = codegen_base.len(builder);
                    let index_value = builder.sub(len_value, offset_value);
                    codegen_base.project_index(builder, index_value)
                }
                ir::PlaceProjection::SubSlice { from, .. } => {
                    let mut sub_slice =
                        codegen_base.project_index(builder, builder.const_usize(from as u64));
                    let projected_ty = PlaceTy::from_ty(codegen_base.info.ty)
                        .projection_ty(builder.ir_ctx(), *projection);

                    // @@Verify: if the size of the array is not known, do we
                    // have to do record the size of this slice using `extra_value`?

                    sub_slice.info = builder.layout_of_id(projected_ty);
                    sub_slice.value = builder.pointer_cast(
                        sub_slice.value,
                        builder.type_ptr_to(builder.backend_type(sub_slice.info)),
                    );

                    sub_slice
                }
                ir::PlaceProjection::Deref => {
                    builder.load_operand(codegen_base).deref(builder.ctx())
                }
            }
        }

        codegen_base
    }
}