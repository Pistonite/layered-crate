use layered_crate::layers;
#[doc(hidden)]
pub(crate) mod src {
    /// My Public APIs
    pub mod api {}
    /// Sub-system 1 if you need
    pub mod sub_system_1 {}
    /// Sub-system 2 if you need
    pub mod sub_system_2 {}
    /// Internal utils
    pub mod utils {}
}
/// My Public APIs
pub(crate) mod api {
    #[doc(inline)]
    pub use crate::src::api::*;
    #[doc(hidden)]
    pub(crate) mod crate_ {
        pub use crate::src::sub_system_1;
        pub use crate::src::sub_system_2;
        pub use crate::src::utils;
    }
}
/// Sub-system 1 if you need
pub mod sub_system_1 {
    #[doc(inline)]
    pub use crate::src::sub_system_1::*;
    #[doc(hidden)]
    pub(crate) mod crate_ {
        pub use crate::src::utils;
    }
}
/// Sub-system 2 if you need
pub mod sub_system_2 {
    #[doc(inline)]
    pub use crate::src::sub_system_2::*;
    #[doc(hidden)]
    pub(crate) mod crate_ {
        pub use crate::src::utils;
    }
}
#[doc(inline)]
pub(crate) use src::utils;
#[doc(inline)]
pub use api::*;
