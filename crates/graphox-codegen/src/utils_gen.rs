use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Schema;
use apollo_compiler::schema::ExtendedType;
use colored::*;
use graphox_core::config::EmitExtensions;
use std::collections::BTreeMap;

use crate::context::FragmentMasking;

pub fn generate_fragment_masking_file(unmask_function_name: &str) -> String {
    format!(
        r#"/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import {{ ResultOf, DocumentTypeDecoration, TypedDocumentNode }} from '@graphql-typed-document-node/core';
import {{ FragmentDefinitionNode }} from 'graphql';
type Incremental<T> = T | {{ [P in keyof T]?: P extends ' $fragmentName' | '__typename' ? T[P] : never }};

export type FragmentType<TDocumentType extends DocumentTypeDecoration<any, any>> = TDocumentType extends DocumentTypeDecoration<
  infer TType,
  any
>
  ? [TType] extends [{{ ' $fragmentName'?: infer TKey }}]
    ? TKey extends string
      ? {{ ' $fragmentRefs'?: {{ [key in TKey]: TType }} }}
      : never
    : never
  : never;

// return non-nullable if `fragmentType` is non-nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: FragmentType<DocumentTypeDecoration<TType, any>>
): TType;
// return nullable if `fragmentType` is undefined
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: FragmentType<DocumentTypeDecoration<TType, any>> | undefined
): TType | undefined;
// return nullable if `fragmentType` is nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: FragmentType<DocumentTypeDecoration<TType, any>> | null
): TType | null;
// return nullable if `fragmentType` is nullable or undefined
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: FragmentType<DocumentTypeDecoration<TType, any>> | null | undefined
): TType | null | undefined;
// return array of non-nullable if `fragmentType` is array of non-nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: Array<FragmentType<DocumentTypeDecoration<TType, any>>>
): Array<TType>;
// return array of nullable if `fragmentType` is array of nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: Array<FragmentType<DocumentTypeDecoration<TType, any>>> | null | undefined
): Array<TType> | null | undefined;
// return readonly array of non-nullable if `fragmentType` is array of non-nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: ReadonlyArray<FragmentType<DocumentTypeDecoration<TType, any>>>
): ReadonlyArray<TType>;
// return readonly array of nullable if `fragmentType` is array of nullable
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: ReadonlyArray<FragmentType<DocumentTypeDecoration<TType, any>>> | null | undefined
): ReadonlyArray<TType> | null | undefined;
export function {}<TType>(
  _documentNode: DocumentTypeDecoration<TType, any>,
  fragmentType: FragmentType<DocumentTypeDecoration<TType, any>> | Array<FragmentType<DocumentTypeDecoration<TType, any>>> | ReadonlyArray<FragmentType<DocumentTypeDecoration<TType, any>>> | null | undefined
): TType | Array<TType> | ReadonlyArray<TType> | null | undefined {{
  return fragmentType as any;
}}

export function makeFragmentData<
  F extends DocumentTypeDecoration<any, any>,
  FT extends ResultOf<F>
>(data: FT, _fragment: F): FragmentType<F> {{
  return data as FragmentType<F>;
}}

export function isFragmentReady<TQuery, TFrag>(
  queryNode: DocumentTypeDecoration<TQuery, any>,
  fragmentNode: TypedDocumentNode<TFrag>,
  data: FragmentType<TypedDocumentNode<Incremental<TFrag>, any>> | null | undefined
): data is FragmentType<typeof fragmentNode> {{
  const deferredFields = (queryNode as {{ __meta__?: {{ deferredFields: Record<string, (keyof TFrag)[]> }} }} ).__meta__
    ?.deferredFields;

  if (!deferredFields) return true;

  const fragDef = fragmentNode.definitions[0] as FragmentDefinitionNode | undefined;
  const fragName = fragDef?.name?.value;

  const fields = (fragName && deferredFields[fragName]) || [];
  return fields.length > 0 && fields.every((field: any) => data && field in data);
}}
"#,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name,
        unmask_function_name
    )
}

pub fn generate_index_content(
    _fragment_masking: &FragmentMasking,
    emit_extensions: EmitExtensions,
    entrypoint_name: &str,
) -> String {
    let mut output = String::with_capacity(256);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    output.push_str(
        "export type { ResultOf, VariablesOf } from \"@graphql-typed-document-node/core\";\n",
    );

    let ext = emit_extensions.as_str();
    output.push_str(&format!(
        "export * from \"./{}{}\";\n",
        entrypoint_name, ext
    ));

    output
}

pub fn generate_possible_types(schema: &apollo_compiler::validation::Valid<Schema>) -> String {
    let mut possible_types: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (name, ty) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        match ty {
            ExtendedType::Object(obj) => {
                for iface in &obj.implements_interfaces {
                    possible_types
                        .entry(iface.to_string())
                        .or_default()
                        .push(name.to_string());
                }
            }
            ExtendedType::Interface(iface) => {
                for iface_impl in &iface.implements_interfaces {
                    possible_types
                        .entry(iface_impl.to_string())
                        .or_default()
                        .push(name.to_string());
                }
            }
            ExtendedType::Union(union) => {
                for member in &union.members {
                    possible_types
                        .entry(name.to_string())
                        .or_default()
                        .push(member.to_string());
                }
            }
            _ => {}
        }
    }

    for values in possible_types.values_mut() {
        values.sort();
    }

    let mut output = String::new();
    output.push_str("/* tslint:disable */\n");
    output.push_str("/* eslint-disable */\n");
    output.push_str("// This file was automatically generated and should not be edited.\n\n");

    output.push_str("export interface PossibleTypesResultData {\n");
    output.push_str("  possibleTypes: { [key: string]: string[] }\n");
    output.push_str("}\n\n");

    output.push_str("const result: PossibleTypesResultData = {\n");
    output.push_str("  possibleTypes: {\n");

    let entries: Vec<_> = possible_types.iter().collect();
    for (i, (name, impls)) in entries.iter().enumerate() {
        let comma = if i + 1 < entries.len() { "," } else { "" };
        output.push_str(&format!("    \"{}\": [", name));
        let impls_str: Vec<String> = impls.iter().map(|s| format!("\"{}\"", s)).collect();
        output.push_str(&impls_str.join(", "));
        output.push(']');
        output.push_str(comma);
        output.push('\n');
    }

    output.push_str("  },\n");
    output.push_str("};\n\n");
    output.push_str("export default result;\n");

    output
}

pub fn generate_type_policies(schema: &apollo_compiler::validation::Valid<Schema>) -> String {
    let mut type_names: Vec<_> = schema
        .types
        .keys()
        .filter(|n| !n.starts_with("__"))
        .filter(|n| {
            matches!(
                schema.types.get(n.as_str()),
                Some(ExtendedType::Object(_)) | Some(ExtendedType::Interface(_))
            )
        })
        .collect();
    type_names.sort();

    let mut output = String::new();
    output.push_str("/* tslint:disable */\n");
    output.push_str("/* eslint-disable */\n");
    output.push_str("// This file was automatically generated and should not be edited.\n\n");

    output.push_str(
        "import { FieldPolicy, FieldReadFunction, TypePolicies, TypePolicy } from '@apollo/client/cache';\n\n",
    );

    for name in &type_names {
        let key_specifier_name = format!("{}KeySpecifier", name);
        output.push_str(&format!("export type {} = (", key_specifier_name));

        let fields: Vec<_> = match schema.types.get(name.as_str()) {
            Some(ExtendedType::Object(obj)) => obj.fields.keys().collect(),
            Some(ExtendedType::Interface(iface)) => iface.fields.keys().collect(),
            _ => Vec::new(),
        };

        let mut first = true;
        for field in &fields {
            if !first {
                output.push_str(" | ");
            }
            output.push_str(&format!("'{}'", field));
            first = false;
        }
        output.push_str(&format!(" | {})[];\n", key_specifier_name));

        let field_policy_name = format!("{}FieldPolicy", name);
        output.push_str(&format!("export type {} = {{\n", field_policy_name));
        for field in &fields {
            output.push_str(&format!(
                "  {}?: FieldPolicy<any> | FieldReadFunction<any>,\n",
                field
            ));
        }
        output.push_str("};\n\n");
    }

    output.push_str("export type StrictTypedTypePolicies = {\n");
    for (i, name) in type_names.iter().enumerate() {
        let comma = if i + 1 < type_names.len() { "," } else { "" };
        let key_specifier_name = format!("{}KeySpecifier", name);
        let field_policy_name = format!("{}FieldPolicy", name);
        output.push_str(&format!(
            "  {}?: Omit<TypePolicy, \"fields\" | \"keyFields\"> & {{\n",
            name
        ));
        output.push_str(&format!(
            "    keyFields?: false | {} | (() => undefined | {}),\n",
            key_specifier_name, key_specifier_name
        ));
        output.push_str(&format!("    fields?: {},\n", field_policy_name));
        output.push_str("  }");
        output.push_str(comma);
        output.push('\n');
    }
    output.push_str("};\n\n");

    output.push_str("export type TypedTypePolicies = StrictTypedTypePolicies & TypePolicies;\n");

    output
}

pub fn emit_permission_data_content(
    schema: &apollo_compiler::validation::Valid<Schema>,
    _scalars: &HashMap<String, String>,
    schema_import: &Option<String>,
) -> String {
    let mut output = String::with_capacity(2048);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let mut types_with_permissions = Vec::new();
    let mut names: Vec<_> = schema.types.keys().collect();
    names.sort();

    for name in names {
        if name.starts_with("__") {
            continue;
        }
        let ty = schema.types.get(name).unwrap();
        let fields = match ty {
            ExtendedType::Object(obj) => Some(&obj.fields),
            ExtendedType::Interface(iface) => Some(&iface.fields),
            _ => None,
        };

        if let Some(fields) = fields
            && let Some(permissions_field) = fields.get("permissions")
        {
            let inner_name = permissions_field.ty.inner_named_type();
            let inner_type = schema.types.get(inner_name.as_str());
            if let Some(ExtendedType::Enum(_)) = inner_type {
                types_with_permissions.push((name, permissions_field));
            } else {
                eprintln!(
                    "{}: Type '{}' has a 'permissions' field, but its type '{}' is not an enum. Skipping permissions generation for this type.",
                    "Warning".yellow(),
                    name.blue(),
                    inner_name.blue()
                );
            }
        }
    }

    if types_with_permissions.is_empty() {
        output.push_str("export interface PermissionTypes {}\n\n");
        output.push_str("export const permissionTypes = {};\n");
        return output;
    }

    if let Some(import_path) = schema_import {
        let mut types_to_import = HashSet::default();
        for (_, field) in &types_with_permissions {
            let inner_name = field.ty.inner_named_type();
            types_to_import.insert(inner_name.to_string());
        }
        if !types_to_import.is_empty() {
            let mut sorted_imports: Vec<_> = types_to_import.into_iter().collect();
            sorted_imports.sort();
            output.push_str(&format!(
                "import type {{ {} }} from \"{}\";\n\n",
                sorted_imports.join(", "),
                import_path
            ));
        }
    }

    output.push_str("export interface PermissionTypes {\n");
    for (typename, field) in &types_with_permissions {
        let inner_name = field.ty.inner_named_type();
        let ts_type = inner_name.to_string();
        output.push_str(&format!("  {}: {};\n", typename, ts_type));
    }
    output.push_str("}\n\n");

    output.push_str("export const permissionTypes = {\n");
    for (typename, field) in &types_with_permissions {
        let inner_name = field.ty.inner_named_type();
        if let Some(ExtendedType::Enum(enm)) = schema.types.get(inner_name.as_str()) {
            let mut values: Vec<_> = enm.values.keys().collect();
            values.sort();
            let values_str = values
                .iter()
                .map(|v| format!("'{}'", v))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!("  {}: [{}],\n", typename, values_str));
        }
    }
    output.push_str("}\n");

    output
}
