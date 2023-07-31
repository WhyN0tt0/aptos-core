// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

//! This file contains the logic for converting Move values into GraphQL values.

use anyhow::Result;
use async_graphql::{indexmap::IndexMap, Name, Value};
use move_core_types::language_storage::CORE_CODE_ADDRESS;
use move_resource_viewer::{AnnotatedMoveStruct, AnnotatedMoveValue};

// TODO: Consdier letting the handling of a single field output multiple fields. This
// way we could have a v1, v2, v3, etc representation of a field. e.g. if Option is
// internally a vec at some point but later we make it an actual option.

// Okay I think instead we should design this in at a higher level. So any API that
// returns GraphQL should support like `data_as_json_v1`, `data_as_json_v2`, etc.

pub fn annotated_struct_to_graphql_object(struc: AnnotatedMoveStruct) -> Result<Value> {
    let mut map = IndexMap::new();
    for (id, val) in struc.value {
        map.insert(
            Name::new(id.into_string()),
            annotated_value_to_graphql_value(val)?,
        );
    }
    Ok(Value::Object(map))
}

pub fn annotated_value_to_graphql_value(value: AnnotatedMoveValue) -> Result<Value> {
    match value {
        AnnotatedMoveValue::U8(v) => Ok(Value::Number(v.into())),
        AnnotatedMoveValue::U16(v) => Ok(Value::Number(v.into())),
        AnnotatedMoveValue::U32(v) => Ok(Value::Number(v.into())),
        // Consider whether it'd be better to represent these all as numbers and push
        // the handling of numbers to the client, e.g. with something like this:
        // https://the-guild.dev/graphql/scalars/docs/scalars/big-int
        AnnotatedMoveValue::U64(v) => Ok(Value::String(v.to_string())),
        AnnotatedMoveValue::U128(v) => Ok(Value::String(v.to_string())),
        AnnotatedMoveValue::U256(v) => Ok(Value::String(v.to_string())),
        AnnotatedMoveValue::Bool(v) => Ok(Value::Boolean(v)),
        AnnotatedMoveValue::Address(v) => {
            Ok(Value::String(format!("0x{}", v.to_canonical_string())))
        },
        AnnotatedMoveValue::Vector(_, vals) => Ok(Value::List(
            vals.into_iter()
                .map(annotated_value_to_graphql_value)
                .collect::<anyhow::Result<_>>()?,
        )),
        AnnotatedMoveValue::Bytes(v) => Ok(Value::Binary(v.into())),
        AnnotatedMoveValue::Struct(v) => {
            // We have special handling for the following:
            //   - string::String
            //   - option::Option
            if v.type_.is_std_string(&CORE_CODE_ADDRESS) {
                convert_string(v)
            } else if v.type_.is_std_option(&CORE_CODE_ADDRESS) {
                convert_option(v)
            } else {
                annotated_struct_to_graphql_object(v)
            }
        },
    }
}

/// This function takes a string::String and returns it as a Value::String. Essentially
/// we take the inner bytes and flatten out the struct into a string.
pub fn convert_string(v: AnnotatedMoveStruct) -> anyhow::Result<Value> {
    if let Some((_, AnnotatedMoveValue::Bytes(bytes))) = v.value.into_iter().next() {
        Ok(Value::String(String::from_utf8(bytes)?))
    } else {
        anyhow::bail!("Expected string::String");
    }
}

/// This function takes an option::Option and converts it from a vec into a real
/// GraphQL option, meaning Value::Null if the vec is empty or the value if the vec
/// has a single value.
pub fn convert_option(v: AnnotatedMoveStruct) -> anyhow::Result<Value> {
    if let Some((_, AnnotatedMoveValue::Vector(_, values))) = v.value.into_iter().next() {
        if values.is_empty() {
            Ok(Value::Null)
        } else if values.len() == 1 {
            annotated_value_to_graphql_value(values.into_iter().next().unwrap())
        } else {
            anyhow::bail!("Expected option::Option with 0 or 1 values");
        }
    } else {
        anyhow::bail!("Expected option::Option");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aptos_move_graphql_test_helpers::build_tontine;

    #[tokio::test]
    async fn test_annotated_struct_to_graphql_object() {
        let tontine = build_tontine();
        let object = annotated_struct_to_graphql_object(tontine).unwrap();
        let expected_json = serde_json::json!({
            "config": {
                // Notice how the string is represented as such, rather than a vec of
                // UTF-8 bytes.
                "description": "My Tontine",
                "per_member_amount_octa": "100000",
                // Notice how the option is denested; the value is pulled out of the
                // inner vec.
                "delegation_pool": "0x0000000000000000000000000000000000000000000000000000000000000123"
            },
            "creation_time_secs": "1686829095",
            "member_data": {
                "data": [
                    {
                        "key": "0x0000000000000000000000000000000000000000000000000000000000000123",
                        "value": {
                            "contributed_octa": "50000",
                            "reconfirmation_required": false
                        }
                    }
                ]
            },
            "fallback_executed": false,
            "funds_claimed_secs": "0",
            // Notice that the option is represented as null rather than an empty vec.
            "funds_claimed_by": null
        });
        assert_eq!(object.into_json().unwrap(), expected_json);
    }
}
