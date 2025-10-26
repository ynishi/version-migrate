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
/// // Serializes to: {"schema_version":"1.0.0","payload":{...}}
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

    let expanded = quote! {
        impl #impl_generics version_migrate::Versioned for #name #ty_generics #where_clause {
            const VERSION: &'static str = #version;
            const VERSION_KEY: &'static str = #version_key;
            const DATA_KEY: &'static str = #data_key;
        }
    };

    TokenStream::from(expanded)
}

struct VersionedAttributes {
    version: String,
    version_key: String,
    data_key: String,
}

fn extract_attributes(input: &DeriveInput) -> VersionedAttributes {
    let mut version = None;
    let mut version_key = String::from("version");
    let mut data_key = String::from("data");

    for attr in &input.attrs {
        if attr.path().is_ident("versioned") {
            if let Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                parse_versioned_attrs(&tokens, &mut version, &mut version_key, &mut data_key);
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
    }
}

fn parse_versioned_attrs(
    tokens: &str,
    version: &mut Option<String>,
    version_key: &mut String,
    data_key: &mut String,
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
