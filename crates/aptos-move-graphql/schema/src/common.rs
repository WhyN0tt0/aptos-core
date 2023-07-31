// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use aptos_api_types::MoveModule;
use async_graphql::dynamic::Schema;
use move_core_types::language_storage::StructTag;

/// Defines functions common to all `SchemaBuilder`s.
pub trait SchemaBuilderTrait {
    /// Add modules that we have already retrieved.
    fn add_modules(self, modules: Vec<MoveModule>) -> Self;

    /// Add a module that we have already retreived.
    fn add_module(self, module: MoveModule) -> Self;
}

/// ObjectName is a version of StructTag that, when used as a string, conforms to
/// GraphQL naming principles.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectName(StructTag);

impl ObjectName {
    pub fn new(struct_tag: StructTag) -> Self {
        ObjectName(struct_tag)
    }

    pub fn inner(&self) -> &StructTag {
        &self.0
    }
}

impl From<&ObjectName> for String {
    fn from(object_name: &ObjectName) -> String {
        format!(
            "_{}__{}__{}",
            object_name.0.address.to_standard_string(),
            object_name.0.module,
            object_name.0.name,
        )
    }
}

impl From<ObjectName> for String {
    fn from(object_name: ObjectName) -> String {
        String::from(&object_name)
    }
}

impl std::fmt::Display for ObjectName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from(self))
    }
}

pub fn build_sdl(schema: &Schema) -> String {
    let sdl = schema.sdl();

    // Trim leading newlines.
    let sdl = sdl.trim_start_matches('\n');

    // Remove the schema section (everything starting with `schema {` and after}).
    // We only want the types.
    let sdl = sdl.split("schema {").next().unwrap();

    // Remove all trailing newlines.
    let sdl = sdl.trim_end_matches('\n');

    // Return the schema with one trailing newline.
    format!("{}\n", sdl)
}
