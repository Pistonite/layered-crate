use layered_crate::layers;
#[doc(hidden)]
pub(crate) mod src {
    /// doc comments are retained
    pub mod y {}
    pub mod x {}
}
#[doc(inline)]
pub(crate) use src::x;
/// doc comments are retained
pub mod y {
    #[doc(inline)]
    pub use crate::src::y::*;
    #[doc(hidden)]
    pub(crate) mod crate_ {
        pub use crate::src::x;
    }
}
