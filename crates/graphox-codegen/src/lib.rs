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
                for j in (i + 1)..acronym_end {
                    if has_lower_after && j == acronym_end - 1 {
                        result.extend(chars[j].to_uppercase());
                    } else {
                        result.extend(chars[j].to_lowercase());
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
}

// Re-export specific helpers if they were used publicly, though most seem internal
// For now, let's keep the core API clean
