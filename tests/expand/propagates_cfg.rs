use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(y)]
    pub extern crate x;

    // has to be something that won't get removed by cargo-expand
    #[this_is_kept]
    #[cfg(not(feature = "y"))]
    pub mod y;
}
