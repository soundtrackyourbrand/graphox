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
pub static GQL_MERGE_QUERY_CACHE: OnceLock<Query> = OnceLock::new();

pub const GQL_SYMBOL_QUERY: &str = r#"
    (object_type_definition 
        (name) @symbol.name) @symbol.container

    (enum_type_definition 
        (name) @symbol.name) @symbol.container

    (interface_type_definition 
        (name) @symbol.name) @symbol.container

    (union_type_definition 
        (name) @symbol.name) @symbol.container

    (input_object_type_definition 
        (name) @symbol.name) @symbol.container

    (scalar_type_definition 
        (name) @symbol.name) @symbol.container

    (fragment_definition 
        (fragment_name (name) @symbol.name)
        (type_condition (named_type (name) @symbol.type_condition))
        (directives)? @symbol.directives) @symbol.container @symbol.full

    (operation_definition 
        (name)? @symbol.name) @symbol.container @symbol.full

    (directive_definition
        (name) @symbol.name) @symbol.container

    (type_extension
        (object_type_extension
            (name) @symbol.name) @symbol.container)

    (type_extension
        (interface_type_extension
            (name) @symbol.name) @symbol.container)

    (type_extension
        (enum_type_extension
            (name) @symbol.name) @symbol.container)

    (type_extension
        (scalar_type_extension
            (name) @symbol.name) @symbol.container)

    (type_extension
        (union_type_extension
            (name) @symbol.name) @symbol.container)

    (type_extension
        (input_object_type_extension
            (name) @symbol.name) @symbol.container)
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
        arguments: [
            (arguments (template_string) @gql_content)
            (template_string) @gql_content
        ]
        (#any-of? @tag_name "gql" "graphql")
    )

    (template_string) @gql_template
"#;

pub const GQL_DEFINITION_QUERY: &str = r#"
    (object_type_definition (name) @name)
    (interface_type_definition (name) @name)
    (enum_type_definition (name) @name)
    (union_type_definition (name) @name)
    (input_object_type_definition (name) @name)
    (scalar_type_definition (name) @name)
    (fragment_definition (fragment_name (name) @name))
    (variable_definition (variable) @name)
"#;

pub const GQL_DESCRIPTION_QUERY: &str = r#"
    (object_type_definition) @container
    (enum_type_definition) @container
    (fragment_definition) @container
    (enum_value_definition) @container
    (field_definition) @container
    (input_value_definition) @container
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
    ;; Fragment usages/definitions
    (fragment_spread (fragment_name (name) @name)) @reference
    (fragment_definition (fragment_name (name) @name)) @definition

    ;; Variables
    (variable) @name @reference
    (variable_definition (variable) @name) @definition

    ;; Field selections and field definitions
    (field (name) @name) @reference
    (field_definition (name) @name) @definition

    ;; Named type usages and type definitions
    (named_type (name) @name) @reference
    (object_type_definition (name) @name) @definition
    (interface_type_definition (name) @name) @definition
    (enum_type_definition (name) @name) @definition
    (union_type_definition (name) @name) @definition
    (input_object_type_definition (name) @name) @definition
    (scalar_type_definition (name) @name) @definition

    ;; Arguments and input value definitions
    (argument (name) @name) @reference
    (input_value_definition (name) @name) @definition

    ;; Directives (usage and definition)
    (directive (name) @name) @reference
    (directive_definition (name) @name) @definition

    ;; Enum value usages and definitions
    (enum_value (name) @name) @reference
    (enum_value_definition (enum_value (name) @name)) @definition

    ;; Operation names
    (operation_definition (name) @name) @definition
"#;

pub const GQL_MERGE_QUERY: &str = r#"
    [
        (object_type_definition (name) @name)
        (interface_type_definition (name) @name)
        (enum_type_definition (name) @name)
        (scalar_type_definition (name) @name)
        (union_type_definition (name) @name)
        (input_object_type_definition (name) @name)

        (type_extension (object_type_extension (name) @name))
        (type_extension (interface_type_extension (name) @name))
        (type_extension (enum_type_extension (name) @name))
        (type_extension (scalar_type_extension (name) @name))
        (type_extension (union_type_extension (name) @name))
        (type_extension (input_object_type_extension (name) @name))
    ] @type_def
"#;
