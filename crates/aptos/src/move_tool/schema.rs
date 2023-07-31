// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use super::IncludedArtifactsArgs;
use crate::common::types::{CliCommand, CliError, CliTypedResult, MovePackageDir};
use aptos_api_types::MoveModule;
use aptos_framework::{BuildOptions, BuiltPackage};
use aptos_move_graphql_schema::{build_sdl, SchemaBuilderLocal, SchemaBuilderTrait};
use async_trait::async_trait;
use clap::Parser;
use std::path::PathBuf;

/// Generate a GraphQL schema based on the ABI of the compiled modules.
///
/// aptos move generate-schema
///
#[derive(Parser)]
pub struct GenerateSchema {
    /// Where to write the schema file. If not given the schema will be written to the
    /// package directory.
    #[clap(long, value_parser)]
    schema_path: Option<PathBuf>,

    /// The filename of the schema.
    #[clap(long, default_value = "schema.graphql")]
    schema_fname: String,

    #[clap(flatten)]
    included_artifacts_args: IncludedArtifactsArgs,

    #[clap(flatten)]
    move_options: MovePackageDir,
}

#[async_trait]
impl CliCommand<String> for GenerateSchema {
    fn command_name(&self) -> &'static str {
        "GenerateSchema"
    }

    async fn execute(self) -> CliTypedResult<String> {
        let dev = false;
        let build_options = BuildOptions {
            install_dir: self.move_options.output_dir.clone(),
            with_abis: true,
            ..self
                .included_artifacts_args
                .included_artifacts
                .build_options(
                    dev,
                    self.move_options.skip_fetch_latest_git_deps,
                    self.move_options.named_addresses(),
                    self.move_options.bytecode_version,
                )
        };

        let package_dir = self.move_options.get_package_path()?;

        // Build the package.
        let package = BuiltPackage::build(package_dir.clone(), build_options)
            .map_err(|e| CliError::MoveCompilationError(format!("{:#}", e)))?;

        // Convert the modules into MoveModule.
        let modules = package
            .all_modules()
            .cloned()
            .into_iter()
            .map(MoveModule::from)
            .collect();

        // Build the Schema struct.
        let schema = SchemaBuilderLocal::new()
            .add_modules(modules)
            .build()
            .map_err(|e| CliError::UnexpectedError(format!("Failed to build schema: {:#}", e)))?;

        let sdl = build_sdl(&schema);

        let schema_path = self
            .schema_path
            .unwrap_or(package_dir)
            .join(&self.schema_fname);

        // Write the schema to the file.
        std::fs::write(&schema_path, sdl)
            .map_err(|e| CliError::UnexpectedError(format!("Failed to write schema: {:#}", e)))?;

        Ok(format!("Schema written to {}", schema_path.display()))
    }
}
