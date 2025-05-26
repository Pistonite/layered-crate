# layered-crate

Enforce dependencies amongst internal modules in a crate

```rust,ignore
use layered_crate::layers;

#[layers]
mod src {
    /// Public APIs
    #[depends_on(details)]
    #[depends_on(utils)]
    extern crate api;

    #[depends_on(utils)]
    pub extern crate details;

    /// Internal utils
    extern crate utils;
}

pub use api::*;
```

## The Problem
In a large Rust project, it's common to have modules or subsystems in a crate
that depends on other parts of the crate, forming an internal dependency
graph amongst modules. Since Rust allows you to import anything anywhere in the same
crate, the dependency can become a mess over long time.

Some projects solve this using a workspace with multiple crates and use crate-level
dependency. That's what happens when you see a bunch of `project-*` crates when searching
for something on crates.io. There are several upsides and downsides to this. Just to list a few:

- Upsides:
  - Uses the standard `Cargo.toml`, which is more stable
  - Might be better to split large code base, so someone doesn't have to download everything
  - Might be better for incremental build but I am clueless if this is true

- Downsides:
  - Need to publish 50 instead of 1 crate
  - Need to have a more complicated `Cargo.toml` setup
  - Might be worse for optimization since one of the factor for inlining is if
    the inlining is across a crate boundary. However I have no clue what degree of effect this has

This crate takes a different approach. It uses a proc-macro and some custom
syntax to check and enforce the dependencies at compile time, all within the same crate.
This has a few advantages:
- Uses the same `Cargo.toml` as before
- The effect is invisible to people using the crate.

... and a few disadvantages:
- Uses custom syntax
- The enforcement is not strict, since that's outside of the power of proc-macros
- rust-analyzer will not use this library's syntax when adding imports

All said, you should do the research needed to figure out if this crate is the right approach for
your use case.

## Usage

Say, you crate has this structure:
- `api` - high level APIs that you want user to call
- `sub_system_1` and `sub_system_2` - some sub-systems of the crate, that maybe some advanced user needs to access directly
- `util` - Shared utility stuff that the rest of the code calls, that you don't want to export

Your `src/lib.rs` might look like:
```rust,ignore
// The example doc comments are only there as example, in reality
// these are usually much longer and detailed

/// My Public APIs
mod api;
#[doc(inline)]
pub use api::*;

/// Sub-system 1 if you need
pub mod sub_system_1;
/// Sub-system 2 if you need
pub mod sub_system_2;

/// Internal utils
mod utils;
```

This is all fine and good, but nothing is stopping some file in `sub_system_2`
to `use sub_system_1::xxx;`, even if that's not how you architected it.
Let's fix that with `layered-crate`!

First, you need to move `lib.rs` outside of `src` - this is so that we can
make the module shims without changing too much of the directory structure
```toml
# Cargo.toml
[lib]
path = "lib.rs"
```

Now, change `lib.rs`:
```rust,ignore
use layered_crate::layers;

#[layers]
mod src {
    /// My Public APIs
    #[depends_on(sub_system_1)]
    #[depends_on(sub_system_2)]
    #[depends_on(utils)]
    extern crate api;

    /// Sub-system 1 if you need
    #[depends_on(utils)]
    pub extern crate sub_system_1;

    /// Sub-system 2 if you need
    #[depends_on(utils)]
    pub extern crate sub_system_2;

    /// Internal utils
    extern crate utils;
}

#[doc(inline)]
pub use api::*;
```

Note that:
- `extern crate` is used because non-inline modules in proc-macro inputs are unstable. This may change in the future to just `mod`
- the `mod src` corresponds to the `src` directory. Now that your original modules
  are located at `src::` path, we can use re-exports to make a shim module with the
  same name plus dependency info
- Inside `mod src`, the use of `pub` is the same as before. The module is accessbie
  at `your_crate::your_module` if it's `pub`. (`pub` on `mod src` has no effect and `src` is never exported.)
- Re-exports and doc comments work just as if the module is declared at the outer scope.

This generates the dependency structures, but does not enforce them. After all,
proc-macros have no right to disallow `use` statements in other files.
What the macro actually does is it generates helper modules that re-exports
the dependencies.

We can use the [`import()`] macro in modules to apply the restriction:

The example is using some file in `sub_system_2`:

```rust,ignore
// some file in sub_system_2, say `src/sub_system_2/foo.rs`

#[layered_crate::import]
use sub_system_2::{
    super::util::SomeUtil,
 // ^ using `super` to mean "dependencies"
    super::sub_system_1,
 // ^ this will error, since sub_system_1 is not declared as a dependency
    SomeThingInSubSys2,
 // ^ this is equivalent to importing from `crate::sub_system_2::SomeThingInSubSys2`
 //   i.e. the current module
};
```

See the [`import()`] macro for a more detailed example. Also single `super`
in the middle of the path will not work in `rust-analyzer`, see the macro
doc for the workaround.

## Extra checks

The macro provides extra checks:
- Circular dependency is not allowed
- For readability, the modules need to be declared top-down. If A depends on B,
  you need to declare A first, then B.
- The `#[depends_on]` attribute needs to be sorted according to the
  same order the modules are defined in
- Warning if `#[depends_on]` specified for a module that doesn't actually use the dependency.
  Note this check is actually done by the compiler, and it only counts if you are using
  the dependency through the generated `crate_` module.

You can see the checks in action in the tests of this crate.
