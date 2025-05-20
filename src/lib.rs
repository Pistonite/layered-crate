#![doc = include_str!("../README.md")]

use std::collections::{BTreeMap, BTreeSet};

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

    // collect the dependency attributes
    let mut graph = DepsGraph::default();
    let mut transformed_src_content = TokenStream2::new();

    for item in content {
        // Limitation - non-inline modules in proc-macro is unstable
        // so as a workaround we use "extern crate" right now
        let syn::Item::ExternCrate(mut item) = item else {
            transformed_src_content.extend(quote! { #item });
            continue;
        };
        let vis = item.vis.clone();
        let ident = item.ident;
        let mut edges = Vec::with_capacity(item.attrs.len());
        let dependencies = {
            let mut d = Vec::with_capacity(item.attrs.len());
            std::mem::swap(&mut item.attrs, &mut d);
            d
        };
        let mut docs = TokenStream2::new();
        for dep in dependencies {
            if !dep.path().is_ident("depends_on") {
                if dep.path().is_ident("doc") {
                    docs.extend(quote! { #dep });
                }
                transformed_src_content.extend(quote! { #dep });
                continue;
            }
            let ident = dep.meta.require_list()?.parse_args::<syn::Ident>()?;
            let ident_str = ident.to_string();
            edges.push(DepEdge {
                attr: dep,
                ident,
                name: ident_str,
            });
        }
        transformed_src_content.extend(quote! {
            pub mod #ident;
        });
        graph.add(
            matches!(vis, syn::Visibility::Public(_)),
            ident,
            docs,
            edges,
        );
    }

    // check - this produces the errors as tokens instead of
    // result. we still emit the expanded output even if check fails,
    // so that we don't cause massive compile failures
    let check_result_tokens = graph.check();

    // create a new ident, so unused warnings don't show up
    // on the entire macro input
    let src_ident = syn::Ident::new(&input.ident.to_string(), Span2::call_site());
    let mod_tokens = graph.generate_impl(&src_ident);

    let expanded = quote! {
        #[doc(hidden)]
        pub(crate) mod #src_ident {
            #transformed_src_content
        }
        #mod_tokens
        #check_result_tokens
    };

    Ok(expanded.into())
}

#[derive(Default)]
struct DepsGraph {
    graph: BTreeMap<String, ModuleDecl>,
    has_circular_deps: bool,
}

struct ModuleDecl {
    /// Order of the module appearance in the source
    order: usize,
    /// The visibility of the mod
    is_pub: bool,
    /// Ident for the mod
    ident: syn::Ident,
    /// Doc attributes for this mod
    docs: TokenStream2,
    /// Dependencies
    edges: Vec<DepEdge>,
}

struct DepEdge {
    /// The depends_on attribute
    attr: syn::Attribute,
    /// The identifier of the dependency
    ident: syn::Ident,
    /// The name of the dependency module
    name: String,
}

impl DepsGraph {
    fn add(&mut self, is_pub: bool, ident: syn::Ident, docs: TokenStream2, edges: Vec<DepEdge>) {
        let order = self.graph.len();
        self.graph.insert(
            ident.to_string(),
            ModuleDecl {
                order,
                is_pub,
                ident,
                docs,
                edges,
            },
        );
    }

    fn check(&mut self) -> TokenStream2 {
        let mut tokens = TokenStream2::new();
        self.check_exists(&mut tokens);
        let circular_deps_result = self.check_circular_deps();
        if circular_deps_result.is_ok() {
            // only check order if no circular deps,
            // because it's impossible to have the right order
            // if there are circular deps
            self.check_attr_order(&mut tokens);
        } else {
            self.has_circular_deps = true;
        }

        tokens.extend(result_to_tokens(circular_deps_result));
        tokens
    }

    // this is mut because we want to remove the dependencies
    // that don't exist, to prevent double errors
    fn check_exists(&mut self, errors: &mut TokenStream2) {
        let keys = self.graph.keys().cloned().collect::<BTreeSet<_>>();
        for entry in self.graph.values_mut() {
            let edges = {
                let mut edges = Vec::with_capacity(entry.edges.len());
                std::mem::swap(&mut entry.edges, &mut edges);
                edges
            };
            for edge in edges {
                if keys.contains(&edge.name) {
                    entry.edges.push(edge);
                    continue;
                }
                let e = syn::Error::new_spanned(
                    &edge.attr,
                    format!("cannot find dependency: {}", edge.name),
                )
                .to_compile_error();
                errors.extend(e);
                // don't add the bad dependency to the graph
            }
        }
    }

    fn check_circular_deps(&self) -> syn::Result<()> {
        let mut checked = BTreeSet::new();
        for (name, entry) in self.graph.iter() {
            let mut stack = vec![name.clone()];
            self.check_circular_deps_recur(name, &entry.ident, &mut stack, &mut checked)?;
        }
        Ok(())
    }

    fn check_circular_deps_recur(
        &self,
        name: &str,
        ident: &syn::Ident,
        stack: &mut Vec<String>, // stack top contains name
        checked: &mut BTreeSet<String>,
    ) -> syn::Result<()> {
        // already searched this node
        if !checked.insert(name.to_owned()) {
            return Ok(());
        }
        let Some(entry) = self.graph.get(name) else {
            return Err(syn::Error::new_spanned(
                ident,
                format!("cannot find dependency: {}", name),
            ));
        };

        for edge in &entry.edges {
            if stack.contains(&edge.name) {
                let graph = format_stack(stack, &edge.name);
                return Err(syn::Error::new_spanned(
                    &edge.attr,
                    format!("circular dependency detected: {}", graph),
                ));
            }
            stack.push(edge.name.clone());
            self.check_circular_deps_recur(&edge.name, &edge.ident, stack, checked)?;
            stack.pop().expect("underflowed dep stack, this is a bug");
        }

        Ok(())
    }

    /// Make sure the #[depends_on] attributes are in the same order
    /// as the module declaration, to make it look nice
    fn check_attr_order(&self, errors: &mut TokenStream2) {
        let mut orders = Vec::<(usize, String)>::new();
        for (name, entry) in &self.graph {
            orders.clear();
            let mut current_dep_order = 0;
            for dep in &entry.edges {
                let Some(m) = self.graph.get(&dep.name) else {
                    continue;
                };
                if m.order < entry.order {
                    let e = syn::Error::new_spanned(
                        &entry.ident,
                        format!(
                            "module `{}` should be declared before its dependency `{}` to ensure top-down readability",
                            name, dep.name
                        ),
                    ).to_compile_error();
                    errors.extend(e);
                }
                if m.order < current_dep_order {
                    // find the right place
                    let mut found = false;
                    for (order, n) in &orders {
                        if m.order < *order {
                            let e = syn::Error::new_spanned(
                                &dep.ident,
                                format!(
                                    "#[depends_on({})] should be before #[depends_on({})] to ensure consistent order of modules",
                                    dep.name, n
                                ),
                            ).to_compile_error();
                            errors.extend(e);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        // just in case the order is messed really bad and we can't find it for
                        // some reason, we still want to emit an error
                        let e = syn::Error::new_spanned(
                            &dep.ident,
                            format!(
                                "#[depends_on({})] should be placed in the same order the modules are declared",
                                dep.name
                            ),
                        ).to_compile_error();
                        errors.extend(e);
                    }
                } else {
                    orders.push((m.order, m.ident.to_string()));
                }
                current_dep_order = m.order;
            }
        }
    }

    fn generate_impl(&self, src_mod: &syn::Ident) -> TokenStream2 {
        let mut mod_tokens = TokenStream2::new();
        for entry in self.graph.values() {
            mod_tokens.extend(entry.generate_mod_impl(src_mod, self.has_circular_deps));
        }
        mod_tokens
    }
}

fn format_stack(stack: &[String], next: &str) -> String {
    format!("{} -> {}", stack.join(" -> "), next)
}

impl ModuleDecl {
    fn generate_mod_impl(&self, src_mod: &syn::Ident, has_circular_deps: bool) -> TokenStream2 {
        let vis = if self.is_pub {
            quote! { pub }
        } else {
            quote! { pub(crate) }
        };
        let doc = &self.docs;
        let deps_ident = &self.ident;

        if self.edges.is_empty() {
            return quote_spanned! {
                self.ident.span() => #[doc(inline)] #vis use #src_mod::#deps_ident;
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
        for edge in &self.edges {
            let dep_ident = &edge.ident;
            dep_tokens.extend(quote_spanned! {
                dep_ident.span() =>
                    pub use crate::#src_mod::#dep_ident;
            });
        }

        quote_spanned! {
            self.ident.span() =>
                #doc
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

fn result_to_tokens(r: syn::Result<()>) -> TokenStream2 {
    match r {
        Ok(_) => quote! {},
        Err(err) => err.to_compile_error(),
    }
}
