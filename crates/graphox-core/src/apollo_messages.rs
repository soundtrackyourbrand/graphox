//! Recognising apollo-compiler diagnostics by message.
//!
//! apollo-compiler exposes no machine-readable identity for its diagnostics:
//! `DiagnosticData` is `pub(crate)`, `GraphQLError::extensions` is always
//! empty, and `unstable_to_json_compat` is `#[doc(hidden)]` and documented as
//! supported only for the Apollo Router. The message text is all we have.
//!
//! Two rules make that as safe as it can be:
//!
//! 1. Match the **summary line only**. `Diagnostic::to_string()` renders a full
//!    ariadne report — labels, notes, and excerpts of the user's query and of
//!    the schema. Matching the whole report means a field or alias named
//!    `unused` suppresses every diagnostic for that block, which is a silent
//!    loss of real errors.
//! 2. Every pattern is covered by a canary test in
//!    `tests/apollo_message_contract.rs`, which triggers the rule through
//!    apollo and asserts the pattern still matches. If a message is reworded
//!    upstream the tests fail rather than diagnostics quietly changing.

/// One recognised diagnostic. All `needles` must be present for a match, which
/// covers the messages that are only unambiguous in combination.
pub struct MessageRule {
    /// Identifier used by the contract tests.
    pub name: &'static str,
    pub needles: &'static [&'static str],
}

impl MessageRule {
    fn matches(&self, summary: &str) -> bool {
        self.needles.iter().all(|n| summary.contains(n))
    }
}

/// The summary line of a rendered diagnostic — everything before the source
/// excerpt begins.
pub fn summary(rendered: &str) -> &str {
    rendered.lines().next().unwrap_or("")
}

/// Diagnostics graphox reports itself, usually with cross-file knowledge that
/// apollo does not have when handed a single block.
pub const HANDLED_BY_GRAPHOX: &[MessageRule] = &[
    // Reported by our own unique-operation-name rule, which sees the whole file.
    MessageRule {
        name: "duplicate_operation",
        needles: &["defined multiple times"],
    },
    // A fragment defined in another file is not missing; we resolve those.
    MessageRule {
        name: "fragment_not_found",
        needles: &["cannot find fragment"],
    },
    // Likewise for a fragment used only from another file.
    MessageRule {
        name: "fragment_unused",
        needles: &["must be used in an operation"],
    },
    // We emit a friendlier version with a "did you mean" suggestion.
    MessageRule {
        name: "unknown_field",
        needles: &["does not have a field"],
    },
    // Blocks are extracted per-operation, so definitions look misplaced to apollo.
    MessageRule {
        name: "type_system_in_executable",
        needles: &["must not contain"],
    },
    MessageRule {
        name: "alias_type_conflict",
        needles: &["must not select different types using the same name"],
    },
    MessageRule {
        name: "alias_field_conflict",
        needles: &["cannot select different fields into the same alias"],
    },
    MessageRule {
        name: "conflicting_arguments",
        needles: &["conflicting field arguments"],
    },
    MessageRule {
        name: "recursive_fragment",
        needles: &["cannot reference itself"],
    },
    // Kept deliberately broad, matching the previous behaviour. graphox has its
    // own directives (`@type_only` and friends) that user schemas do not
    // declare, and client-side directives such as `@client` are commonly
    // undeclared too. Narrowing this to just our own directives would start
    // reporting those, which is a product decision rather than a refactor.
    MessageRule {
        name: "unknown_directive",
        needles: &["cannot find directive"],
    },
    MessageRule {
        name: "undefined_variable",
        needles: &["variable", "is not defined"],
    },
    MessageRule {
        name: "unused_variable",
        needles: &["unused variable"],
    },
    MessageRule {
        name: "variable_type_mismatch",
        needles: &["variable", "cannot be used for argument"],
    },
];

/// A fragment name defined more than once across the workspace. The engine
/// treats this as fatal for fragment resolution, so it is matched separately
/// from the diagnostics that are merely suppressed. Operation-name collisions
/// share the "defined multiple times" wording and must not match here, which is
/// why the fragment wording is part of the needle.
pub const DUPLICATE_FRAGMENT: &[MessageRule] = &[
    MessageRule {
        name: "duplicate_fragment",
        needles: &["the fragment", "defined multiple times"],
    },
    MessageRule {
        name: "duplicate_fragment_name",
        needles: &["Duplicate fragment name"],
    },
];

/// Build errors tolerated because codegen accepts these documents: a block may
/// legitimately contain type-system definitions we ignore.
pub const TOLERATED_BUILD_ERRORS: &[MessageRule] = &[MessageRule {
    name: "type_system_in_executable",
    needles: &["must not contain"],
}];

/// Whether graphox reports this diagnostic itself, so apollo's copy is dropped.
pub fn is_handled_by_graphox(rendered: &str) -> bool {
    let s = summary(rendered);
    HANDLED_BY_GRAPHOX.iter().any(|r| r.matches(s))
}

/// Whether this diagnostic reports a duplicated fragment name.
pub fn is_duplicate_fragment(rendered: &str) -> bool {
    let s = summary(rendered);
    DUPLICATE_FRAGMENT.iter().any(|r| r.matches(s))
}

/// Whether this build error is tolerated rather than surfaced.
pub fn is_tolerated_build_error(rendered: &str) -> bool {
    let s = summary(rendered);
    TOLERATED_BUILD_ERRORS.iter().any(|r| r.matches(s))
}
