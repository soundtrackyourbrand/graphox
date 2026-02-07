#![allow(dead_code)]

use graphql_rust::DocumentState;
use tower_lsp::lsp_types::{Diagnostic, NumberOrString, Range};

pub fn find_diag_by_code<'a>(diags: &'a [Diagnostic], code: &str) -> Option<&'a Diagnostic> {
    diags.iter().find(|d| match &d.code {
        Some(NumberOrString::String(s)) => s == code,
        _ => false,
    })
}

pub fn find_diag_by_message<'a>(diags: &'a [Diagnostic], message: &str) -> Option<&'a Diagnostic> {
    diags.iter().find(|d| d.message == message)
}

/// Compute a Range for the last occurrence of `token` in `text` using the document's
/// `byte_to_position` helper. Panics if the token isn't found.
pub fn range_for_token(doc: &DocumentState, text: &str, token: &str) -> Range {
    let start_byte = text
        .rfind(token)
        .unwrap_or_else(|| panic!("Token '{}' not found in text", token));
    let end_byte = start_byte + token.len();
    Range {
        start: doc.byte_to_position(start_byte),
        end: doc.byte_to_position(end_byte),
    }
}
