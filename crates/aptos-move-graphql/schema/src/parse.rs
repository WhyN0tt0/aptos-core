// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::common::ObjectName;
use anyhow::{bail, Context, Result};
use aptos_api_types::{MoveModule, MoveType};
use async_graphql::dynamic::{Field, FieldFuture, Object, TypeRef, TypeRefInner};
use move_core_types::{
    identifier::Identifier,
    language_storage::{ModuleId, StructTag, CORE_CODE_ADDRESS},
};
use std::collections::{BTreeSet, HashSet};

/// This is a custom scalar that represents a blob of Move data that we haven't been
/// able to fully parse out. Ideally we never need this but for now we don't support
/// generic type params, so we represent them as this. This way the downstream client
/// code generators can cast this to Any rather than something inaccurate like String.
const SCALAR_ANY: &'static str = "Any";

// These scalars represent the Move primitive types. The client can choose to handle
// them as they wish.
const SCALAR_U8: &'static str = "U8";
const SCALAR_U16: &'static str = "U16";
const SCALAR_U32: &'static str = "U32";
const SCALAR_U64: &'static str = "U64";
const SCALAR_U128: &'static str = "U128";
const SCALAR_U256: &'static str = "U256";
const SCALAR_ADDRESS: &'static str = "Address";

pub const ALL_CUSTOM_SCALARS: &[&'static str] = &[
    SCALAR_ANY,
    SCALAR_U8,
    SCALAR_U16,
    SCALAR_U32,
    SCALAR_U64,
    SCALAR_U128,
    SCALAR_U256,
    SCALAR_ADDRESS,
];

pub fn parse_module(
    module: MoveModule,
) -> Result<(
    // GraphQL objects to include in the final schema.
    Vec<Object>,
    // Any new Move modules we need to retrieve.
    BTreeSet<ModuleId>,
)> {
    let mut objects = Vec::new();
    let mut modules_to_retrieve = BTreeSet::new();

    let module_id = ModuleId::new(module.address.into(), module.name.into());

    // For each struct in the module build an Object to include in the schema. While we
    // do this we look through the types of the fields and determine any more modules
    // we need to look up.
    for struc in module.structs.into_iter() {
        let mut types_to_resolve = Vec::new();
        let mut types_seen = HashSet::new();

        let struc_name: Identifier = struc.name.into();

        let struct_tag = StructTag {
            address: *module_id.address(),
            module: module_id.name().to_owned(),
            name: struc_name.clone(),
            // TODO: How should I handle generics? Make sure to think about this case.
            type_params: vec![],
        };
        let object_name = ObjectName::new(struct_tag);
        let mut object = Object::new(object_name);

        for field in struc.fields {
            types_to_resolve.push(field.typ.clone());
            let blah = field.name.to_string() == "data".to_string()
                && struc_name.to_string() == "Element".to_string();
            let field_type =
                TypeRef(move_type_to_field_type(&field.typ, blah).with_context(|| {
                    format!(
                        "Failed to parse field {} of struct {}",
                        field.name, struc_name,
                    )
                })?);
            if blah {
                println!("field: {:#?}", field);
                println!("field_type: {:#?}", field_type);
            }
            // TODO: When we have an enhanced ABI with comments set Field.description.
            let field = Field::new(
                field.name.to_string(),
                field_type,
                // The resolved value doesn't matter. These Fields will be used to
                // build an Object that we feed into a Schema only for the puspose of
                // getting a schema file. We won't ever execute queries against this
                // directly.
                move |_| FieldFuture::new(async move { Ok(Some(())) }),
            );
            object = object.field(field);
        }

        objects.push(object);

        // Go through the types recursively until we hit leaf types. As we do so,
        // we add more modules to `modules_to_retrieve`. This way, we can ensure
        // that we look up the types for all modules relevant to this struct.
        while let Some(typ) = types_to_resolve.pop() {
            if types_seen.contains(&typ) {
                continue;
            }
            types_seen.insert(typ.clone());

            // For types that refer to other types, add those to the list of
            // types. This continues until we hit leaves / a cycle.
            match typ {
                MoveType::Vector { items: typ } => {
                    types_to_resolve.push(*typ);
                },
                MoveType::Reference {
                    mutable: _,
                    to: typ,
                } => {
                    types_to_resolve.push(*typ);
                },
                MoveType::Struct(struct_tag) => {
                    let module_id =
                        ModuleId::new(struct_tag.address.into(), struct_tag.module.into());
                    modules_to_retrieve.insert(module_id);
                },
                _other => {},
            }
        }
    }

    Ok((objects, modules_to_retrieve))
}

// Should be

// TODO add tests to ensure this matches what we do in value.rs
pub fn move_type_to_field_type(field_type: &MoveType, blah: bool) -> Result<TypeRefInner> {
    if blah {
        println!("field type: {:#?}", field_type);
    }
    match field_type {
        MoveType::Bool => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            TypeRef::BOOLEAN.into(),
        )))),
        // You'll see that we use custom scalar types in the schema for these types.
        // This doesn't directly affect the way we encode the responses. Indeed, we
        // encode u8, u16, and u32 as ints and u64, u128, and u256 as strings in
        // the messages over the wire. It is then up to the client to choose how
        // to interpret these values.
        MoveType::U8 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U8),
        )))),
        MoveType::U16 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U16),
        )))),
        MoveType::U32 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U32),
        )))),
        MoveType::U64 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U64),
        )))),
        MoveType::U128 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U128),
        )))),
        MoveType::U256 => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_U256),
        )))),
        MoveType::Address => Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
            std::borrow::Cow::Borrowed(SCALAR_ADDRESS),
        )))),
        // TODO: Do we want special behavior for vectors of u8? What about other byte
        // vectors? Okay yeah I know for vector<u8> at the least I want to represent
        // this a different way. Sort of hard to differentiate between byte data and
        // someone just wanting to store a small vec of u8 though, I wish we had some
        // kind of bytes wrapper type. I can bring this up, though it's ofc too late
        // in most cases.
        MoveType::Vector { items: move_type } => Ok(TypeRefInner::NonNull(Box::new(
            TypeRefInner::List(Box::new(move_type_to_field_type(move_type, blah)?)),
        ))),
        MoveType::Struct(struct_tag) => {
            // We have special handling for the following:
            //   - Strings
            //   - Options
            let struct_tag = StructTag::try_from(struct_tag.clone())
                .context("Unexpectedly failed to build StructTag")?;
            if struct_tag.is_std_string(&CORE_CODE_ADDRESS) {
                // We "unwrap" the string::String and just represent it as a string in
                // the schema. The value builder will do the same, pulling the bytes
                // out from the String and returning them as a normal UTF-8 string.
                Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
                    TypeRef::STRING.into(),
                ))))
            }
            /*
            // Commenting out the handling of Option for now because the API doesn't
            // return data in this way either yet, so if I want to use this tool now
            // I can't have this behavior enabled.
            else if struct_tag.is_std_option(&CORE_CODE_ADDRESS) {
                // Extract the inner type of the Option and get the type of that.
                // Because for all other Move types we return them as non-nullable we
                // pull out the inner type and return just that, to indicate it could
                // possibly be null.
                let type_tag = struct_tag.type_params.into_iter().next().context(
                    "Option unexpectedly had no generic type params, this should be impossible",
                )?;
                if blah {
                    println!("generic type param type: {:#?}", type_tag);
                }
                let move_type = MoveType::from(type_tag);
                let field_type = move_type_to_field_type(&move_type, blah)?;
                // There is no great way to represent Option<Option<T>> in a GraphQL
                // schema. Theoretically we could add some artifical struct like `inner`
                // to store the inner option, but in reality this pattern never gets
                // used. Indeed, at the time of writing no Move code in aptos-move uses
                // Option<Option<T>>. So we choose not to handle it.
                if let TypeRefInner::NonNull(field_type) = field_type {
                    Ok(*field_type)
                } else {
                    // TODO: Theoretically it could just do what we do for the non-string,
                    // non-option case?
                    Err(anyhow::anyhow!(
                        "Expected non-null type for Option inner type but got: {:?}. \
                            Likely this means you have an Option<Option<T>>. The schema \
                            generator does not support this.",
                        field_type
                    ))
                }
            } */
            else {
                let object_name = ObjectName::new(struct_tag);
                // TODO: This needs to take generics into account.
                Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
                    object_name.to_string().into(),
                ))))
            }
        },
        MoveType::GenericTypeParam { index } => {
            if blah {
                println!("GENERIC TYPE PARAM: {}", index);
            }
            // TODO: Currently we're pretty much just declaring bankruptcy on generics.
            // For example, if something uses a SimpleMap, the key and value will just
            // be represented as Any even though they have actual types.
            Ok(TypeRefInner::NonNull(Box::new(TypeRefInner::Named(
                std::borrow::Cow::Borrowed(SCALAR_ANY),
            ))))
        },
        // These types cannot appear in structs that we read from storage:
        //   - Signer is not store
        //   - References aren't store.
        //   - Unparseable is only used on the input side
        MoveType::Signer | MoveType::Reference { mutable: _, to: _ } | MoveType::Unparsable(_) => {
            bail!(
                "Type {:?} should not appear in a struct from storage",
                field_type
            )
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use aptos_api_types::{Address, MoveStructTag};
    use move_core_types::identifier::Identifier;
    use std::str::FromStr;

    /// This function builds the following Move type:
    ///
    /// Option<T>
    ///
    /// In GraphQL schema syntax this is represented as `T`
    fn build_option(inner: MoveType) -> MoveType {
        MoveType::Struct(MoveStructTag {
            address: Address::from_str("0x1").unwrap(),
            module: Identifier::new("option").unwrap().into(),
            name: Identifier::new("Option").unwrap().into(),
            generic_type_params: vec![inner],
        })
    }

    /// This function builds the following Move type:
    ///
    /// vector<u32>
    ///
    /// This is a mandatory vector filled with mandatory u32s.
    ///
    /// In GraphQL schema syntax this is represented as `[Int!]!`
    fn build_vec_of_u32s() -> MoveType {
        MoveType::Vector {
            items: Box::new(MoveType::U32),
        }
    }

    /// This function builds the following Move type:
    ///
    /// vector<vector<u32>>
    ///
    /// This is a mandatory vector filled with mandatory vectors filled with u32s.
    ///
    /// In GraphQL schema syntax this is represented as `[[Int!]!]!`
    fn build_vec_of_vecs_of_u32s() -> MoveType {
        MoveType::Vector {
            items: Box::new(build_vec_of_u32s()),
        }
    }

    /// This function builds the following Move type:
    ///
    /// vector<Option<vector<u32>>>
    ///
    /// So, it's a mandatory vector containing optional vectors filled with
    /// mandatory u32s.
    ///
    /// In GraphQL schema syntax this is represented as `[[Int!]]!`
    fn build_vec_of_optional_vecs_of_u32s() -> MoveType {
        MoveType::Vector {
            items: Box::new(build_option(build_vec_of_u32s())),
        }
    }

    /// This function builds the following Move type:
    ///
    /// Option<vector<vector<Option<vector<u32>>>>>,
    ///
    /// So, it's an optional vector containing non-optional vectors filled with
    /// optional vectors of mandatory u32s.
    ///
    /// In GraphQL schema syntax this is represented as `[[[Int!]]!]`
    fn build_complex_type() -> MoveType {
        build_option(MoveType::Vector {
            items: Box::new(build_vec_of_optional_vecs_of_u32s()),
        })
    }

    #[test]
    fn test_option() -> Result<()> {
        let testing_move_type = build_option(MoveType::U32);
        let field_type = move_type_to_field_type(&testing_move_type, true)?;
        println!("field_type: {:#?}", field_type);
        println!("field_type: {}", field_type);
        assert_eq!(&field_type.to_string(), "U32");
        Ok(())
    }

    /// See the comment in move_type_to_field_type for an explanation for why we
    /// expect this to fail.
    #[test]
    fn test_option_of_option() -> Result<()> {
        let testing_move_type = build_option(build_option(MoveType::U16));
        assert!(move_type_to_field_type(&testing_move_type, true).is_err());
        Ok(())
    }

    #[test]
    fn test_vec_of_u32s() -> Result<()> {
        let testing_move_type = build_vec_of_u32s();
        let field_type = move_type_to_field_type(&testing_move_type, true)?;
        println!("field_type: {:#?}", field_type);
        println!("field_type: {}", field_type);
        assert_eq!(&field_type.to_string(), "[U32!]!");
        Ok(())
    }

    #[test]
    fn test_vec_of_vecs_of_u32s() -> Result<()> {
        let testing_move_type = build_vec_of_vecs_of_u32s();
        let field_type = move_type_to_field_type(&testing_move_type, true)?;
        println!("field_type: {:#?}", field_type);
        println!("field_type: {}", field_type);
        assert_eq!(&field_type.to_string(), "[[U32!]!]!");
        Ok(())
    }

    #[test]
    fn test_vec_of_optional_vecs_of_u32s() -> Result<()> {
        let testing_move_type = build_vec_of_optional_vecs_of_u32s();
        let field_type = move_type_to_field_type(&testing_move_type, true)?;
        println!("field_type: {:#?}", field_type);
        println!("field_type: {}", field_type);
        assert_eq!(&field_type.to_string(), "[[U32!]]!");
        Ok(())
    }

    #[test]
    fn test_complex_move_type() -> Result<()> {
        let testing_move_type = build_complex_type();
        let field_type = move_type_to_field_type(&testing_move_type, true)?;
        println!("field_type: {:#?}", field_type);
        println!("field_type: {}", field_type);
        assert_eq!(&field_type.to_string(), "[[[U32!]]!]");
        Ok(())
    }
}
