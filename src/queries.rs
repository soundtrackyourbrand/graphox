use std::sync::OnceLock;
use tree_sitter::Query;

pub static TS_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static TSX_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_SYMBOL_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_SEMANTIC_TOKEN_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_DEFINITION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_DESCRIPTION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_COMPLETION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_DIAGNOSTICS_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_REFERENCES_QUERY_CACHE: OnceLock<Query> = OnceLock::new();

pub const GQL_SYMBOL_QUERY: &str = r#"
    (object_type_definition 
        (name) @symbol.name) @symbol.container

    (enum_type_definition 
        (name) @symbol.name) @symbol.container

    (fragment_definition 
        (fragment_name (name) @symbol.name)
        (directives)? @symbol.directives) @symbol.container

    (operation_definition 
        (name) @symbol.name) @symbol.container

    (interface_type_definition 
        (name) @symbol.name) @symbol.container
"#;

pub const SEMANTIC_TOKEN_QUERY: &str = r#"
    (name) @variable
    (named_type) @type
    (string_value) @string
"#;

// Optimized query to find: gql` ... ` or potential /* GraphQL */ ` ... `
// We combine them to let Tree-sitter's engine optimize the search.
pub const TS_GQL_QUERY: &str = r#"
    (call_expression
        function: (identifier) @tag_name
        arguments: (template_string) @gql_content
        (#any-of? @tag_name "gql" "graphql")
    )

    (template_string) @gql_template
"#;

pub const GQL_DEFINITION_QUERY: &str = r#"
    (object_type_definition (name) @name)
    (fragment_definition (fragment_name (name) @name))
    (enum_type_definition (name) @name)
"#;

pub const GQL_DESCRIPTION_QUERY: &str = r#"
    (object_type_definition) @container
    (enum_type_definition) @container
    (fragment_definition) @container
    (comment) @comment
"#;

pub const GQL_DIAGNOSTICS_QUERY: &str = r#"
    (operation_definition) @operation
    (fragment_definition) @fragment
"#;

pub const GQL_COMPLETION_QUERY: &str = r#"
    (operation_definition) @operation
    (fragment_definition) @fragment
    (type_condition) @type_cond
    (named_type) @type_cond
    (fragment_spread) @frag_spread
    (variable) @variable
    (arguments) @args
"#;

pub const GQL_REFERENCES_QUERY: &str = r#"
    (fragment_spread (fragment_name (name) @name)) @reference
    (fragment_definition (fragment_name (name) @name)) @definition
"#;
