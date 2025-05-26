#[layered_crate::import]
use clayer::{
    self::MyTypeInThisLayer,
    super::my_dep::MyType,
    super::my_dep2::{Type1, Type2},
    super::{
        more_dep::{A, B, C},
        more_nested::{
            D, E,
            F::{G, H},
        },
    },
    MyDepInThisLayer,
};

// single should work as well
#[layered_crate::import]
use clayer::super::my_dep::MyType;
#[layered_crate::import]
pub use clayer::my_dep::MyType;
