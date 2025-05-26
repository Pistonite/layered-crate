#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod graph;
mod import;
mod layers;

/// See [`crate documentation`](crate)
#[proc_macro_attribute]
pub fn layers(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::ItemMod);
    match layers::expand(input) {
        Ok(expanded) => expanded,
        Err(err) => err.to_compile_error().into(),
    }
}

/// The import attribute will transform a `use` item to use
/// `self` and `crate` to refer to the current module and its dependencies
/// as defined by the [`#[layers]`](crate) attribute.
///
/// # Example
/// Assuming we organize our imports in the following order:
/// - std
/// - external dependencies
/// - dependencies within our crate
///
/// ```rust,ignore
/// use std::fs::File;
/// // .. other std dependencies
///
/// use serde_json::Value;
/// // .. other external dependencies
///
/// #[layered_crate::import]
/// use my_layer::{
/// //  ^ `my_layer` refers to the current layer as defined in lib.rs
///     super::my_dep::MyType,
///  // ^ `super` in this context is used to refer to this layer's dependencies
///  //   the above is equivalent to `use crate::my_dep::MyType`,
///  //   but only works if `my_dep` is declared as a dependency
///     super::{
///         my_other_dep::MyOtherType,
///         my_more_dep::MyMoreType,
///     },
///  // ^ other forms work as well
///     MyDepInThisLayer, // ... import rest from the current layer
///
///     self::MyTypeInThisLayer,
///  // ^ you can also use `self` to refer to the current module
///  //   if you prefer
///
/// };
///
/// // any item outside of the macro isn't transformed, and you can
/// // use this to bypass the layer restrictions
/// use crate::unchecked;
///
/// // if you actually need to import from `super`, also put it
/// // outside
/// use super::*;
/// ```
///
/// # Workaround for single `super` import
///
/// Currently, rust-analyzer does not process "malformed" imports, including
/// `super` in the middle of the import:
/// ```rust,ignore
/// #[layered_crate::import]
/// use my_layer::super::my_dep::MyType;
/// ```
///
/// While `layered-crate` transforms this correctly, rust-analyzer will not
/// provide LSP functionality for this import (e.g. goto definition).
///
/// If this is an issue for you, `super_` is allowed as a workaround:
/// ```rust,ignore
/// #[layered_crate::import]
/// use my_layer::super_::my_dep::MyType;
/// ```
///
/// # Other checks
/// - It will make sure all of your `super` and `self` (imports from current layer) are grouped
///   together for readability
///   - If you actually want to group the imports in another way, consider
///     using multiple `import` attributes
/// - It will make sure imports from the current layer either all use `self`
///   or not use `self` for consistency, the `self` import by itself is always allowed
#[proc_macro_attribute]
pub fn import(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::ItemUse);
    match import::expand(input) {
        Ok(expanded) => expanded,
        Err(err) => err.to_compile_error().into(),
    }
}
