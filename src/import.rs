use proc_macro::TokenStream;
use proc_macro2::Span as Span2;
use proc_macro2::TokenStream as TokenStream2;
use quote::ToTokens as _;
use quote::quote;
use syn::Token;

static DONT_USE_CRATE: &str = "Don't use `crate_` as it's an implementation detail.";
static PLEASE_GROUP: &str =
    "Please group self (current layer) imports and other imports separately for readability.";
static MOVE_UP_TO_SELF: &str = "Move this import up to be together with other `self` imports";
static MOVE_UP_TO_OTHER: &str = "Move this import up to be together with other `super` imports";
static SELF_CONSISTENT: &str = "Please either import all current layer with or without `self::` prefix. Mixing the two styles can cause confusion.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfStyle {
    Unspecified,
    HasSelf,
    NoSelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfOrder {
    Unspecified,
    SelfFirst,
    OtherFirst,
}

pub fn expand(input: syn::ItemUse) -> syn::Result<TokenStream> {
    let mut error_tokens = TokenStream2::new();

    let vis = input.vis;
    let mut vis_str = vis.to_token_stream().to_string();
    if !vis_str.is_empty() {
        vis_str.push(' ');
    }

    // parse the root use tree
    let (top_ident, mut subtree) = match input.tree {
        syn::UseTree::Glob(t) => {
            let error = "Glob (i.e. `*`) cannot be used at the root of the import for the #[import] attribute";
            let error = syn::Error::new_spanned(&t, error);
            return Err(error);
        }
        syn::UseTree::Group(t) => {
            let error = format!(
                "Group (i.e. `{vis_str}use {{...}}`) cannot be used at the root of the import for the #[import] attribute"
            );
            let error = syn::Error::new_spanned(&t, error);
            return Err(error);
        }
        syn::UseTree::Name(name) => {
            let ident = name.ident.to_string();
            let error = format!(
                "An unnested use statement (i.e. `{vis_str}use {ident}`) is used with #[import]\nThis is likely a mistake. If this is intentional, try `{vis_str}use crate::{ident};` (without the #[import])"
            );
            let error = syn::Error::new_spanned(&name, &error);
            return Err(error);
        }
        syn::UseTree::Rename(name) => {
            let ident = name.ident.to_string();
            let rename = name.rename.to_string();
            let error = format!(
                "An unnested use statement (i.e. `{vis_str}use {ident} as {rename}`) is used with #[import]\nThis is likely a mistake. If this is intentional, try `{vis_str} use crate::{ident} as {rename};` (without the #[import])"
            );
            let error = syn::Error::new_spanned(&name, &error);
            return Err(error);
        }
        syn::UseTree::Path(path) => {
            let top_ident = path.ident;
            let subtree = path.tree;
            (top_ident, subtree)
        }
    };

    let top_ident_string = top_ident.to_string();
    let mut prefix = TokenStream2::new();
    // `crate` cannot be used here
    if top_ident_string == "crate" {
        let error = format!(
            "`crate` cannot be used with the #[import] attribute.\nRemove `crate` and put the name of the current layer, e.g. `{vis_str}use my_layer::{{...}}`;"
        );
        let error = syn::Error::new_spanned(&top_ident, error);
        error_tokens.extend(error.to_compile_error());
    } else {
        prefix.extend(quote! { crate:: });
    }

    // warn user about leading colons
    if input.leading_colon.is_some() {
        let error = syn::Error::new_spanned(
            input.leading_colon,
            "Leading colons are ignored by the #[import] attribute, please remove them",
        );
        error_tokens.extend(error.to_compile_error());
    }

    // we only need to transform one layer of the subtree
    match subtree.as_mut() {
        syn::UseTree::Glob(_) => {
            // use layer::*; -> use crate::layer::*;
        }
        syn::UseTree::Name(name) => {
            // use layer::foo; -> use crate::layer::foo;
            // use layer::super; -> X not allowed
            if name.ident == "super" {
                let error = format!(
                    "Cannot use `super` as the last segment in the #[import] attribute.\nIf you want to rename the dependency module, please rename it instead (e.g. `{vis_str}use {top_ident}::super as deps;`)."
                );
                let error = syn::Error::new_spanned(&name, error);
                return Err(error);
            }
            if name.ident == "crate_" {
                let error = format!(
                    "{DONT_USE_CRATE}\nTry `{vis_str}use {top_ident_string}::super as deps`. If this is intentional, rename it to `crate_` with this syntax."
                );
                let error = syn::Error::new_spanned(&name, error);
                error_tokens.extend(error.to_compile_error());
            }
        }
        syn::UseTree::Rename(rename) => {
            // use layer::foo as bar; -> use crate::layer::foo as bar;
            // use layer::super as renamed; -> use crate::layer::crate_ as renamed;

            let ident_str = rename.ident.to_string();
            let renamed_str = rename.rename.to_string();
            if ident_str == "crate_" {
                let error = format!(
                    "{DONT_USE_CRATE}\nTry `{vis_str}use {top_ident_string}::super as {renamed_str}`. If this is intentional, rename it to `crate_` with this syntax."
                );
                let error = syn::Error::new_spanned(&rename.ident, error);
                error_tokens.extend(error.to_compile_error());
            }
            if ident_str == "super" {
                rename.ident = syn::Ident::new("crate_", Span2::call_site());
            }
        }
        syn::UseTree::Path(path) => {
            // use layer::foo::...; -> use crate::layer::foo::...;
            // use layer::super::... -> use crate::layer::crate_::...;
            // use layer::super_::... -> use crate::layer::crate_::...;
            //
            // the last one is a workaround that rust-analyzer cannot analyze
            // `super` in the middle of the import
            let ident_str = path.ident.to_string();
            if ident_str == "crate_" {
                let error =
                    format!("{DONT_USE_CRATE}\nTry `{vis_str}use {top_ident_string}::super::...`.");
                let error = syn::Error::new_spanned(&path.ident, error);
                error_tokens.extend(error.to_compile_error());
            }
            if ident_str == "super" || ident_str == "super_" {
                path.ident = syn::Ident::new("crate_", Span2::call_site());
            }
        }
        syn::UseTree::Group(group) => {
            // use layer::{...};
            let mut self_style = SelfStyle::Unspecified;
            let mut self_order = SelfOrder::Unspecified;
            let mut has_none_self = false;
            let mut has_self = false;
            let mut notified_order_error = false;
            for tree in &mut group.items {
                mutate_item(
                    tree,
                    &mut error_tokens,
                    &mut self_style,
                    &mut self_order,
                    &mut has_self,
                    &mut has_none_self,
                    &mut notified_order_error,
                );
            }
        }
    };

    let attrs = input.attrs;

    let expanded = quote! {
        #(#attrs)*
        #vis use #prefix #top_ident::#subtree;
        #error_tokens
    };

    Ok(expanded.into())
}

fn mutate_item(
    tree: &mut syn::UseTree,
    error_tokens: &mut TokenStream2,
    self_style: &mut SelfStyle,
    self_order: &mut SelfOrder,
    has_self: &mut bool,
    has_none_self: &mut bool,
    notified_order_error: &mut bool,
) {
    match tree {
        syn::UseTree::Glob(_) => {
            // we don't care about glob (why is glob in a group anyway)
        }
        syn::UseTree::Name(name) => {
            let ident_str = name.ident.to_string();
            if ident_str == "super" {
                let error = syn::Error::new_spanned(
                    &name.ident,
                    "Cannot use `super` inside #[import].\nIf you are trying to import from the parent module, put the `super` outside.",
                );
                error_tokens.extend(error.to_compile_error());
                check_order_current_is_super(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &name.ident,
                );
                *tree = syn::UseTree::Rename(syn::UseRename {
                    ident: syn::Ident::new("crate_", Span2::call_site()),
                    as_token: Token![as](Span2::call_site()),
                    rename: syn::Ident::new("_", Span2::call_site()),
                });
            } else if ident_str == "crate_" {
                let error = format!(
                    "{DONT_USE_CRATE}\nIf you want to import the dependency module, import `super` and rename it with `as` (e.g. `super as deps`"
                );
                let error = syn::Error::new_spanned(&name.ident, error);
                error_tokens.extend(error.to_compile_error());
                *has_none_self = true;
            } else {
                check_order_current_is_self(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &name.ident,
                );
                check_self_style_for_non_path(self_style, &ident_str, error_tokens, &name.ident);
            }
        }
        syn::UseTree::Rename(rename) => {
            let ident_str = rename.ident.to_string();
            let renamed_str = rename.rename.to_string();
            if ident_str == "super" {
                rename.ident = syn::Ident::new("crate_", Span2::call_site());
                check_order_current_is_super(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &rename.ident,
                );
            } else if ident_str == "crate_" {
                let error = format!(
                    "{DONT_USE_CRATE}\nIf this is intentional, use `super` instead (i.e. `super as {renamed_str}`)"
                );
                let error = syn::Error::new_spanned(&rename.ident, error);
                error_tokens.extend(error.to_compile_error());
                *has_none_self = true;
            } else {
                check_order_current_is_self(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &rename.ident,
                );
                check_self_style_for_non_path(self_style, &ident_str, error_tokens, &rename.ident);
            }
        }
        syn::UseTree::Group(group) => {
            for item in &mut group.items {
                mutate_item(
                    item,
                    error_tokens,
                    self_style,
                    self_order,
                    has_self,
                    has_none_self,
                    notified_order_error,
                );
            }
        }
        syn::UseTree::Path(path) => {
            let ident_str = path.ident.to_string();
            if ident_str == "super" {
                check_order_current_is_super(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &path.ident,
                );
                path.ident = syn::Ident::new("crate_", Span2::call_site());
            } else if ident_str == "crate_" {
                let error = syn::Error::new_spanned(&path.ident, DONT_USE_CRATE);
                error_tokens.extend(error.to_compile_error());
                *has_none_self = true;
            } else {
                check_order_current_is_self(
                    notified_order_error,
                    self_order,
                    has_self,
                    has_none_self,
                    error_tokens,
                    &path.ident,
                );
                match *self_style {
                    SelfStyle::Unspecified => {
                        if ident_str != "self" {
                            *self_style = SelfStyle::NoSelf;
                        } else {
                            *self_style = SelfStyle::HasSelf;
                        }
                    }
                    SelfStyle::NoSelf => {
                        if ident_str == "self" {
                            let error = syn::Error::new_spanned(&path.ident, SELF_CONSISTENT);
                            error_tokens.extend(error.to_compile_error());
                        }
                    }
                    SelfStyle::HasSelf => {
                        if ident_str != "self" {
                            let error = syn::Error::new_spanned(&path.ident, SELF_CONSISTENT);
                            error_tokens.extend(error.to_compile_error());
                        }
                    }
                }
            }

            if ident_str == "self" {
                // self can only be at top position, so we unwrap it here
                *tree = *path.tree.clone();
            }
        }
    }
}

fn check_order_current_is_super(
    notified_order_error: &mut bool,
    self_order: &mut SelfOrder,
    has_self: &mut bool,
    has_none_self: &mut bool,
    error_tokens: &mut TokenStream2,
    ident: &syn::Ident,
) {
    if !*notified_order_error {
        if *self_order == SelfOrder::OtherFirst {
            if *has_self {
                let error = format!("{PLEASE_GROUP}\n{MOVE_UP_TO_OTHER}");
                let error = syn::Error::new_spanned(ident, error);
                error_tokens.extend(error.to_compile_error());
                *notified_order_error = true;
            }
        } else if *self_order == SelfOrder::Unspecified {
            *self_order = SelfOrder::OtherFirst;
        }
        *has_none_self = true;
    }
}

fn check_order_current_is_self(
    notified_order_error: &mut bool,
    self_order: &mut SelfOrder,
    has_self: &mut bool,
    has_none_self: &mut bool,
    error_tokens: &mut TokenStream2,
    ident: &syn::Ident,
) {
    if !*notified_order_error {
        if *self_order == SelfOrder::SelfFirst {
            if *has_none_self {
                let error = format!("{PLEASE_GROUP}\n{MOVE_UP_TO_SELF}");
                let error = syn::Error::new_spanned(ident, error);
                error_tokens.extend(error.to_compile_error());
                *notified_order_error = true;
            }
        } else if *self_order == SelfOrder::Unspecified {
            *self_order = SelfOrder::SelfFirst;
        }
        *has_self = true;
    }
}

fn check_self_style_for_non_path(
    self_style: &mut SelfStyle,
    ident_str: &str,
    error_tokens: &mut TokenStream2,
    ident: &syn::Ident,
) {
    if ident_str != "self" {
        if *self_style == SelfStyle::Unspecified {
            *self_style = SelfStyle::NoSelf;
        } else if *self_style != SelfStyle::NoSelf {
            let error = syn::Error::new_spanned(ident, SELF_CONSISTENT);
            error_tokens.extend(error.to_compile_error());
        }
    }
}
