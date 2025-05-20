use layered_crate::layers;

#[layers]
mod src {
    /// doc comments are retained
    #[depends_on(x)]
    pub extern crate y;
    extern crate x;
}
