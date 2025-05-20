use layered_crate::layers;
#[doc(hidden)]
pub(crate) mod src {
    pub mod x {}
}
pub mod x {
    #[doc(inline)]
    pub use crate::src::x::*;
    #[doc(hidden)]
    #[allow(unused_imports)]
    pub(crate) mod crate_ {
        pub use crate::src::x;
    }
}
fn main() {
    x::do_something();
}
