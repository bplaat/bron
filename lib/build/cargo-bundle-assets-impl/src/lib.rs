/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Derive implementation for cargo-bundle-assets.

use std::path::{Path, PathBuf};
use std::{env, fs};

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, Lit, Meta, parse_macro_input};

/// Declare an embedded or bundled asset collection with a `#[folder = "..."]` attribute.
#[proc_macro_derive(Assets, attributes(folder))]
pub fn derive_assets(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fail = |message: &str| syn::Error::new_spanned(input, message);
    if !input.generics.params.is_empty()
        || !matches!(&input.data, syn::Data::Struct(data) if matches!(data.fields, syn::Fields::Unit))
    {
        return Err(fail("Assets requires a unit struct without generics"));
    }
    let folder = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("folder"))
        .and_then(|attr| match &attr.meta {
            Meta::NameValue(meta) => match &meta.value {
                Expr::Lit(expr) => match &expr.lit {
                    Lit::Str(value) => Some(value.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .ok_or_else(|| fail("Expected #[folder = \"assets\"]"))?;
    let (resolved, variables) =
        interpolate_folder(&folder, |key| env::var(key).ok()).map_err(|message| fail(&message))?;
    let root =
        Path::new(&env::var("CARGO_MANIFEST_DIR").map_err(|_| fail("Missing package directory"))?)
            .join(resolved);
    if fs::symlink_metadata(&root)
        .map_err(|error| fail(&format!("Invalid asset folder: {error}")))?
        .file_type()
        .is_symlink()
    {
        return Err(fail("Asset folder symlinks are unsupported"));
    }
    let root = root
        .canonicalize()
        .map_err(|error| fail(&format!("Invalid asset folder: {error}")))?;
    let root_utf8 = root
        .to_str()
        .ok_or_else(|| fail("Asset root must be UTF-8"))?;
    let mut paths = Vec::new();
    visit(&root, &mut paths).map_err(|error| fail(&format!("Cannot index assets: {error}")))?;
    paths.sort();
    let names: Vec<_> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .expect("asset is in its root")
                .to_str()
                .map(|name| name.replace('\\', "/"))
                .ok_or_else(|| fail("Asset names must be UTF-8"))
        })
        .collect::<syn::Result<_>>()?;
    let sources: Vec<_> = paths
        .iter()
        .map(|path| path.to_str().expect("UTF-8 asset path"))
        .collect();
    let name = &input.ident;
    let identity = format!(
        "{}:{}:{folder}:{name}",
        env::var("CARGO_PKG_NAME").unwrap_or_default(),
        env::var("CARGO_PKG_VERSION").unwrap_or_default()
    );
    let hash = identity
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let collection = format!("{hash:016x}");
    let external = env::var_os("CARGO_BUNDLE").is_some();
    let (metadata, read) = if external {
        // v1 payload: postcard tuple (collection ID, UTF-8 root, relative file names).
        let data = postcard::to_allocvec(&(&collection, root_utf8, &names))
            .expect("Asset metadata is serializable");
        let mut record = b"\0cargo-bundle-assets:v1\0".to_vec();
        record.extend_from_slice(&(data.len() as u32).to_le_bytes());
        record.extend_from_slice(&data);
        let length = record.len();
        (
            quote! {
                #[used]
                #[allow(unsafe_code)]
                #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__bundle_assets"))]
                #[cfg_attr(windows, unsafe(link_section = ".bassets"))]
                #[cfg_attr(not(any(target_vendor = "apple", windows)), unsafe(link_section = ".bundle_assets"))]
                static METADATA: [u8; #length] = [#(#record),*];
            },
            quote! {
                match path {
                    #(#names => ::cargo_bundle_assets::read(#collection, path, &METADATA).map(Some),)*
                    _ => Ok(None),
                }
            },
        )
    } else {
        (
            quote! {},
            quote! {
                Ok(match path {
                    #(#names => Some(::cargo_bundle_assets::EmbeddedFile {
                        data: ::cargo_bundle_assets::__private::Cow::Borrowed(include_bytes!(#sources)),
                        metadata: ::cargo_bundle_assets::__private::Metadata::__rust_embed_new([0; 32], None, None),
                    }),)*
                    _ => None,
                })
            },
        )
    };
    let external_metadata_reference = external.then(|| quote! { std::hint::black_box(&METADATA); });
    let count = names.len();
    Ok(quote! {
        const _: () = {
            // Track the mode and interpolated environment variables in Cargo's dep-info.
            const _: Option<&str> = option_env!("CARGO_BUNDLE");
            const _: Option<&str> = option_env!("CARGO_BUNDLE_ASSETS_REVISION");
            #(const _: Option<&str> = option_env!(#variables);)*
            // Track edits without retaining payloads in external builds.
            #(const _: &[u8] = include_bytes!(#sources);)*
            #metadata
            impl #name {
                fn get(path: &str) -> Option<::cargo_bundle_assets::EmbeddedFile> {
                    Self::try_get(path).ok().flatten()
                }
                fn try_get(path: &str) -> ::cargo_bundle_assets::__private::io::Result<Option<::cargo_bundle_assets::EmbeddedFile>> {
                    #read
                }
                fn iter() -> impl Iterator<Item = ::cargo_bundle_assets::__private::Cow<'static, str>> + 'static {
                    // iter-only collections must also be visible to the bundler.
                    #name::__retain_metadata();
                    {
                        let names: [&'static str; #count] = [#(#names),*];
                        names.into_iter().map(::cargo_bundle_assets::__private::Cow::Borrowed)
                    }
                }
                fn __retain_metadata() {
                    #external_metadata_reference
                }
            }
            impl ::cargo_bundle_assets::RustEmbed for #name {
                fn get(path: &str) -> Option<::cargo_bundle_assets::EmbeddedFile> {
                    Self::get(path)
                }
                fn iter() -> impl Iterator<Item = ::cargo_bundle_assets::__private::Cow<'static, str>> + 'static {
                    Self::iter()
                }
            }
        };
    })
}

fn interpolate_folder(
    folder: &str,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<(String, Vec<String>), String> {
    let mut resolved = String::new();
    let mut variables = Vec::new();
    let mut remaining = folder;
    while let Some(start) = remaining.find('$') {
        resolved.push_str(&remaining[..start]);
        remaining = &remaining[start + 1..];
        let length = remaining
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if length == 0 {
            resolved.push('$');
            continue;
        }
        let key = &remaining[..length];
        let value = lookup(key).ok_or_else(|| format!("Missing asset folder variable: {key}"))?;
        resolved.push_str(&value);
        variables.push(key.to_owned());
        remaining = &remaining[length..];
    }
    resolved.push_str(remaining);
    Ok((resolved, variables))
}

fn visit(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(std::io::Error::other(format!(
                "Asset symlinks are unsupported: {}",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            visit(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_complete_variable_names_without_expanding_values() {
        let (folder, variables) =
            interpolate_folder("$ASSET_ROOT/$ASSET/texture", |key| match key {
                "ASSET" => Some("$LITERAL".to_owned()),
                "ASSET_ROOT" => Some("generated".to_owned()),
                _ => None,
            })
            .expect("Valid folder variables");
        assert_eq!(folder, "generated/$LITERAL/texture");
        assert_eq!(variables, ["ASSET_ROOT", "ASSET"]);
    }

    #[test]
    fn rejects_missing_variables_and_preserves_literal_dollars() {
        assert!(interpolate_folder("$MISSING/assets", |_| None).is_err());
        assert_eq!(
            interpolate_folder("assets/$/texture$", |_| None).expect("Literal dollars"),
            ("assets/$/texture$".to_owned(), Vec::new())
        );
    }
}
