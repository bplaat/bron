/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [rust-embed-impl](https://crates.io/crates/rust-embed-impl) crate

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, Lit, Meta, parse_macro_input};

/// [RustEmbed] derive
#[proc_macro_derive(RustEmbed, attributes(folder))]
pub fn embed_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input).unwrap_or_else(|error| error.into_compile_error().into())
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "RustEmbed does not support generic types",
        ));
    }
    if !matches!(input.data, syn::Data::Struct(ref data) if matches!(data.fields, syn::Fields::Unit))
    {
        return Err(syn::Error::new_spanned(
            name,
            "RustEmbed can only be derived for unit structs",
        ));
    }

    // Extract the #[folder = "..."] attribute
    let folder_path = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("folder"))
        .and_then(|attr| match &attr.meta {
            Meta::NameValue(meta) => match &meta.value {
                Expr::Lit(expr) => match &expr.lit {
                    Lit::Str(lit_str) => Some(lit_str.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new_spanned(
                name,
                "#[derive(RustEmbed)] must contain one attribute like this #[folder = \"examples/public/\"]",
            )
        })?;

    // Replace $NAME and ${NAME} in folder_path with their values from the environment
    #[cfg(feature = "interpolate-folder-path")]
    let folder_path =
        interpolate(&folder_path).map_err(|message| syn::Error::new_spanned(name, message))?;

    // Base relative paths on the Cargo.toml location
    let folder_path =
        Path::new(&env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"))
            .join(folder_path);

    // Recursively index all files from the folder
    fn visit_dir(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                visit_dir(&path, files)?;
            } else if path.is_file() {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit_dir(&folder_path, &mut files).map_err(|error| {
        syn::Error::new_spanned(
            name,
            format!(
                "#[derive(RustEmbed)] folder '{}' can't be read: {error}",
                folder_path.display()
            ),
        )
    })?;
    files.sort();

    let sources: Vec<_> = files
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let file_paths: Vec<_> = files
        .iter()
        .map(|path| {
            path.strip_prefix(&folder_path)
                .expect("Should be relative path")
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect();

    Ok(quote! {
        const _: () = {
            extern crate alloc;

            impl #name {
                /// Get an embedded file and its metadata.
                pub fn get(file_path: &str) -> Option<rust_embed::EmbeddedFile> {
                    match file_path.replace('\\', "/").as_str() {
                        #(#file_paths => Some(rust_embed::EmbeddedFile {
                            data: alloc::borrow::Cow::Borrowed(include_bytes!(#sources)),
                            metadata: rust_embed::Metadata::__rust_embed_new([0; 32], None, None),
                        }),)*
                        _ => None,
                    }
                }

                /// Iterates over the file paths in the folder.
                pub fn iter() -> impl Iterator<Item = alloc::borrow::Cow<'static, str>> + 'static {
                    [#(alloc::borrow::Cow::Borrowed(#file_paths),)*].into_iter()
                }
            }

            impl rust_embed::RustEmbed for #name {
                fn get(file_path: &str) -> Option<rust_embed::EmbeddedFile> {
                    #name::get(file_path)
                }

                fn iter() -> impl Iterator<Item = alloc::borrow::Cow<'static, str>> + 'static {
                    #name::iter()
                }
            }
        };
    }
    .into())
}

/// Expand `$NAME` and `${NAME}` environment variables like `shellexpand::env`
#[cfg(feature = "interpolate-folder-path")]
fn interpolate(input: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut rest = input;
    while let Some(index) = rest.find('$') {
        output.push_str(&rest[..index]);
        rest = &rest[index + 1..];
        let (key, remaining) = if let Some(braced) = rest.strip_prefix('{')
            && let Some(end) = braced.find('}')
        {
            (&braced[..end], &braced[end + 1..])
        } else {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            (&rest[..end], &rest[end..])
        };
        if key.is_empty() {
            output.push('$');
            continue;
        }
        let value =
            env::var(key).map_err(|error| format!("error looking key '{key}' up: {error}"))?;
        output.push_str(&value);
        rest = remaining;
    }
    output.push_str(rest);
    Ok(output)
}

// MARK: Tests
#[cfg(all(test, feature = "interpolate-folder-path"))]
mod test {
    use super::*;

    #[test]
    fn test_interpolate() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pkg_name = env!("CARGO_PKG_NAME");
        assert_eq!(
            interpolate("$CARGO_MANIFEST_DIR/a").as_deref(),
            Ok(format!("{manifest_dir}/a").as_str())
        );
        assert_eq!(
            interpolate("${CARGO_PKG_NAME}_x/$CARGO_PKG_NAME").as_deref(),
            Ok(format!("{pkg_name}_x/{pkg_name}").as_str())
        );
        assert_eq!(interpolate("a$/b$").as_deref(), Ok("a$/b$"));
        assert_eq!(interpolate("${unclosed").as_deref(), Ok("${unclosed"));
        assert!(interpolate("$RUST_EMBED_UNDEFINED_VAR").is_err());
    }
}
