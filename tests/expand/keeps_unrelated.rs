use layered_crate::layers;

#[layers]
/// This should be kept
#[doc(hidden)]
#[otther]
mod src {
    #[depends_on(y)]
    pub extern crate x;
    // even though non-inline module is unstable, it's still supported
    // if passed in
    #[this_is_kept]
    pub mod y;

    // inline module is ok and kept
    pub mod z {
        pub fn hey() {}
    }

    // other items are kept
    pub fn say_hello() {
        println!("hello");
    }
}
