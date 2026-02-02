use std::sync::OnceLock;
use tree_sitter::Query;

pub static TS_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_SYMBOL_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_SEMANTIC_TOKEN_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_DEFINITION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_DESCRIPTION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
pub static GQL_VALIDATION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();

pub const GQL_SYMBOL_QUERY: &str = r#"
    (object_type_definition 
        (name) @symbol.name) @symbol.container

    (enum_type_definition 
        (name) @symbol.name) @symbol.container

    (fragment_definition 
        (fragment_name (name) @symbol.name)) @symbol.container

    (interface_type_definition 
        (name) @symbol.name) @symbol.container
"#;

pub const SEMANTIC_TOKEN_QUERY: &str = r#"
    (name) @variable
    (named_type) @type
    (string_value) @string
"#;

// A query to find: gql` ... `
pub const TS_GQL_QUERY: &str = r#"
    (call_expression
        function: (identifier) @tag_name
        arguments: (template_string) @gql_content
        (#eq? @tag_name "gql")
    )
"#;

pub const GQL_DEFINITION_QUERY: &str = r#"
    (object_type_definition (name) @name)
    (fragment_definition (fragment_name (name) @name))
    (enum_type_definition (name) @name)
"#;

pub const GQL_DESCRIPTION_QUERY: &str = r#"
    (object_type_definition (description (string_value))? @desc (name) @name)
    (enum_type_definition (description (string_value))? @desc (name) @name)
"#;

pub const GQL_VALIDATION_QUERY: &str = r#"
    (operation_definition) @operation
    (fragment_definition) @fragment
    (type_condition) @type_cond
    (fragment_spread) @frag_spread
    (inline_fragment) @inline_frag
    (variable) @variable
    (arguments) @args
"#;
