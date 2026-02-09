use graphox_core::document::DocumentState;
use tree_sitter::Node;

pub fn is_after_at(doc: &DocumentState, cursor_offset: usize) -> bool {
    if cursor_offset == 0 {
        return false;
    }
    doc.rope.char(doc.rope.byte_to_char(cursor_offset - 1)) == '@'
}

pub fn is_after_dots(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut dot_count = 0;
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        if c == '.' {
            dot_count += 1;
            curr -= 1;
            if dot_count == 3 {
                return true;
            }
            continue;
        }
        break;
    }
    false
}

pub fn is_after_on(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut found_n = false;
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        if !found_n {
            if c == 'n' || c == 'N' {
                found_n = true;
                curr -= 1;
                continue;
            }
            return false;
        } else {
            if c == 'o' || c == 'O' {
                if curr > 1 {
                    let prev = doc.rope.char(doc.rope.byte_to_char(curr - 2));
                    return !is_name_char(prev);
                }
                return true;
            }
            return false;
        }
    }
    false
}

pub fn is_after_pipe(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        return c == '|';
    }
    false
}

pub fn is_after_implements(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        break;
    }

    let target = "implements";
    let target_len = target.len();
    if curr < target_len {
        return false;
    }

    let start_byte = curr - target_len;
    let slice = doc.rope.byte_slice(start_byte..curr).to_string();
    if slice == target {
        if start_byte > 0 {
            let prev = doc.rope.char(doc.rope.byte_to_char(start_byte - 1));
            return !is_name_char(prev);
        }
        return true;
    }
    false
}

pub fn is_after_directive_open_paren(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        return c == '(';
    }
    false
}

pub fn get_word_prefix_before_paren(doc: &DocumentState, cursor_offset: usize) -> Option<String> {
    let mut curr = cursor_offset;
    // skip whitespace and (
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() || c == '(' {
            curr -= 1;
            continue;
        }
        break;
    }

    let end = curr;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if is_name_char(c) {
            curr -= 1;
            continue;
        }
        break;
    }

    if curr < end && curr > 0 && doc.rope.char(doc.rope.byte_to_char(curr - 1)) == '@' {
        Some(doc.rope.byte_slice(curr..end).to_string())
    } else {
        None
    }
}

pub fn is_after_equals_in_variable(doc: &DocumentState, cursor_offset: usize, node: &Node) -> bool {
    // Check if we're in a variable_definition context
    if node.kind() != "variable_definition" {
        return false;
    }
    is_after_equals(doc, cursor_offset)
}

pub fn is_after_equals_in_argument(doc: &DocumentState, cursor_offset: usize, node: &Node) -> bool {
    // Check if we're in an argument definition context
    if !matches!(node.kind(), "input_value_definition" | "argument") {
        return false;
    }
    is_after_equals(doc, cursor_offset)
}

pub fn is_after_equals(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut curr = cursor_offset;
    while curr > 0 {
        let char_idx = doc.rope.byte_to_char(curr - 1);
        let c = doc.rope.char(char_idx);
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        return c == '=';
    }
    false
}

pub fn is_after_colon_in_selection(doc: &DocumentState, cursor_offset: usize) -> bool {
    let mut curr = cursor_offset;
    while curr > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(curr - 1));
        if c.is_whitespace() {
            curr -= 1;
            continue;
        }
        return c == ':';
    }
    false
}

pub fn is_after_question_mark(doc: &DocumentState, cursor_offset: usize) -> bool {
    if cursor_offset == 0 {
        return false;
    }
    let c = doc.rope.char(doc.rope.byte_to_char(cursor_offset - 1));
    c == '?'
}

pub fn is_operation_type_position(
    _doc: &DocumentState,
    _cursor_offset: usize,
    node: &Node,
) -> bool {
    // Trigger after typing prefixes like 'qu', 'mu', 'su'
    node.kind() == "operation_type"
}

pub fn is_schema_definition_position(
    _doc: &DocumentState,
    _cursor_offset: usize,
    node: &Node,
) -> bool {
    // Trigger in schema_document or at root level for schema keywords
    matches!(
        node.kind(),
        "schema_document" | "schema_definition" | "document"
    )
}

pub fn get_word_prefix_at_cursor(doc: &DocumentState, cursor_offset: usize) -> Option<String> {
    let mut start = cursor_offset;
    while start > 0 {
        let c = doc.rope.char(doc.rope.byte_to_char(start - 1));
        if !is_name_char(c) {
            break;
        }
        start -= 1;
    }

    if start == cursor_offset {
        return None;
    }

    Some(doc.rope.byte_slice(start..cursor_offset).to_string())
}

pub fn get_prefix_at_cursor(doc: &DocumentState, cursor_offset: usize) -> (usize, usize) {
    let max_scan = 64usize;
    let search_start = cursor_offset.saturating_sub(max_scan);
    let slice = doc.rope.byte_slice(search_start..cursor_offset).to_string();
    let bytes = slice.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        let b = bytes[i - 1];
        if b == b'_' || b.is_ascii_alphanumeric() {
            i -= 1;
            continue;
        }
        break;
    }
    (bytes.len() - i, search_start + i)
}

pub fn is_name_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}
