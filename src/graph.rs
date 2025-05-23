use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

#[derive(Default)]
pub struct DepsGraph {
    pub graph: BTreeMap<String, ModuleDecl>,
    pub has_circular_deps: bool,
}

pub struct ModuleDecl {
    /// Order of the module appearance in the source
    pub order: usize,
    /// Whether the mod has `pub`
    pub is_pub: bool,
    /// Ident for the mod
    pub ident: syn::Ident,
    /// Doc attributes for this mod
    pub docs: TokenStream2,
    /// Cfg attributes for this mod
    pub cfg: TokenStream2,
    /// Dependencies
    pub edges: Vec<DepEdge>,
}

impl ModuleDecl {
    pub fn new(
        is_pub: bool,
        ident: syn::Ident,
        docs: TokenStream2,
        cfg: TokenStream2,
        edges: Vec<DepEdge>,
    ) -> Self {
        Self {
            order: 0,
            is_pub,
            ident,
            docs,
            cfg,
            edges,
        }
    }
}

pub struct DepEdge {
    /// The depends_on attribute
    pub attr: syn::Attribute,
    /// The identifier of the dependency
    pub ident: syn::Ident,
    /// The name of the dependency module
    pub name: String,
}

impl DepsGraph {
    pub fn add(&mut self, mut module: ModuleDecl) {
        module.order = self.graph.len();
        self.graph.insert(module.ident.to_string(), module);
    }

    pub fn check(&mut self) -> TokenStream2 {
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
}

fn format_stack(stack: &[String], next: &str) -> String {
    format!("{} -> {}", stack.join(" -> "), next)
}

fn result_to_tokens(r: syn::Result<()>) -> TokenStream2 {
    match r {
        Ok(_) => quote! {},
        Err(err) => err.to_compile_error(),
    }
}
