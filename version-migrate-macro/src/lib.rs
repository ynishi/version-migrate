use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Meta};

/// Derives the `Versioned` trait for a struct.
///
/// # Attributes
///
/// - `#[versioned(version = "x.y.z")]`: Specifies the semantic version.
///   The version string must be a valid semantic version.
///
/// # Example
///
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
#[proc_macro_derive(Versioned, attributes(versioned))]
pub fn derive_versioned(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Extract the version attribute
    let version = extract_version(&input);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics version_migrate::Versioned for #name #ty_generics #where_clause {
            const VERSION: &'static str = #version;
        }
    };

    TokenStream::from(expanded)
}

fn extract_version(input: &DeriveInput) -> String {
    for attr in &input.attrs {
        if attr.path().is_ident("versioned") {
            if let Meta::List(meta_list) = &attr.meta {
                let nested = meta_list.tokens.to_string();
                // Parse version = "x.y.z"
                if let Some(version_str) = parse_version_attr(&nested) {
                    // Validate semver at compile time
                    if let Err(e) = semver::Version::parse(&version_str) {
                        panic!("Invalid semantic version '{}': {}", version_str, e);
                    }
                    return version_str;
                }
            }
        }
    }
    panic!("Missing #[versioned(version = \"x.y.z\")] attribute");
}

fn parse_version_attr(tokens: &str) -> Option<String> {
    // Simple parser for: version = "x.y.z"
    let tokens = tokens.trim();
    if let Some(rest) = tokens.strip_prefix("version") {
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
