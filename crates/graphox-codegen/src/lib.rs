pub mod context;
pub mod entrypoint;
pub mod helpers;
pub mod schema_types;
pub mod selection_set;
pub mod typescript;
pub mod utils_gen;

pub use context::*;
pub use entrypoint::*;
pub use schema_types::*;
pub use typescript::*;
pub use utils_gen::*;

pub fn apply_naming_convention(
    name: &str,
    convention: &graphox_core::config::NamingConvention,
) -> String {
    match convention {
        graphox_core::config::NamingConvention::Preserve => name.to_string(),
        graphox_core::config::NamingConvention::PascalCase => to_pascal_case(name),
    }
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if c == '_' || c == '-' || c == ' ' {
            if c == '_' {
                let input_before_underscore = &chars[..i];
                let has_uppercase_before =
                    input_before_underscore.iter().any(|ch| ch.is_uppercase());

                if has_uppercase_before {
                    result.push('_');
                    i += 1;
                    if i < len && chars[i].is_alphabetic() {
                        result.extend(chars[i].to_uppercase());
                        i += 1;
                    }
                    continue;
                }
            }
            i += 1;
            if i < len && chars[i].is_alphabetic() {
                result.extend(chars[i].to_uppercase());
                i += 1;
            }
            continue;
        }

        if c.is_uppercase() {
            let mut acronym_end = i + 1;
            while acronym_end < len
                && chars[acronym_end].is_uppercase()
                && chars[acronym_end].is_alphabetic()
            {
                acronym_end += 1;
            }

            if acronym_end > i + 1 {
                let has_lower_after = acronym_end < len && chars[acronym_end].is_lowercase();

                result.extend(c.to_uppercase());
                for (j, &ch) in chars.iter().enumerate().take(acronym_end).skip(i + 1) {
                    if has_lower_after && j == acronym_end - 1 {
                        result.extend(ch.to_uppercase());
                    } else {
                        result.extend(ch.to_lowercase());
                    }
                }
                i = acronym_end;
                continue;
            }

            result.extend(c.to_uppercase());
            i += 1;
            continue;
        }

        if c.is_alphabetic() || c.is_numeric() {
            if result.is_empty() {
                result.extend(c.to_uppercase());
            } else {
                result.push(c);
            }
            i += 1;
            continue;
        }

        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::TypeCacheKey;

    #[test]
    fn test_pascal_case_basic() {
        assert_eq!(to_pascal_case("get_user_by_id"), "GetUserById");
        assert_eq!(to_pascal_case("create_post"), "CreatePost");
        assert_eq!(to_pascal_case("user_fields"), "UserFields");
    }

    #[test]
    fn test_pascal_case_acronyms() {
        assert_eq!(to_pascal_case("generateOTP"), "GenerateOtp");
        assert_eq!(to_pascal_case("SAMLConfig"), "SamlConfig");
    }

    #[test]
    fn test_pascal_case_underscore_preservation() {
        assert_eq!(
            to_pascal_case("AddTracks_CreateManualPlaylist"),
            "AddTracks_CreateManualPlaylist"
        );
        assert_eq!(to_pascal_case("ChangePlan_account"), "ChangePlan_Account");
        assert_eq!(to_pascal_case("ChangePlan_prices"), "ChangePlan_Prices");
    }

    #[test]
    fn test_type_cache_key_equality() {
        use ahash::AHashMap as HashMap;
        let key1 = TypeCacheKey::from_context("User", false, &None, &HashMap::new());
        let key2 = TypeCacheKey::from_context("User", false, &None, &HashMap::new());
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_type_cache_key_inequality_use_names() {
        use ahash::AHashMap as HashMap;
        let key_no_names = TypeCacheKey::from_context("User", false, &None, &HashMap::new());
        let key_with_names = TypeCacheKey::from_context("User", true, &None, &HashMap::new());
        assert_ne!(key_no_names, key_with_names);
    }

    #[test]
    fn test_type_cache_key_inequality_schema_import() {
        use ahash::AHashMap as HashMap;
        let key_no_import = TypeCacheKey::from_context("User", false, &None, &HashMap::new());
        let key_with_import = TypeCacheKey::from_context(
            "User",
            false,
            &Some("./schema".to_string()),
            &HashMap::new(),
        );
        assert_ne!(key_no_import, key_with_import);
    }

    #[test]
    fn test_type_cache_key_inequality_type_imports() {
        use ahash::AHashMap as HashMap;
        let empty: HashMap<String, String> = HashMap::new();
        let mut with_type: HashMap<String, String> = HashMap::new();
        with_type.insert("User".to_string(), "./types".to_string());

        let key_empty = TypeCacheKey::from_context("User", false, &None, &empty);
        let key_with_type = TypeCacheKey::from_context("User", false, &None, &with_type);
        assert_ne!(key_empty, key_with_type);
    }

    #[test]
    fn test_type_cache_separation() {
        use crate::context::TypeCache;
        use ahash::AHashMap as HashMap;

        let cache = TypeCache::new();

        let key1 = TypeCacheKey::from_context("User", false, &None, &HashMap::new());
        let key2 = TypeCacheKey::from_context("User", true, &None, &HashMap::new());

        let result1 = cache.get_or_insert_tuple(key1.clone(), || "inline_enum".to_string());
        assert_eq!(result1, "inline_enum");

        let result2 = cache.get_or_insert_tuple(key2.clone(), || "type_name".to_string());
        assert_eq!(result2, "type_name");

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_abstract_members_union_returns_no_quotes() {
        use apollo_compiler::Schema;

        let schema_text = r#"
            type Query {
                item: Item
            }
            union Item = A | B
            type A { id: ID! }
            type B { name: String }
        "#;

        let schema = Schema::parse(schema_text, "test.graphql").unwrap();
        let members = crate::helpers::compute_abstract_members("Item", &schema);
        assert!(!members.is_empty(), "Union should have members");
        for member in &members {
            assert!(
                !member.starts_with('"'),
                "Union member '{}' should NOT have quotes",
                member
            );
        }
    }

    #[test]
    fn test_abstract_members_interface_returns_with_quotes() {
        use apollo_compiler::Schema;

        let schema_text = r#"
            type Query {
                node: Node
            }
            interface Node {
                id: ID!
            }
            type A implements Node { id: ID! }
            type B implements Node { id: ID! }
        "#;

        let schema = Schema::parse(schema_text, "test.graphql").unwrap();
        let members = crate::helpers::compute_abstract_members("Node", &schema);
        assert!(!members.is_empty(), "Interface should have implementors");
        for member in &members {
            assert!(
                member.starts_with('"'),
                "Interface implementor '{}' SHOULD have quotes",
                member
            );
        }
    }

    #[test]
    fn test_typename_value_for_type_union_uses_default() {
        use apollo_compiler::Schema;

        let schema_text = r#"
            type Query {
                item: Item
            }
            union Item = A | B
            type A { id: ID! }
            type B { name: String }
        "#;

        let schema = Schema::parse(schema_text, "test.graphql").unwrap();
        let union_type = schema.types.get("Item").unwrap();

        let result = crate::helpers::get_typename_value_for_type(union_type, &schema);
        assert!(
            result.starts_with('"'),
            "Union typename should be quoted string literal, got: {}",
            result
        );
    }

    #[test]
    fn test_typename_value_for_type_interface_uses_implementors() {
        use apollo_compiler::Schema;

        let schema_text = r#"
            type Query {
                node: Node
            }
            interface Node {
                id: ID!
            }
            type A implements Node { id: ID! }
            type B implements Node { id: ID! }
        "#;

        let schema = Schema::parse(schema_text, "test.graphql").unwrap();
        let interface_type = schema.types.get("Node").unwrap();

        let result = crate::helpers::get_typename_value_for_type(interface_type, &schema);
        assert!(
            result.contains('"'),
            "Interface typename should contain quoted implementors, got: {}",
            result
        );
        assert!(
            result.contains("A") && result.contains("B"),
            "Interface typename should contain implementors A and B, got: {}",
            result
        );
    }
}
// Re-export specific helpers if they were used publicly, though most seem internal
// For now, let's keep the core API clean
