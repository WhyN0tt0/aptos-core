// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{
    common::SchemaBuilderTrait,
    parse::{parse_module, ALL_CUSTOM_SCALARS},
};
use anyhow::{bail, Context as AnyhowContext, Result};
use aptos_api_types::MoveModule;
use async_graphql::dynamic::{Scalar, Schema};
use move_core_types::language_storage::ModuleId;
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Clone, Debug)]
pub struct SchemaBuilderLocal {
    modules: BTreeMap<ModuleId, MoveModule>,
}

impl SchemaBuilderLocal {
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
        }
    }

    // TODO: The way this works now we end up building objects for every struct in the
    // top level modules and anything the fields in those structs reference. It'd be
    // nice to say "I only care about this module, generate objects only for the structs"
    // in that module and for the fields in those structs. Then when we generate stuff
    // from the modules that the top level module depends on, only generate structs
    // that the top level structs actually used.
    pub fn build(mut self) -> Result<Schema> {
        if self.modules.is_empty() {
            anyhow::bail!("Cannot build Schema without any modules to lookup or add");
        }

        let mut objects = Vec::new();

        let mut modules_processed = BTreeSet::new();

        while let Some((module_id, module)) = self.modules.pop_first() {
            if modules_processed.contains(&module_id) {
                continue;
            }
            modules_processed.insert(module_id.clone());
            let (new_objects, modules_to_retrieve) = parse_module(module)
                .with_context(|| format!("Failed to parse module {}", module_id))?;
            objects.extend(new_objects);

            // Filter out modules we already have / have processed.
            let modules_to_retrieve: HashSet<_> = modules_to_retrieve
                .into_iter()
                .filter(|module_id| {
                    !self.modules.contains_key(module_id) && !modules_processed.contains(module_id)
                })
                .collect();

            if !modules_to_retrieve.is_empty() {
                bail!(
                    "While processing {} references to modules not available \
                    to the builder were found: {:?}. This makes it impossible \
                    to comprehensively resolve types, and this builder is \
                    unable to look up new modules, so it is impossible to \
                    build a complete Schema.",
                    module_id,
                    modules_to_retrieve
                );
            }
        }

        // TODO: The fact you can only pass in one query is not ideal, find a way to
        // do something better than this. It'd be best to include no queries at all.
        let mut builder = Schema::build(objects[0].type_name(), None, None);
        for object in objects.into_iter() {
            builder = builder.register(object);
        }
        // Add our custom scalars.
        for scalar in ALL_CUSTOM_SCALARS {
            builder = builder.register(Scalar::new(*scalar));
        }
        let schema = builder.finish().context("Failed to build Schema")?;

        Ok(schema)
    }
}

impl SchemaBuilderTrait for SchemaBuilderLocal {
    fn add_modules(mut self, modules: Vec<MoveModule>) -> Self {
        for module in modules {
            self.modules.insert(
                ModuleId::new(module.address.into(), module.name.clone().into()),
                module,
            );
        }
        self
    }

    fn add_module(self, module: MoveModule) -> Self {
        self.add_modules(vec![module])
    }
}

impl Default for SchemaBuilderLocal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aptos_move_graphql_test_helpers::compile_package;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_build_schema() -> Result<()> {
        // Compile the hero package and all the packages we know it recursively depends on.
        let mut modules = Vec::new();
        for name in &[
            "aptos-stdlib",
            "move-stdlib",
            "aptos-framework",
            "aptos-token-objects",
        ] {
            let path =
                PathBuf::try_from(format!("../../../aptos-move/framework/{}", name)).unwrap();
            modules.extend(compile_package(path)?);
        }
        modules.extend(compile_package(
            PathBuf::try_from("../../../aptos-move/move-examples/token_objects/hero").unwrap(),
        )?);

        SchemaBuilderLocal::new()
            .add_modules(modules)
            .build()
            .context("Failed to build Schema with all the modules")?;

        // TODO: Assert some things about the schema.

        Ok(())
    }
}
