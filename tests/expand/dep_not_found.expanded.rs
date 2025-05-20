use layered_crate::layers;
#[doc(hidden)]
pub(crate) mod src {
    pub mod x {}
}
#[doc(inline)]
pub use src::x;
fn main() {}
