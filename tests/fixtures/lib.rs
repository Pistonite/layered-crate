use layered_crate::layers;

#[layers]
pub mod src {
    /// My Public APIs
    #[depends_on(sub_system_1)]
    #[depends_on(sub_system_2)]
    #[depends_on(utils)]
    extern crate api;

    /// Sub-system 1 if you need
    #[depends_on(utils)]
    pub extern crate sub_system_1;

    /// Sub-system 2 if you need
    #[depends_on(utils)]
    pub extern crate sub_system_2;

    /// Internal utils
    extern crate utils;
}

#[doc(inline)]
pub use api::*;
