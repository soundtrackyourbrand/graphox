use crate::shared::type_resolver::{self, SemanticSymbol};
use apollo_compiler::Schema;
use graphox_core::config::{Config, ProjectConfig};
use graphox_core::document::DocumentState;
use ls_types::{Location, Position, Range, Uri};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

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

                let path = self.uri.to_file_path()?.into_owned();
                let project_config = config.get_project_for_path(&path)?;

                // Fields are emitted as inline nested properties of the operation/
                // fragment type rather than as named types, so navigate by descending
                // the generated interface along the field's selection path.
                if matches!(symbol, SemanticSymbol::Field { .. }) {
                    return resolve_field_codegen_location(
                        self,
                        node,
                        offset,
                        project_config,
                        config,
                    );
                }

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
    let source_path = doc.uri.to_file_path()?.into_owned();
    let codegen_path = get_codegen_path(&source_path, project_config, config)?;
    let type_name = get_codegen_type_name(&symbol, project_config, config)?;

    let content = std::fs::read_to_string(&codegen_path).ok()?;
    let range = find_type_in_content(&content, &type_name)?;

    Some(Location {
        uri: Uri::from_file_path(codegen_path)?,
        range,
    })
}

pub fn get_codegen_path(
    source_path: &Path,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<PathBuf> {
    let base_dir = config.base_dir();

    // Match codegen's own output-path rule (`utils::get_output_path`): the generated
    // file lives under the project's `output_dir`, mirroring the source path relative
    // to the matching include-glob root, with the extension *replaced* by `.codegen.ts`
    // (so `Foo.graphql` → `Foo.codegen.ts`, not `Foo.graphql.codegen.ts`).
    let abs_source =
        std::fs::canonicalize(source_path).unwrap_or_else(|_| source_path.to_path_buf());
    let include_prefix = project_config
        .include()
        .patterns()
        .iter()
        .map(|p| graphox_core::utils::get_glob_root(p))
        .find(|root| {
            let abs_root = base_dir.join(root);
            let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
            graphox_core::utils::path_starts_with(&abs_source, &abs_root)
        })
        .unwrap_or_default();

    Some(graphox_core::utils::get_output_path(
        source_path,
        base_dir,
        project_config.output_dir().map(Path::new),
        Some(&include_prefix),
    ))
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
            let suffix = match op_type.as_ref() {
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
            let start = byte_to_position(content, byte_idx);
            return Some(Range {
                start,
                end: Position {
                    line: start.line,
                    character: start.character + pattern.len() as u32,
                },
            });
        }
    }
    None
}

/// Convert a byte offset in `content` into an LSP [`Position`].
///
/// Generated TypeScript identifiers (the navigation targets) are ASCII, so a plain
/// character count matches the negotiated UTF-16 offsets for these positions.
fn byte_to_position(content: &str, byte_idx: usize) -> Position {
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
    Position {
        line,
        character: col,
    }
}

/// Navigate to a field's generated TypeScript by descending the operation/fragment
/// interface along the field's selection path.
///
/// Codegen emits selected fields as inline nested properties (e.g. `album` → its
/// inner `display` → `image`), so we resolve the enclosing operation/fragment type,
/// then walk into the generated object scope by scope following the response keys
/// (honouring aliases) to land on the target field's property.
fn resolve_field_codegen_location(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<Location> {
    let source_path = doc.uri.to_file_path()?.into_owned();
    let codegen_path = get_codegen_path(&source_path, project_config, config)?;

    let root_type = enclosing_root_type_name(doc, node, offset, project_config, config)?;
    let field_path = enclosing_field_path(doc, node, offset)?;

    let content = std::fs::read_to_string(&codegen_path).ok()?;
    let range = find_field_property(&content, &root_type, &field_path)?;

    Some(Location {
        uri: Uri::from_file_path(codegen_path)?,
        range,
    })
}

/// The codegen type name of the operation/fragment that encloses `node`
/// (e.g. `AlbumQuery`, `UserFieldsFragment`).
fn enclosing_root_type_name(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    project_config: &ProjectConfig,
    config: &Config,
) -> Option<String> {
    let codegen_config = config.get_codegen_config(Some(project_config));
    let mut current = Some(node);
    while let Some(n) = current {
        match n.kind() {
            "operation_definition" => {
                let name = doc.get_node_text(doc.find_child_by_kind(n, "name")?, offset);
                let suffix = match doc.get_operation_type(n, offset).as_str() {
                    "query" => codegen_config.query_suffix(),
                    "mutation" => codegen_config.mutation_suffix(),
                    "subscription" => codegen_config.subscription_suffix(),
                    _ => return None,
                };
                return Some(format!("{name}{suffix}"));
            }
            "fragment_definition" => {
                let name_node = doc
                    .find_child_by_kind(n, "fragment_name")
                    .and_then(|fn_node| doc.find_child_by_kind(fn_node, "name"))
                    .or_else(|| doc.find_child_by_kind(n, "name"))?;
                let name = doc.get_node_text(name_node, offset);
                return Some(format!("{name}{}", codegen_config.fragment_suffix()));
            }
            _ => {}
        }
        current = n.parent();
    }
    None
}

/// A single step when descending the generated type along a selection path.
#[derive(Debug, PartialEq)]
enum PathStep {
    /// Descend into a field by its response key (alias when present).
    Field(String),
    /// An inline fragment `... on Type` narrows the value to a union member whose
    /// `__typename` is `Type` (or is transparent when no union was generated).
    Narrow(String),
}

/// Selection path from the enclosing operation/fragment down to and including the
/// field at `node`. Field steps use the response key (alias when present); inline
/// fragments contribute a `Narrow` step carrying their type condition.
fn enclosing_field_path(doc: &DocumentState, node: Node, offset: usize) -> Option<Vec<PathStep>> {
    // Find the field the cursor is on (the cursor may sit on its name/alias).
    let mut target = Some(node);
    while let Some(n) = target {
        match n.kind() {
            "field" => break,
            "operation_definition" | "fragment_definition" => return None,
            _ => target = n.parent(),
        }
    }

    let mut steps = Vec::new();
    let mut current = target;
    while let Some(n) = current {
        match n.kind() {
            "field" => steps.push(PathStep::Field(field_response_key(doc, n, offset))),
            "inline_fragment" => {
                if let Some(type_name) = inline_fragment_type_condition(doc, n, offset) {
                    steps.push(PathStep::Narrow(type_name));
                }
            }
            "operation_definition" | "fragment_definition" => {
                steps.reverse();
                return Some(steps);
            }
            _ => {}
        }
        current = n.parent();
    }
    None
}

/// The type condition of an inline fragment (`... on Type` → `Type`), if any.
fn inline_fragment_type_condition(
    doc: &DocumentState,
    node: Node,
    offset: usize,
) -> Option<String> {
    doc.find_child_by_kind(node, "type_condition")
        .and_then(|tc| doc.find_child_by_kind(tc, "named_type"))
        .and_then(|nt| doc.find_child_by_kind(nt, "name"))
        .map(|name| doc.get_node_text(name, offset))
}

/// The response key for a field node: the alias if one is present, otherwise the
/// field name (matching how codegen names the generated property).
fn field_response_key(doc: &DocumentState, field_node: Node, offset: usize) -> String {
    let mut name_node = None;
    let mut alias_node = None;
    let mut cursor = field_node.walk();
    for child in field_node.children(&mut cursor) {
        match child.kind() {
            "alias" => alias_node = Some(child),
            "name" => name_node = Some(child),
            _ => {}
        }
    }

    if let Some(alias) = alias_node
        && let Some(alias_name) = doc.find_child_by_kind(alias, "name")
    {
        return doc.get_node_text(alias_name, offset);
    }
    name_node
        .map(|n| doc.get_node_text(n, offset))
        .unwrap_or_default()
}

/// The region of generated TypeScript we are currently searching within.
enum Scope {
    /// The body of a single object (between its braces); properties live here at
    /// brace depth 0.
    Object { start: usize, end: usize },
    /// A property value that may be a single object or a union of `{...}` members
    /// (e.g. `{...} | {...} | null`).
    Value { start: usize, end: usize },
}

/// Find the property at the end of `path` inside the generated `root_type`,
/// returning the range of its name. Descends nested object types scope by scope,
/// selecting the matching union member for inline-fragment (`Narrow`) steps.
/// Returns `None` if any step can't be resolved (e.g. a fragment-masked spread) so
/// we never jump to a wrong location.
fn find_field_property(content: &str, root_type: &str, path: &[PathStep]) -> Option<Range> {
    let bytes = content.as_bytes();
    let body_start = find_type_body_start(content, root_type)?;
    let body_end = matching_brace(bytes, body_start)?;

    let mut scope = Scope::Object {
        start: body_start,
        end: body_end,
    };
    let mut result: Option<(usize, usize)> = None;

    for (idx, step) in path.iter().enumerate() {
        match step {
            PathStep::Field(key) => {
                let (obj_start, obj_end) = object_body_of(content, &scope)?;
                let (name_start, name_end, value_start, value_end) =
                    find_property_in(content, obj_start, obj_end, key)?;
                result = Some((name_start, name_end));
                if idx + 1 < path.len() {
                    scope = Scope::Value {
                        start: value_start,
                        end: value_end,
                    };
                }
            }
            PathStep::Narrow(type_name) => {
                let (start, end) = match scope {
                    Scope::Value { start, end } => (start, end),
                    // A `Narrow` not following a field value (rare) is treated as the
                    // current object body.
                    Scope::Object { start, end } => (start, end),
                };
                let (body_start, body_end) = narrow_to_member(content, start, end, type_name)?;
                scope = Scope::Object {
                    start: body_start,
                    end: body_end,
                };
            }
        }
    }

    let (name_start, name_end) = result?;
    let start = byte_to_position(content, name_start);
    Some(Range {
        start,
        end: Position {
            line: start.line,
            character: start.character + (name_end - name_start) as u32,
        },
    })
}

/// Resolve a scope to an object body to search properties in. A `Value` scope is
/// resolved to its first `{...}` block (single object, or first union member).
fn object_body_of(content: &str, scope: &Scope) -> Option<(usize, usize)> {
    match *scope {
        Scope::Object { start, end } => Some((start, end)),
        Scope::Value { start, end } => top_level_object_blocks(content, start, end)
            .into_iter()
            .next(),
    }
}

/// Select the union member whose `__typename` matches `type_name` within a value
/// region. When the value is a single object (no union), it is returned as-is
/// (the inline fragment did not change the generated shape).
fn narrow_to_member(
    content: &str,
    start: usize,
    end: usize,
    type_name: &str,
) -> Option<(usize, usize)> {
    let blocks = top_level_object_blocks(content, start, end);
    if blocks.len() <= 1 {
        return blocks.into_iter().next();
    }
    blocks
        .into_iter()
        .find(|&(bs, be)| member_typename(content, bs, be).as_deref() == Some(type_name))
}

/// The `__typename` literal of an object body (`__typename: "User"` → `User`).
fn member_typename(content: &str, body_start: usize, body_end: usize) -> Option<String> {
    let body = &content[body_start..body_end];
    let from = body.find("__typename")?;
    let after = &body[from..];
    let open = after.find('"')?;
    let rest = &after[open + 1..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

/// All top-level `{...}` object blocks within `start..end`, returned as the byte
/// ranges of their bodies (between the braces). Handles `{a} | {b}`, `Array<{x}>`,
/// and a lone `{x}`.
fn top_level_object_blocks(content: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut blocks = Vec::new();
    let mut i = start;
    while i < end {
        match bytes[i] {
            b'"' | b'\'' | b'`' => i = skip_string(bytes, i),
            b'/' if i + 1 < end && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'/' if i + 1 < end && bytes[i + 1] == b'/' => i = skip_line_comment(bytes, i),
            b'{' => {
                if let Some(close) = matching_brace(bytes, i + 1) {
                    blocks.push((i + 1, close));
                    i = close + 1;
                } else {
                    break;
                }
            }
            _ => i += 1,
        }
    }
    blocks
}

/// Within object body `obj_start..obj_end` (brace depth 0), find property `key`.
/// Returns `(name_start, name_end, value_start, value_end)` where `value_*` bound
/// the property's type, from just after the `:` to the terminating `;` (or the
/// object body end).
fn find_property_in(
    content: &str,
    obj_start: usize,
    obj_end: usize,
    key: &str,
) -> Option<(usize, usize, usize, usize)> {
    let bytes = content.as_bytes();
    let mut i = obj_start;
    let mut depth: i32 = 0;
    while i < obj_end {
        let b = bytes[i];
        match b {
            b'"' | b'\'' | b'`' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'/' if i + 1 < obj_end && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
                continue;
            }
            b'/' if i + 1 < obj_end && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                continue;
            }
            _ if depth == 0 && (b.is_ascii_alphabetic() || b == b'_') => {
                let start = i;
                while i < obj_end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let ident_end = i;
                // Skip whitespace, then an optional `?`, then expect `:` for a property.
                let mut j = i;
                while j < obj_end && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < obj_end && bytes[j] == b'?' {
                    j += 1;
                    while j < obj_end && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                }
                if j < obj_end && bytes[j] == b':' && &content[start..ident_end] == key {
                    let mut value_start = j + 1;
                    while value_start < obj_end && bytes[value_start].is_ascii_whitespace() {
                        value_start += 1;
                    }
                    let value_end = find_value_end(bytes, j + 1, obj_end);
                    return Some((start, ident_end, value_start, value_end));
                }
                continue;
            }
            _ => i += 1,
        }
    }
    None
}

/// Scan from just after a property's `:` to the `;` that terminates it at brace
/// depth 0 (or the object body end).
fn find_value_end(bytes: &[u8], mut i: usize, obj_end: usize) -> usize {
    let mut depth: i32 = 0;
    while i < obj_end {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'/' if i + 1 < obj_end && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
                continue;
            }
            b'/' if i + 1 < obj_end && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b';' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    obj_end
}

/// Byte offset just after the `{` that opens the body of `type_name`'s declaration.
fn find_type_body_start(content: &str, type_name: &str) -> Option<usize> {
    for prefix in ["export interface ", "export type "] {
        let needle = format!("{prefix}{type_name}");
        let mut from = 0;
        while let Some(rel) = content[from..].find(&needle) {
            let idx = from + rel;
            let after = idx + needle.len();
            // Ensure we matched the whole identifier, not a prefix (e.g. `AlbumQuery`
            // must not match `AlbumQueryVariables`).
            let boundary_ok = content[after..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            if boundary_ok && let Some(brace_rel) = content[after..].find('{') {
                return Some(after + brace_rel + 1);
            }
            from = after;
        }
    }
    None
}

/// Given the byte offset just after a `{`, return the offset of the matching `}`.
fn matching_brace(bytes: &[u8], after_open: usize) -> Option<usize> {
    let mut i = after_open;
    let mut depth = 1i32;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// From a quote byte at `i`, return the offset just past the closing quote.
fn skip_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b if b == quote => return j + 1,
            _ => j += 1,
        }
    }
    bytes.len()
}

/// From `/*` at `i`, return the offset just past the closing `*/`.
fn skip_block_comment(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < bytes.len() {
        if bytes[j] == b'*' && bytes[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    bytes.len()
}

/// From `//` at `i`, return the offset just past the end of the line.
fn skip_line_comment(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j < bytes.len() && bytes[j] != b'\n' {
        j += 1;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed but structurally faithful copy of a generated operation type, covering
    // nested objects, `Array<{}>`, doc comments, an aliased field, and a sibling
    // `…Variables` type whose name shares a prefix with the operation type.
    const SAMPLE: &str = r#"/* tslint:disable */
export interface AlbumQuery {
  __typename: "Query";
  /**
   * Get a single album identified by its unique ID
   */
  album?: {
    __typename: "Album";
    id: string;
    title: string;
    display?: {
      __typename: "Display";
      image?: {
        __typename: "Image";
        url?: string | null;
      } | null;
    } | null;
    artists?: Array<{
      __typename: "Artist";
      id: string;
      name: string;
    }> | null;
  } | null;
}

export type AlbumQueryVariables = Exact<{
  id: string;
  imageSize: number;
}>;
"#;

    // Faithful copy of an inline-fragment (`... on User`) operation type, where the
    // narrowed field is emitted as a discriminated-union member.
    const UNION_SAMPLE: &str = r#"export interface AuthUserQuery {
  __typename: "Query";
  me?: {
      __typename: "Device";
    }
    | {
      __typename: "PublicAPIClient";
    } | {
      __typename: "User";
      id: string;
      name: string;
    } | null;
}
"#;

    /// Returns the identifier the range points at, for assertions.
    fn ident_at(content: &str, range: Range) -> String {
        let line = content.lines().nth(range.start.line as usize).unwrap();
        let start = range.start.character as usize;
        let end = range.end.character as usize;
        line[start..end].to_string()
    }

    /// Build a path of `Field` steps from plain names.
    fn path(parts: &[&str]) -> Vec<PathStep> {
        parts
            .iter()
            .map(|s| PathStep::Field(s.to_string()))
            .collect()
    }

    #[test]
    fn descends_to_top_level_field() {
        let r = find_field_property(SAMPLE, "AlbumQuery", &path(&["album"])).unwrap();
        assert_eq!(ident_at(SAMPLE, r), "album");
    }

    #[test]
    fn descends_through_nested_objects() {
        let r = find_field_property(SAMPLE, "AlbumQuery", &path(&["album", "display", "image"]))
            .unwrap();
        assert_eq!(ident_at(SAMPLE, r), "image");
        // It must be the deeply nested `image`, not anything shallower.
        let line = SAMPLE.lines().nth(r.start.line as usize).unwrap();
        assert!(line.trim_start().starts_with("image?:"));
    }

    #[test]
    fn resolves_scalar_leaf_in_correct_scope() {
        // `id` exists under both `album` and `artists`; the album path must land on
        // album's `id`, not the one nested in `artists`.
        let r = find_field_property(SAMPLE, "AlbumQuery", &path(&["album", "id"])).unwrap();
        assert_eq!(ident_at(SAMPLE, r), "id");
        let album_id_line = SAMPLE
            .lines()
            .position(|l| l.trim() == "id: string;")
            .unwrap();
        assert_eq!(r.start.line as usize, album_id_line);
    }

    #[test]
    fn descends_into_array_element_object() {
        let r = find_field_property(SAMPLE, "AlbumQuery", &path(&["album", "artists", "name"]))
            .unwrap();
        assert_eq!(ident_at(SAMPLE, r), "name");
    }

    #[test]
    fn resolves_aliased_leaf() {
        let r = find_field_property(
            SAMPLE,
            "AlbumQuery",
            &path(&["album", "display", "image", "url"]),
        )
        .unwrap();
        assert_eq!(ident_at(SAMPLE, r), "url");
    }

    #[test]
    fn unknown_field_in_path_returns_none() {
        assert!(find_field_property(SAMPLE, "AlbumQuery", &path(&["album", "missing"])).is_none());
    }

    #[test]
    fn narrows_into_matching_union_member() {
        // `me { ... on User { name } }` -> the `name` inside the User union member.
        let steps = vec![
            PathStep::Field("me".to_string()),
            PathStep::Narrow("User".to_string()),
            PathStep::Field("name".to_string()),
        ];
        let r = find_field_property(UNION_SAMPLE, "AuthUserQuery", &steps).unwrap();
        assert_eq!(ident_at(UNION_SAMPLE, r), "name");
        // Must be inside the User member (after its `id`), not another member.
        let line = UNION_SAMPLE.lines().nth(r.start.line as usize).unwrap();
        assert!(line.trim_start().starts_with("name:"));
    }

    #[test]
    fn narrow_to_wrong_type_without_member_returns_none() {
        // `Device` member has no `name`, so narrowing there must not match.
        let steps = vec![
            PathStep::Field("me".to_string()),
            PathStep::Narrow("Device".to_string()),
            PathStep::Field("name".to_string()),
        ];
        assert!(find_field_property(UNION_SAMPLE, "AuthUserQuery", &steps).is_none());
    }

    #[test]
    fn type_name_prefix_does_not_match_variables_type() {
        // `AlbumQuery` must resolve into the interface body, not `AlbumQueryVariables`.
        let r = find_field_property(SAMPLE, "AlbumQuery", &path(&["album"])).unwrap();
        // `album` only exists in AlbumQuery; if we'd matched the Variables type this
        // would be None.
        assert_eq!(ident_at(SAMPLE, r), "album");
        // And the Variables type genuinely lacks it.
        assert!(find_field_property(SAMPLE, "AlbumQueryVariables", &path(&["album"])).is_none());
    }
}
