use crate::shared::type_resolver::{self, SemanticSymbol};
use apollo_compiler::Schema;
use graphox_core::config::{Config, ProjectConfig};
use graphox_core::document::DocumentState;
use lsp_types::{Location, Position, Range, Url};
use std::path::{Path, PathBuf};

pub trait DocumentTypeDefinition {
    fn get_type_definition(
        &self,
        position: Position,
        schema: &Schema,
        config: &Config,
    ) -> Option<Location>;
}

impl DocumentTypeDefinition for DocumentState {
    fn get_type_definition(
        &self,
        position: Position,
        schema: &Schema,
        config: &Config,
    ) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;

                let symbol =
                    type_resolver::resolve_symbol_at_node(self, node, offset, byte_offset, schema)
                        .or_else(|| {
                            type_resolver::resolve_fragment_spread_at_node(
                                self,
                                node,
                                offset,
                                byte_offset,
                            )
                        })?;

                let path = self.uri.to_file_path().ok()?;
                let project_config = config.get_project_for_path(&path)?;

                return resolve_codegen_location(self, symbol, project_config, config);
            }
        }
        None
    }
}

pub fn resolve_codegen_location(
    doc: &DocumentState,
    symbol: SemanticSymbol,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<Location> {
    let source_path = doc.uri.to_file_path().ok()?;
    let codegen_path = get_codegen_path(&source_path, project_config, config)?;
    let type_name = get_codegen_type_name(&symbol, project_config, config)?;

    let content = std::fs::read_to_string(&codegen_path).ok()?;
    let range = find_type_in_content(&content, &type_name)?;

    Some(Location {
        uri: Url::from_file_path(codegen_path).ok()?,
        range,
    })
}

pub fn get_codegen_path(
    source_path: &Path,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<PathBuf> {
    let file_name = source_path.file_name()?.to_str()?;
    let codegen_file_name = format!("{}.codegen.ts", file_name);

    if let Some(output_dir) = project_config.output_dir() {
        let mut path = config.base_dir().to_path_buf();
        path.push(output_dir);

        // Calculate relative path from project root if source_path is inside base_dir
        if let Ok(rel_path) = source_path.strip_prefix(config.base_dir())
            && let Some(parent) = rel_path.parent()
        {
            path.push(parent);
        }

        path.push(codegen_file_name);
        Some(path)
    } else {
        let mut path = source_path.to_path_buf();
        path.set_file_name(codegen_file_name);
        Some(path)
    }
}

pub fn get_codegen_type_name(
    symbol: &SemanticSymbol,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<String> {
    let codegen_config = config.get_codegen_config(Some(project_config));

    match symbol {
        SemanticSymbol::Operation { op_type, name, .. } => {
            let name = name.as_ref()?;
            let suffix = match op_type.as_str() {
                "query" => codegen_config.query_suffix(),
                "mutation" => codegen_config.mutation_suffix(),
                "subscription" => codegen_config.subscription_suffix(),
                _ => return None,
            };
            Some(format!("{}{}", name, suffix))
        }
        SemanticSymbol::Fragment { name, .. } => {
            let suffix = codegen_config.fragment_suffix();
            Some(format!("{}{}", name, suffix))
        }
        _ => None,
    }
}

pub fn find_type_in_content(content: &str, type_name: &str) -> Option<Range> {
    let patterns = [
        format!("export type {} =", type_name),
        format!("export type {}=", type_name),
        format!("export interface {} ", type_name),
        format!("export interface {} {{", type_name),
    ];

    for pattern in patterns {
        if let Some(byte_idx) = content.find(&pattern) {
            // Simple line/char calculation
            let mut line = 0;
            let mut col = 0;
            for (i, c) in content.char_indices() {
                if i >= byte_idx {
                    break;
                }
                if c == '\n' {
                    line += 1;
                    col = 0;
                } else {
                    col += 1;
                }
            }
            return Some(Range {
                start: Position {
                    line,
                    character: col,
                },
                end: Position {
                    line,
                    character: col + pattern.len() as u32,
                },
            });
        }
    }
    None
}
