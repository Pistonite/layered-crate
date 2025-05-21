use layered_crate::layers;
/// This should be kept
#[doc(hidden)]
#[otther]
pub(crate) mod src {
    pub mod x {}
    #[this_is_kept]
    pub mod y {}
    pub mod z {
        pub fn hey() {}
    }
    pub fn say_hello() {
        {
            ::std::io::_print(format_args!("hello\n"));
        };
    }
}
pub mod x {
    #[doc(inline)]
    pub use crate::src::x::*;
    #[doc(hidden)]
    pub(crate) mod crate_ {
        pub use crate::src::y;
    }
}
#[doc(inline)]
pub use src::y;
#[doc(inline)]
pub use src::z;
