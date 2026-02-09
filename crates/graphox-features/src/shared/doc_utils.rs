use graphox_core::document::DocumentState;
use graphox_core::queries::*;
use tree_sitter::{Node, StreamingIterator};

pub fn find_description(doc: &DocumentState, target_name: &str) -> Option<String> {
    let query = GQL_DESCRIPTION_QUERY_CACHE.get_or_init(|| {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        tree_sitter::Query::new(&lang, GQL_DESCRIPTION_QUERY).unwrap()
    });

    let mut cursor = tree_sitter::QueryCursor::new();

    for block in doc.get_graphql_trees() {
        let offset = block.offset;
        let mut matches = cursor.matches(query, block.tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();
            doc.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            let node = m.captures[0].node;
            if node.kind() == "comment" {
                continue;
            }

            let container = node;
            let mut name = None;
            let mut description = None;

            let mut cursor = container.walk();
            for child in container.children(&mut cursor) {
                match child.kind() {
                    "name" => {
                        name = Some(doc.get_node_text(child, offset));
                    }
                    "enum_value" => {
                        if let Some(n) = child.child_by_field_name("name") {
                            name = Some(doc.get_node_text(n, offset));
                        } else if let Some(n) = child.child(0) {
                            name = Some(doc.get_node_text(n, offset));
                        }
                    }
                    "fragment_name" => {
                        if let Some(n) = child.child_by_field_name("name") {
                            name = Some(doc.get_node_text(n, offset));
                        } else if let Some(n) = child.child(0) {
                            name = Some(doc.get_node_text(n, offset));
                        }
                    }
                    "description" => {
                        if let Some(sv) = child.child_by_field_name("content") {
                            description = Some(doc.get_node_text(sv, offset));
                        } else if let Some(sv) = child.child(0) {
                            description = Some(doc.get_node_text(sv, offset));
                        }
                    }
                    "string_value" => {
                        description = Some(doc.get_node_text(child, offset));
                    }
                    _ => {}
                }
            }

            if description.is_none() {
                // Try to find preceding comment by looking at the line above
                let range = doc.translate_to_file_range(container, offset);
                if range.start.line > 0 {
                    let prev_line_num = range.start.line - 1;
                    let line_start = doc.rope.line_to_char(prev_line_num as usize);
                    let line_end = doc.rope.line_to_char(range.start.line as usize);
                    let line_text = doc.rope.slice(line_start..line_end).to_string();
                    let trimmed = line_text.trim();
                    if trimmed.starts_with('#') {
                        description = Some(trimmed.trim_start_matches('#').trim().to_string());
                    }
                }
            }

            if let Some(n) = name
                && n == target_name
            {
                return description.map(|d| d.trim_matches('"').to_string());
            }
        }
    }
    None
}
