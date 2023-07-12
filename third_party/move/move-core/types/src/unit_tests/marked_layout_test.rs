// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    account_address::AccountAddress,
    u256,
    value::{MoveStruct, MoveStructLayout, MoveTypeLayout, MoveValue},
};

/// Panics if a value is deserialized differently with the given two layouts.
fn assert_same_serialization(v: &MoveValue, l1: &MoveTypeLayout, l2: &MoveTypeLayout) {
    let blob = v.simple_serialize().unwrap();
    let v1 = MoveValue::simple_deserialize(&blob, l1).unwrap();
    let v2 = MoveValue::simple_deserialize(&blob, l2).unwrap();
    assert_eq!(v.clone(), v1);
    assert_eq!(v1, v2);
}

macro_rules! marked {
    ($layout:expr) => {
        MoveTypeLayout::Marked(Box::new($layout))
    };
}

#[test]
fn test_marked_layouts_for_primitive_values() {
    use MoveTypeLayout as L;
    use MoveValue::*;

    assert_same_serialization(&Bool(false), &marked!(L::Bool), &L::Bool);
    assert_same_serialization(&U8(1), &marked!(L::U8), &L::U8);
    assert_same_serialization(&U16(2), &marked!(L::U16), &L::U16);
    assert_same_serialization(&U32(3), &marked!(L::U32), &L::U32);
    assert_same_serialization(&U64(4), &marked!(L::U64), &L::U64);
    assert_same_serialization(&U128(5), &marked!(L::U128), &L::U128);
    assert_same_serialization(&U256(u256::U256::one()), &marked!(L::U256), &L::U256);
    assert_same_serialization(
        &Address(AccountAddress::ONE),
        &marked!(L::Address),
        &L::Address,
    );
    assert_same_serialization(
        &Signer(AccountAddress::TWO),
        &marked!(L::Signer),
        &L::Signer,
    );
}

#[test]
fn test_marked_layouts_for_vector_values() {
    use MoveTypeLayout as L;
    use MoveValue::*;

    let v = Vector(vec![U32(1), U32(2), U32(3)]);
    let unmarked_layout = L::Vector(Box::new(L::U32));

    let marked_layout = L::Vector(Box::new(marked!(L::U32)));
    assert_same_serialization(&v, &marked_layout, &unmarked_layout);

    let marked_layout = marked!(L::Vector(Box::new(L::U32)));
    assert_same_serialization(&v, &marked_layout, &unmarked_layout);
}

#[test]
fn test_marked_layouts_for_nested_types() {
    use MoveStructLayout::*;
    use MoveTypeLayout as L;
    use MoveValue::*;

    let a = Struct(MoveStruct::Runtime(vec![U64(1)]));
    let b = Struct(MoveStruct::Runtime(vec![
        U8(2),
        Vector(vec![U32(3), U32(4)]),
        Bool(true),
    ]));
    let c = Struct(MoveStruct::Runtime(vec![a, U128(2), b]));

    let unmarked_layout = L::Struct(Runtime(vec![
        L::Struct(Runtime(vec![L::U64])),
        L::U128,
        L::Struct(Runtime(vec![L::U8, L::Vector(Box::new(L::U32)), L::Bool])),
    ]));

    let marked_layout = marked!(L::Struct(Runtime(vec![
        L::Struct(Runtime(vec![L::U64,])),
        L::U128,
        L::Struct(Runtime(vec![L::U8, L::Vector(Box::new(L::U32)), L::Bool,]))
    ])));
    assert_same_serialization(&c, &marked_layout, &unmarked_layout);

    let marked_layout = L::Struct(Runtime(vec![
        marked!(L::Struct(Runtime(vec![L::U64,]))),
        L::U128,
        L::Struct(Runtime(vec![L::U8, L::Vector(Box::new(L::U32)), L::Bool])),
    ]));
    assert_same_serialization(&c, &marked_layout, &unmarked_layout);

    let marked_layout = L::Struct(Runtime(vec![
        marked!(L::Struct(Runtime(vec![L::U64,]))),
        L::U128,
        L::Struct(Runtime(vec![
            marked!(L::U8),
            L::Vector(Box::new(marked!(L::U32))),
            L::Bool,
        ])),
    ]));
    assert_same_serialization(&c, &marked_layout, &unmarked_layout);
}

#[test]
fn test_nested_marked_layouts() {
    use MoveTypeLayout as L;
    use MoveValue::*;

    let v = U32(1);
    let unmarked_layout = L::U32;
    let marked_layout = marked!(marked!(marked!(marked!(L::U32))));
    assert_same_serialization(&v, &marked_layout, &unmarked_layout);
}
