# layered-crate

Enforce dependencies amongst internal modules in a crate

##### 0.2.0 -> 0.3.0, this tool is changed to a CLI tool rather than a proc-macro crate. See [this issue](https://github.com/Pistonite/layered-crate/issues/8) for details

```bash
# build the tool from source
cargo install layered-crate

# check internal dependencies amongst layers
layered-crate

# deny unused dependencies
RUSTFLAGS=-Dunused-imports layered-crate 

CARGO=/my-cargo layered-crate -- +nightly check --lib --color=always --features ... 
#     ^ change the cargo binary  ^ customize args passed to cargo
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
  - Cannot have `pub(crate)` visibility or `impl` for types from dependencies
  - Might be worse for optimization since one of the factor for inlining is if
    the inlining is across a crate boundary. However I have no clue what degree of effect this has

This tool uses a `Layerfile.toml` to specify the internal dependencies, and
automatically checks that the dependencies are respected in the code as
if they were separate crates. This allows you to keep the code in a single crate
while enforcing the internal dependencies without having to split the crate manually.

It is designed to work out of the box with existing code base by adding
the `Layerfile.toml` file. However, there are some limitations and edge cases,
especially regarding macros, that you should read about below if you have
regular or procedural macros in your code.

## Usage
To split your crate into layers, this tool expects your entry point (e.g. `src/lib.rs`)
to contain module definitions that correspond to the layers you want to create.
For example:
```rust,ignore
// src/lib.rs
mod layer1 { // inline module
    pub fn foo() {
        // ...
    }
}
pub use layer1::foo; // re-exporting the function
pub mod layer2; // non-inline module at layer2.rs or layer2/mod.rs

/* ... */
```
Note that both private and public items in the module are checked,

Then, create a `Layerfile.toml` next to `Cargo.toml` with the following content:
```toml
[crate]
exclude = [] 
# ^ optional, list of modules to delete when checking layers
# note this is different from ignoring the layer/module
# to ignore something, just don't have a [layer.<name>] section for it

[layer.layer1] # for each module you want to check in lib.rs, create a table for it
#      ^ `layer1` corresponds to `mod layer1` in the code above
depends-on = ["layer2"] # list of layers that this layer depends on
impl = [] # any layer specified here will be checked together, see below for more details

[layer.layer2]
# ^ if the layer is at the bottom (doesn't depend on any other layer),
# you still need to create an empty table for it like this
```

Now, simply run `layered-crate` to check for violations - you will get an error if anything in `layer2` imports from `layer1`!

To detect and deny unused layers specified in `depends-on`, you can use the `RUSTFLAGS` environment variable
to pass custom compiler flags to cargo
```bash
RUSTFLAGS=-Dunused-imports layered-crate
```

During the layer checking, the layer and its dependencies are split
into different crates, so features that normally would work for you in the 
same-crate setup might not work as expected. Please read the limitations below

## `pub(crate)` visibility and `impl` for types from dependencies
If one of your layers depends on an item that is `pub(crate)` in a layer below,
or needs to implement a type for a layer below, you will get an error since
the layer and its dependencies are split into different crates during layer checking.

To workaround this, add the `impl` property to the layer in `Layerfile.toml`:

```toml
[layer.layer1]
depends-on = ["layer2"]
impl = ["layer2"]       # <- add this

[layer.layer2]
```
When checking `layer1`, the tool will also put `layer2` in the same test crate as `layer1`.
However, the check is loosened in this case, since `layer1` can also import
from `layer2`'s dependencies (i.e. transitive dependencies).

`layer2` still cannot import from `layer1` - you will get an error when checking `layer2`

## Crate name in macro expansion
Macro expansion can give some nasty errors - especially procedural macros.
If your crate uses macros (including procedural macros), please read 
[this issue on GitHub](https://github.com/Pistonite/layered-crate/issues/8#issuecomment-2923598649)
before considering this tool.

## Other Limitations
Here are some more limitations of the tool other than the ones
mentioned above:

1. Currently, we can only check library targets. For binary target,
   you have to declare a library target, then use that in your `main.rs`:
   ```toml
   # these are the defaults so you can omit them
   [lib]
   name = "my_lib"      
   path = "src/lib.rs"

   [[bin]]
   name = "my_bin"
   path = "src/main.rs"
   ```
   ```rust
   // src/main.rs
   fn main() { my_lib::main_internal() }
   ```
   
2. We do not support modules produced by macros in the entry point, as we purely
   parse the entry point as syntax tree. Macros in other modules are fine.
