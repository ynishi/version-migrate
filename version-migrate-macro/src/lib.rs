use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Meta};

/// Derives the `Versioned` trait for a struct.
///
/// # Attributes
///
/// - `#[versioned(version = "x.y.z")]`: Specifies the semantic version (required).
///   The version string must be a valid semantic version.
/// - `#[versioned(version_key = "...")]`: Customizes the version field key (optional, default: "version").
/// - `#[versioned(data_key = "...")]`: Customizes the data field key (optional, default: "data").
/// - `#[versioned(auto_tag = true)]`: Auto-generates Serialize/Deserialize with version field (optional, default: false).
///   When enabled, the version field is automatically inserted during serialization and validated during deserialization.
/// - `#[versioned(queryable = true)]`: Auto-generates Queryable trait implementation (optional, default: false).
///   Enables use with ConfigMigrator for ORM-like queries.
/// - `#[versioned(queryable_key = "...")]`: Customizes the entity name for Queryable (optional).
///   If not specified, uses the lowercased type name. Only used when `queryable = true`.
///
/// # Examples
///
/// Basic usage:
/// ```ignore
/// use version_migrate::Versioned;
///
/// #[derive(Versioned)]
/// #[versioned(version = "1.0.0")]
/// pub struct Task_V1_0_0 {
///     pub id: String,
///     pub title: String,
/// }
/// ```
///
/// Custom keys:
/// ```ignore
/// #[derive(Versioned)]
/// #[versioned(
///     version = "1.0.0",
///     version_key = "schema_version",
///     data_key = "payload"
/// )]
/// pub struct Task { ... }
/// // When used with Migrator:
/// // Serializes to: {"schema_version":"1.0.0","payload":{...}}
/// ```
///
/// Auto-tag for direct serialization:
/// ```ignore
/// #[derive(Versioned)]
/// #[versioned(version = "1.0.0", auto_tag = true)]
/// pub struct Task {
///     pub id: String,
///     pub title: String,
/// }
///
/// // Use serde directly without Migrator
/// let task = Task { id: "1".into(), title: "Test".into() };
/// let json = serde_json::to_string(&task)?;
/// // → {"version":"1.0.0","id":"1","title":"Test"}
/// ```
///
/// Queryable for ConfigMigrator:
/// ```ignore
/// #[derive(Serialize, Deserialize, Versioned)]
/// #[versioned(version = "2.0.0", queryable = true, queryable_key = "task")]
/// pub struct TaskEntity {
///     pub id: String,
///     pub title: String,
///     pub description: Option<String>,
/// }
///
/// // Now TaskEntity implements Queryable automatically
/// let tasks: Vec<TaskEntity> = config_migrator.query("tasks")?;
/// ```
#[proc_macro_derive(Versioned, attributes(versioned))]
pub fn derive_versioned(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Extract attributes
    let attrs = extract_attributes(&input);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let version = &attrs.version;
    let version_key = &attrs.version_key;
    let data_key = &attrs.data_key;

    let versioned_impl = quote! {
        impl #impl_generics version_migrate::Versioned for #name #ty_generics #where_clause {
            const VERSION: &'static str = #version;
            const VERSION_KEY: &'static str = #version_key;
            const DATA_KEY: &'static str = #data_key;
        }
    };

    let mut impls = vec![versioned_impl];

    if attrs.auto_tag {
        // Generate custom Serialize and Deserialize implementations
        let serialize_impl = generate_serialize_impl(&input, &attrs);
        let deserialize_impl = generate_deserialize_impl(&input, &attrs);
        impls.push(serialize_impl);
        impls.push(deserialize_impl);
    }

    if attrs.queryable {
        // Generate Queryable trait implementation
        let queryable_impl = generate_queryable_impl(&input, &attrs);
        impls.push(queryable_impl);
    }

    let expanded = quote! {
        #(#impls)*
    };

    TokenStream::from(expanded)
}

struct VersionedAttributes {
    version: String,
    version_key: String,
    data_key: String,
    auto_tag: bool,
    queryable: bool,
    queryable_key: Option<String>,
}

fn extract_attributes(input: &DeriveInput) -> VersionedAttributes {
    let mut version = None;
    let mut version_key = String::from("version");
    let mut data_key = String::from("data");
    let mut auto_tag = false;
    let mut queryable = false;
    let mut queryable_key = None;

    for attr in &input.attrs {
        if attr.path().is_ident("versioned") {
            if let Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                parse_versioned_attrs(
                    &tokens,
                    &mut version,
                    &mut version_key,
                    &mut data_key,
                    &mut auto_tag,
                    &mut queryable,
                    &mut queryable_key,
                );
            }
        }
    }

    let version = version.unwrap_or_else(|| {
        panic!("Missing #[versioned(version = \"x.y.z\")] attribute");
    });

    // Validate semver at compile time
    if let Err(e) = semver::Version::parse(&version) {
        panic!("Invalid semantic version '{}': {}", version, e);
    }

    VersionedAttributes {
        version,
        version_key,
        data_key,
        auto_tag,
        queryable,
        queryable_key,
    }
}

fn parse_versioned_attrs(
    tokens: &str,
    version: &mut Option<String>,
    version_key: &mut String,
    data_key: &mut String,
    auto_tag: &mut bool,
    queryable: &mut bool,
    queryable_key: &mut Option<String>,
) {
    // Parse comma-separated key = "value" pairs
    for part in tokens.split(',') {
        let part = part.trim();

        if let Some(val) = parse_attr_value(part, "version") {
            *version = Some(val);
        } else if let Some(val) = parse_attr_value(part, "version_key") {
            *version_key = val;
        } else if let Some(val) = parse_attr_value(part, "data_key") {
            *data_key = val;
        } else if let Some(val) = parse_attr_bool_value(part, "auto_tag") {
            *auto_tag = val;
        } else if let Some(val) = parse_attr_bool_value(part, "queryable") {
            *queryable = val;
        } else if let Some(val) = parse_attr_value(part, "queryable_key") {
            *queryable_key = Some(val);
        }
    }
}

fn parse_attr_value(token: &str, key: &str) -> Option<String> {
    let token = token.trim();
    if let Some(rest) = token.strip_prefix(key) {
        let rest = rest.trim();
        if let Some(rest) = rest.strip_prefix('=') {
            let rest = rest.trim();
            if rest.starts_with('"') && rest.ends_with('"') {
                return Some(rest[1..rest.len() - 1].to_string());
            }
        }
    }
    None
}

fn parse_attr_bool_value(token: &str, key: &str) -> Option<bool> {
    let token = token.trim();
    if let Some(rest) = token.strip_prefix(key) {
        let rest = rest.trim();
        if let Some(rest) = rest.strip_prefix('=') {
            let rest = rest.trim();
            return match rest {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    None
}

fn generate_queryable_impl(
    input: &DeriveInput,
    attrs: &VersionedAttributes,
) -> proc_macro2::TokenStream {
    let name = &input.ident;

    // Determine the entity name
    let entity_name = if let Some(ref key) = attrs.queryable_key {
        key.clone()
    } else {
        // Default: use the type name in lowercase
        name.to_string().to_lowercase()
    };

    quote! {
        impl version_migrate::Queryable for #name {
            const ENTITY_NAME: &'static str = #entity_name;
        }
    }
}

/// Derives the `Queryable` trait for a struct.
///
/// This is a standalone macro for domain entities that need to be queryable
/// via `ConfigMigrator` but don't have version information themselves.
///
/// # Attributes
///
/// - `#[queryable(entity = "name")]`: Specifies the entity name (required).
///   This must match the entity name used when registering migration paths.
///
/// # Examples
///
/// Basic usage:
/// ```ignore
/// use version_migrate::Queryable;
///
/// #[derive(Queryable)]
/// #[queryable(entity = "task")]
/// pub struct TaskEntity {
///     pub id: String,
///     pub title: String,
/// }
///
/// // Now can be used with ConfigMigrator
/// let tasks: Vec<TaskEntity> = config.query("tasks")?;
/// ```
///
/// The entity name must match the Migrator registration:
/// ```ignore
/// let path = Migrator::define("task")  // ← This name
///     .from::<TaskV1>()
///     .into::<TaskEntity>();
///
/// #[derive(Queryable)]
/// #[queryable(entity = "task")]  // ← Must match
/// struct TaskEntity { ... }
/// ```
#[proc_macro_derive(Queryable, attributes(queryable))]
pub fn derive_queryable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut entity_name: Option<String> = None;

    // Extract entity attribute
    for attr in &input.attrs {
        if attr.path().is_ident("queryable") {
            if let Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                entity_name = parse_entity_attr(&tokens);
            }
        }
    }

    let entity_name = entity_name.unwrap_or_else(|| {
        panic!("Missing #[queryable(entity = \"name\")] attribute");
    });

    let expanded = quote! {
        impl #impl_generics version_migrate::Queryable for #name #ty_generics #where_clause {
            const ENTITY_NAME: &'static str = #entity_name;
        }
    };

    TokenStream::from(expanded)
}

fn parse_entity_attr(tokens: &str) -> Option<String> {
    for part in tokens.split(',') {
        let part = part.trim();
        if let Some(val) = parse_attr_value(part, "entity") {
            return Some(val);
        }
    }
    None
}

fn generate_serialize_impl(
    input: &DeriveInput,
    attrs: &VersionedAttributes,
) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let version = &attrs.version;
    let version_key = &attrs.version_key;

    // Extract field information
    let fields = match &input.data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(fields) => &fields.named,
            _ => panic!("auto_tag only supports structs with named fields"),
        },
        _ => panic!("auto_tag only supports structs"),
    };

    let field_count = fields.len() + 1; // +1 for version field
    let field_serializations = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        quote! {
            state.serialize_field(#field_name_str, &self.#field_name)?;
        }
    });

    quote! {
        impl serde::Serialize for #name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(stringify!(#name), #field_count)?;
                state.serialize_field(#version_key, #version)?;
                #(#field_serializations)*
                state.end()
            }
        }
    }
}

fn generate_deserialize_impl(
    input: &DeriveInput,
    attrs: &VersionedAttributes,
) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let version = &attrs.version;
    let version_key = &attrs.version_key;

    // Extract field information
    let fields = match &input.data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(fields) => &fields.named,
            _ => panic!("auto_tag only supports structs with named fields"),
        },
        _ => panic!("auto_tag only supports structs"),
    };

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_name_strs: Vec<_> = field_names.iter().map(|f| f.to_string()).collect();

    let all_field_names = {
        let mut names = vec![version_key.clone()];
        names.extend(field_name_strs.iter().cloned());
        names
    };

    let field_enum_variants = field_names.iter().map(|name| {
        let variant = quote::format_ident!("{}", name.to_string().to_uppercase());
        quote! { #variant }
    });

    let field_match_arms =
        field_names
            .iter()
            .zip(field_name_strs.iter())
            .map(|(name, name_str)| {
                let variant = quote::format_ident!("{}", name.to_string().to_uppercase());
                quote! {
                    #name_str => Ok(Field::#variant)
                }
            });

    let field_visit_arms = field_names.iter().map(|name| {
        let variant = quote::format_ident!("{}", name.to_string().to_uppercase());
        quote! {
            Field::#variant => {
                if #name.is_some() {
                    return Err(serde::de::Error::duplicate_field(stringify!(#name)));
                }
                #name = Some(map.next_value()?);
            }
        }
    });

    let field_unwrap = field_names.iter().map(|name| {
        quote! {
            let #name = #name.ok_or_else(|| serde::de::Error::missing_field(stringify!(#name)))?;
        }
    });

    quote! {
        impl<'de> serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                #[allow(non_camel_case_types)]
                enum Field {
                    Version,
                    #(#field_enum_variants,)*
                }

                impl<'de> serde::Deserialize<'de> for Field {
                    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
                    where
                        D: serde::Deserializer<'de>,
                    {
                        struct FieldVisitor;

                        impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                            type Value = Field;

                            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                                formatter.write_str(&format!("field identifier: {}", &[#(#all_field_names),*].join(", ")))
                            }

                            fn visit_str<E>(self, value: &str) -> Result<Field, E>
                            where
                                E: serde::de::Error,
                            {
                                match value {
                                    #version_key => Ok(Field::Version),
                                    #(#field_match_arms,)*
                                    _ => Err(serde::de::Error::unknown_field(value, &[#(#all_field_names),*])),
                                }
                            }
                        }

                        deserializer.deserialize_identifier(FieldVisitor)
                    }
                }

                struct StructVisitor;

                impl<'de> serde::de::Visitor<'de> for StructVisitor {
                    type Value = #name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str(&format!("struct {}", stringify!(#name)))
                    }

                    fn visit_map<V>(self, mut map: V) -> Result<#name, V::Error>
                    where
                        V: serde::de::MapAccess<'de>,
                    {
                        let mut version: Option<String> = None;
                        #(let mut #field_names = None;)*

                        while let Some(key) = map.next_key()? {
                            match key {
                                Field::Version => {
                                    if version.is_some() {
                                        return Err(serde::de::Error::duplicate_field(#version_key));
                                    }
                                    let v: String = map.next_value()?;
                                    if v != #version {
                                        return Err(serde::de::Error::custom(format!(
                                            "version mismatch: expected {}, found {}",
                                            #version, v
                                        )));
                                    }
                                    version = Some(v);
                                }
                                #(#field_visit_arms)*
                            }
                        }

                        let _version = version.ok_or_else(|| serde::de::Error::missing_field(#version_key))?;
                        #(#field_unwrap)*

                        Ok(#name {
                            #(#field_names,)*
                        })
                    }
                }

                deserializer.deserialize_struct(
                    stringify!(#name),
                    &[#(#all_field_names),*],
                    StructVisitor,
                )
            }
        }
    }
}
