use crate::completion::cursor;
use apollo_compiler::ast;
use graphox_core::document::DocumentState;
use ls_types::{InsertTextFormat, Range, TextEdit};

pub fn get_type_before_equals(doc: &DocumentState, cursor_offset: usize) -> Option<ast::Type> {
    let mut eq_pos = cursor_offset;
    // 1. Find the '='
    let mut found_eq = false;
    while eq_pos > 0 {
        let char_idx = doc.rope.byte_to_char(eq_pos - 1);
        let c = doc.rope.char(char_idx);
        if c == '=' {
            eq_pos -= 1;
            found_eq = true;
            break;
        }
        if !c.is_whitespace() {
            // Keep looking for '='
        }
        eq_pos -= 1;
    }
    if !found_eq {
        return None;
    }

    // 2. Find the ':' before '='
    let mut colon_pos = eq_pos;
    while colon_pos > 0 {
        let char_idx = doc.rope.byte_to_char(colon_pos - 1);
        let c = doc.rope.char(char_idx);
        if c == ':' {
            break;
        }
        if matches!(c, '{' | '(' | ')' | '}') {
            return None;
        }
        colon_pos -= 1;
    }

    if colon_pos == 0 {
        return None;
    }

    let type_text = doc.rope.byte_slice(colon_pos..eq_pos).to_string();
    let type_text = type_text.trim();
    if type_text.is_empty() {
        return None;
    }

    apollo_compiler::ast::Type::parse(type_text, "type.graphql").ok()
}

pub fn create_braced_snippet(
    doc: &DocumentState,
    name: &str,
    cursor_offset: usize,
) -> Option<(String, InsertTextFormat, TextEdit)> {
    let line_idx = doc.rope.byte_to_line(cursor_offset);
    let line_start = doc.rope.line_to_byte(line_idx);
    let line_slice = doc.rope.byte_slice(line_start..cursor_offset).to_string();

    let mut indent = String::new();
    for c in line_slice.chars() {
        if c.is_whitespace() {
            indent.push(c);
        } else {
            break;
        }
    }

    let (_prefix_len, start_offset) = cursor::get_prefix_at_cursor(doc, cursor_offset);
    let start_pos = doc.byte_to_position(start_offset);
    let end_pos = doc.byte_to_position(cursor_offset);

    let snippet = format!(
        "{} {{
{}  $0
{}}}",
        name, indent, indent
    );
    let text_edit = TextEdit {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        new_text: snippet.clone(),
    };

    Some((snippet, InsertTextFormat::SNIPPET, text_edit))
}
