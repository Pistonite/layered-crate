#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use proc_macro2::Span as Span2;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use quote::quote_spanned;
use syn::parse_macro_input;

/// See [`crate documentation`](crate)
#[proc_macro_attribute]
pub fn layers(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::ItemMod);
    match layered_crate_expand(input) {
        Ok(expanded) => expanded,
        Err(err) => err.to_compile_error().into(),
    }
}

fn layered_crate_expand(input: syn::ItemMod) -> syn::Result<TokenStream> {
    let (_, content) = match input.content {
        None => {
            // nothing in the mod
            return Ok(quote! { #input }.into());
        }
        Some(content) => content,
    };

    let mut before_tokens = TokenStream2::new();
    let mut has_doc_hidden = false;

    // keep the original attributes on the whole import
    // and ensure #[doc(hidden)] is present
    for attr in input.attrs {
        if attr.path().is_ident("doc") {
            if let Ok(x) = attr
                .meta
                .require_list()
                .and_then(|m| m.parse_args::<syn::Ident>())
            {
                if x == "hidden" {
                    has_doc_hidden = true;
                }
            }
        }
        before_tokens.extend(quote! { #attr });
    }
    if !has_doc_hidden {
        before_tokens.extend(quote! { #[doc(hidden)] });
    }

    // collect the dependency attributes
    let mut graph = graph::DepsGraph::default();
    let mut transformed_src_content = TokenStream2::new();
    let mut error_tokens = TokenStream2::new();

    for item in content {
        // attrs  vis              ident  extra_tokens
        // #[...] pub mod          xxx    {...}
        // #[...] pub mod          yyy    ;
        // #[...] pub extern crate zzz    ;
        let (attrs, vis, ident, extra_tokens) = match item {
            // Limitation - non-inline modules in proc-macro is unstable
            // so as a workaround we use "extern crate" as a placeholder
            // for non-inline modules
            syn::Item::ExternCrate(item) => {
                let mut extra_tokens = TokenStream2::new();
                if let Some(rename) = item.rename {
                    let e = syn::Error::new_spanned(
                        &rename.1,
                        "rename syntax (as ...) is not supported when using #[layers]",
                    );
                    extra_tokens.extend(e.to_compile_error());
                }
                let semi = item.semi_token;
                extra_tokens.extend(quote! { #semi });

                (item.attrs, item.vis, item.ident, extra_tokens)
            }
            syn::Item::Mod(item) => {
                let mut extra_tokens = TokenStream2::new();
                if let Some((_, content)) = item.content {
                    extra_tokens.extend(quote! { { #(#content)* } });
                }
                if let Some(semi) = item.semi {
                    extra_tokens.extend(quote! { #semi });
                }

                (item.attrs, item.vis, item.ident, extra_tokens)
            }
            _ => {
                // other items in the mod, we just leave them along
                transformed_src_content.extend(quote! { #item });
                continue;
            }
        };

        // Extract the attributes
        let mut edges = Vec::with_capacity(attrs.len());
        let mut docs = TokenStream2::new();
        let mut cfg = TokenStream2::new();
        for attr in attrs {
            if attr.path().is_ident("depends_on") {
                let ident = match attr
                    .meta
                    .require_list()
                    .and_then(|m| m.parse_args::<syn::Ident>())
                {
                    Ok(x) => x,
                    Err(e) => {
                        error_tokens.extend(e.to_compile_error());
                        continue;
                    }
                };
                edges.push(graph::DepEdge {
                    name: ident.to_string(),
                    attr,
                    ident,
                });
                continue;
            }

            if attr.path().is_ident("doc") {
                docs.extend(quote! { #attr });
            }
            if attr.path().is_ident("cfg") {
                cfg.extend(quote! { #attr });
            }

            // keep attributes unrelated to us
            transformed_src_content.extend(quote! { #attr });
        }

        transformed_src_content.extend(quote! {
            pub mod #ident #extra_tokens
        });
        graph.add(graph::ModuleDecl::new(
            matches!(vis, syn::Visibility::Public(_)),
            ident,
            docs,
            cfg,
            edges,
        ));
    }

    // check - this produces the errors as tokens instead of
    // result. we still emit the expanded output even if check fails,
    // so that we don't cause massive compile failures
    error_tokens.extend(graph.check());

    // create a new ident, so unused warnings don't show up
    // on the entire macro input
    let src_ident = syn::Ident::new(&input.ident.to_string(), Span2::call_site());
    let mod_tokens = graph.generate_impl(&src_ident);

    let expanded = quote! {
        #before_tokens
        pub(crate) mod #src_ident {
            #transformed_src_content
        }
        #mod_tokens
        #error_tokens
    };

    Ok(expanded.into())
}

mod graph;

impl graph::DepsGraph {
    fn generate_impl(&self, src_mod: &syn::Ident) -> TokenStream2 {
        let mut mod_tokens = TokenStream2::new();
        for entry in self.graph.values() {
            mod_tokens.extend(self.generate_mod_impl(entry, src_mod, self.has_circular_deps));
        }
        mod_tokens
    }
    fn generate_mod_impl(
        &self,
        module: &graph::ModuleDecl,
        src_mod: &syn::Ident,
        has_circular_deps: bool,
    ) -> TokenStream2 {
        let vis = if module.is_pub {
            quote! { pub }
        } else {
            quote! { pub(crate) }
        };
        let doc = &module.docs;
        let cfg = &module.cfg;
        let deps_ident = &module.ident;

        if module.edges.is_empty() {
            return quote_spanned! {
                module.ident.span() =>
                    #cfg
                    #[doc(inline)]
                    #vis use #src_mod::#deps_ident;
            };
        }

        let mut suppress_lints = TokenStream2::new();
        if has_circular_deps {
            // allow unused imports in circular deps, because
            // the warning will make it hard to see what actually is the cause
            suppress_lints.extend(quote! {
                #[allow(unused_imports)]
            });
        }

        let mut dep_tokens = TokenStream2::new();
        for edge in &module.edges {
            let dep_module = self.graph.get(&edge.name).unwrap();
            let dep_cfg = &dep_module.cfg;
            let dep_ident = &edge.ident;
            dep_tokens.extend(quote_spanned! {
                dep_ident.span() =>
                    #dep_cfg
                    pub use crate::#src_mod::#dep_ident;
            });
        }

        quote_spanned! {
            module.ident.span() =>
                #doc
                #cfg
                #vis mod #deps_ident {
                    #[doc(inline)]
                    pub use crate::#src_mod::#deps_ident::*;
                    #[doc(hidden)]
                    #suppress_lints
                    pub(crate) mod crate_ {
                        #dep_tokens
                    }
                }
        }
    }
}
