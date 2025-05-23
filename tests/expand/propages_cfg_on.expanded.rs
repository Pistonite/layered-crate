use layered_crate::layers;
#[doc(hidden)]
pub(crate) mod src {
    pub mod x {}
    #[this_is_kept]
    pub mod y {}
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
