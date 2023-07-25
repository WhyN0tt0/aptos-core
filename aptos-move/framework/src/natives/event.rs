// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{
    get_metadata,
    natives::helpers::{
        make_module_natives, make_safe_native, SafeNativeContext, SafeNativeError, SafeNativeResult,
    },
    safely_pop_arg,
};
use aptos_gas_algebra_ext::{AbstractValueSize, InternalGasPerAbstractValueUnit};
use aptos_types::on_chain_config::{Features, TimedFeatures};
use aptos_utils::aptos_try;
use ark_std::iterable::Iterable;
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::language_storage::{StructTag, TypeTag};
use move_core_types::resolver::MoveResolver;
use move_core_types::vm_status::StatusCode;
use move_core_types::{gas_algebra::InternalGas, value::MoveTypeLayout};
use move_vm_runtime::native_functions::NativeFunction;
use move_vm_types::{loaded_data::runtime_types::Type, pop_arg, values::Value};
use smallvec::{smallvec, SmallVec};
use std::{collections::VecDeque, sync::Arc};

/// Cached emitted module events.
#[derive(Tid, default)]
pub struct NativeEventContext<'a> {
    resolver: &'a dyn MoveResolver,
    events: Vec<(StructTag, Vec<u8>)>,
}

impl<'a> NativeEventContext<'a> {
    pub fn new(resolver: &dyn MoveResolverExt) -> Self {
        Self {
            resolver,
            events: Vec::new(),
        }
    }

    pub fn into_events(self) -> Vec<(StructTag, Vec<u8>)> {
        self.events
    }
}
/***************************************************************************************************
 * native fun write_to_event_store
 *
 *   gas cost: base_cost
 *
 **************************************************************************************************/
#[derive(Debug, Clone)]
pub struct WriteToEventStoreGasParameters {
    pub base: InternalGas,
    pub per_abstract_value_unit: InternalGasPerAbstractValueUnit,
}

#[inline]
fn native_write_to_event_store(
    gas_params: &WriteToEventStoreGasParameters,
    calc_abstract_val_size: impl FnOnce(&Value) -> AbstractValueSize,
    context: &mut SafeNativeContext,
    mut ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> SafeNativeResult<SmallVec<[Value; 1]>> {
    debug_assert!(ty_args.len() == 1);
    debug_assert!(arguments.len() == 3);

    let ty = ty_args.pop().unwrap();
    let msg = arguments.pop_back().unwrap();
    let seq_num = safely_pop_arg!(arguments, u64);
    let guid = safely_pop_arg!(arguments, Vec<u8>);

    // TODO(Gas): Get rid of abstract memory size
    context.charge(
        gas_params.base + gas_params.per_abstract_value_unit * calc_abstract_val_size(&msg),
    )?;

    if !context.save_event(guid, seq_num, ty, msg)? {
        return Err(SafeNativeError::Abort { abort_code: 0 });
    }

    Ok(smallvec![])
}

pub fn make_native_write_to_event_store(
    calc_abstract_val_size: impl Fn(&Value) -> AbstractValueSize + Send + Sync + 'static,
) -> impl Fn(
    &WriteToEventStoreGasParameters,
    &mut SafeNativeContext,
    Vec<Type>,
    VecDeque<Value>,
) -> SafeNativeResult<SmallVec<[Value; 1]>> {
    move |gas_params, context, ty_args, args| -> SafeNativeResult<SmallVec<[Value; 1]>> {
        native_write_to_event_store(gas_params, &calc_abstract_val_size, context, ty_args, args)
    }
}

#[inline]
fn native_write_module_event_to_store(
    gas_params: &WriteToEventStoreGasParameters,
    calc_abstract_val_size: impl FnOnce(&Value) -> AbstractValueSize,
    context: &mut SafeNativeContext,
    mut ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> SafeNativeResult<SmallVec<[Value; 1]>> {
    debug_assert!(ty_args.len() == 1);
    debug_assert!(arguments.len() == 1);

    let ty = ty_args.pop().unwrap();
    let msg = arguments.pop_back().unwrap();

    // TODO(Gas): Get rid of abstract memory size
    context.charge(
        gas_params.base + gas_params.per_abstract_value_unit * calc_abstract_val_size(&msg),
    )?;

    let ctx = context.extensions().get_mut::<NativeEventContext>();
    let struct_tag = match context.type_to_type_tag(&ty)? {
        TypeTag::Struct(struct_tag) => Ok(*struct_tag),
        _ => Err(SafeNativeError::Abort {
            // not an struct type
            abort_code: 0x10001,
        }),
    }?;
    match check_event(ctx, &struct_tag) {
        Some(true) => (),
        _ => {
            return Err(SafeNativeError::Abort {
                // not a struct with event attribute
                abort_code: 0x10001,
            });
        },
    };
    let layout = get_type_layout(context, &ty)?;
    let blob = msg.simple_serialize(&layout).ok_or_else(|| {
        SafeNativeError::InvariantViolation(
            PartialVMError::new(StatusCode::VALUE_SERIALIZATION_ERROR)
                .with_message("Event serialization failure".to_string()),
        )
    })?;
    ctx.events.push((struct_tag, blob));

    Ok(smallvec![])
}
/***************************************************************************************************
 * module
 *
 **************************************************************************************************/
#[derive(Debug, Clone)]
pub struct GasParameters {
    pub write_to_event_store: WriteToEventStoreGasParameters,
}

pub fn make_all(
    gas_params: GasParameters,
    calc_abstract_val_size: impl Fn(&Value) -> AbstractValueSize + Send + Sync + 'static,
    timed_features: TimedFeatures,
    features: Arc<Features>,
) -> impl Iterator<Item = (String, NativeFunction)> {
    let natives = [
        (
            "write_to_event_store",
            make_safe_native(
                gas_params.write_to_event_store,
                timed_features.clone(),
                features.clone(),
                make_native_write_to_event_store(calc_abstract_val_size),
            ),
        ),
        (
            "write_to_module_event_store",
            make_safe_native(
                gas_params.write_to_event_store,
                timed_features,
                features,
                make_native_write_to_event_store(calc_abstract_val_size),
            ),
        ),
    ];

    make_module_natives(natives)
}

fn check_event(ctx: &mut NativeEventContext, struct_tag: &StructTag) -> Option<bool> {
    // check the event struct is valid.
    let md = get_metadata(
        ctx.resolver
            .get_module_metadata(&struct_tag.module_id())
            .as_slice(),
    )?;
    Some(
        md.struct_attributes
            .get(struct_tag.name.as_ident_str().as_str())?
            .iter()
            .any(|attr| attr.is_event()),
    )
}

fn get_type_layout(context: &SafeNativeContext, ty: &Type) -> PartialVMResult<MoveTypeLayout> {
    context
        .type_to_type_layout(ty)?
        .ok_or_else(|| partial_extension_error("cannot determine type layout"))
}
