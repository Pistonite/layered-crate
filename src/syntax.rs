use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use proc_macro2::Span;
use quote::{ToTokens, quote};

use crate::util;

pub struct EntryFile {
    /// The file's syntax tree with modifications
    pub syntax: syn::File,

    /// Map from top-level module names to their absolute paths
    pub top_module_to_paths: BTreeMap<String, String>,
}

impl EntryFile {
    pub fn resolve(content: &str, base_path: &Path) -> anyhow::Result<Self> {
        log::debug!("parsing entry file content");

        let mut syntax = syn::parse_file(content)
            .context("failed to parse entrypoint for the library - you have syntax errors.")?;
        let mut resolve_map = BTreeMap::new();
        resolve_items(
            "crate",
            &mut syntax.items,
            base_path,
            true,
            &mut resolve_map,
        )
        .context("failed to resolve items in the entrypoint file")?;

        log::debug!("entry file resolved successfully");
        Ok(Self {
            syntax,
            top_module_to_paths: resolve_map,
        })
    }

    /// Get all top level module names in the entry file
    pub fn all_modules(&self) -> BTreeSet<String> {
        let mut modules = BTreeSet::new();
        for item in &self.syntax.items {
            if let syn::Item::Mod(item_mod) = item {
                modules.insert(item_mod.ident.to_string());
            }
        }
        modules
    }

    /// Produce the library source code as a string.
    pub fn produce_lib(&self) -> String {
        self.syntax.to_token_stream().to_string()
    }

    pub fn produce_test_lib(
        &self,
        test_modules: &[String],
        dependencies: &BTreeSet<String>,
    ) -> anyhow::Result<String> {
        log::debug!(
            "producing test library with test modules: {test_modules:?}, dependencies: {dependencies:?}"
        );

        // keep the original crate attributes
        let file_attrs = &self.syntax.attrs;

        // keep "extern crate"s
        let mut extern_crates = Vec::new();
        for item in &self.syntax.items {
            if let syn::Item::ExternCrate(item_extern) = item {
                extern_crates.push(item_extern);
            }
        }

        let test_module_paths = test_modules
            .iter()
            .map(|test_module| {
                self.top_module_to_paths.get(test_module).context(format!(
                    "test module `{}` not found in entry file",
                    test_module
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let test_module_idents = test_modules
            .iter()
            .map(|test_module| syn::Ident::new(test_module, Span::call_site()))
            .collect::<Vec<_>>();

        let dep_idents = dependencies
            .iter()
            .map(|dep| syn::Ident::new(dep, Span::call_site()))
            .collect::<Vec<_>>();

        let test_file = quote! {
            #(#file_attrs)*
            #(#extern_crates)*

            #(
                #[path = #test_module_paths]
                #[rustfmt::skip]
                pub mod #test_module_idents;
            )*

            #( use ::__layer_test::#dep_idents;)*
        };
        Ok(test_file.to_string())
    }
}

// note: this will not work if there are modules produced by macros
fn resolve_items(
    tag: &str,
    items: &mut Vec<syn::Item>,
    base_path: &Path,
    resolve_path_attrs: bool,
    resolve_map: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    log::debug!(
        "resolving items in {tag}, base path: {}",
        base_path.display()
    );
    for item in items {
        let syn::Item::Mod(item) = item else {
            continue;
        };
        // add rustfmt skip attribute to all modules, so we don't
        // format the original source code
        item.attrs.push(syn::parse_quote! {
            #[rustfmt::skip]
        });
        // modules must be publicly visible so the test package can access them
        if !matches!(item.vis, syn::Visibility::Public(_)) {
            log::trace!("making module `{}` public", item.ident);
            item.vis = syn::parse_quote! { pub };
        }
        // if the item already as a path attribute, resolve it to absolute path from base
        if let Some(path_attr) = item
            .attrs
            .iter_mut()
            .find(|attr| attr.path().is_ident("path"))
        {
            log::trace!("found path attribute for module: {}", item.ident);
            if resolve_path_attrs {
                if let syn::Meta::NameValue(meta) = &mut path_attr.meta {
                    if let syn::Expr::Lit(expr) = &mut meta.value {
                        if let syn::Lit::Str(lit) = &mut expr.lit {
                            let module_path =
                                util::resolve_path(&lit.value(), base_path).context(format!(
                                    "failed to resolve path for module `{}` in {tag}",
                                    item.ident
                                ))?;
                            resolve_map.insert(item.ident.to_string(), module_path.clone());
                            *lit = syn::LitStr::new(&module_path, lit.span());
                        }
                    }
                }
            }
        } else {
            // otherwise, resolve the module path based on the module name
            log::trace!("resolving module: {}", item.ident);
            // is it an inline module? i.e. mod { ... }
            if let Some((_, child_items)) = &mut item.content {
                log::trace!(
                    "module `{}` is inline, processing its items recursively",
                    item.ident
                );
                let child_tag = format!("{tag}::{}", item.ident);
                let child_path = base_path.join(item.ident.to_string());
                resolve_items(&child_tag, child_items, &child_path, false, resolve_map).context(
                    format!(
                        "failed to resolve items in inline module `{}` in {tag}",
                        item.ident
                    ),
                )?;
                continue;
            }
            // add path attribute to non-inline modules
            let path = resolve_module(tag, &item.ident, base_path).context(format!(
                "failed to resolve module `{}` in {tag}",
                item.ident
            ))?;
            log::trace!("adding path attribute to module `{}`: {}", item.ident, path);
            item.attrs.push(syn::parse_quote! {
                #[path = #path]
            });
            resolve_map.insert(item.ident.to_string(), path.clone());
        }
    }

    log::debug!("all items in {tag} resolved successfully");
    Ok(())
}

fn resolve_module(
    tag: &str,
    module_ident: &syn::Ident,
    base_path: &Path,
) -> anyhow::Result<String> {
    log::trace!(
        "resolving module `{}` in {tag}, base path: {}",
        module_ident,
        base_path.display()
    );

    let module_name = module_ident.to_string();

    // <base_path>/module_ident.rs
    if let Ok(module_path) = util::resolve_path(format!("{module_name}.rs"), base_path) {
        log::trace!("found module file at {module_path}");
        return Ok(module_path);
    }

    // <base_path>/module_ident/mod.rs
    let module_path = util::resolve_path(format!("{module_name}/mod.rs"), base_path)
        .context(format!("failed to resolve module `{module_name}` in {tag}"))?;
    log::trace!("found module file at {module_path}");
    Ok(module_path)
}
