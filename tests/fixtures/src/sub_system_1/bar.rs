#[layered_crate::import]
use sub_system_1::{
    super::utils,
    self::sub1,
};

pub fn sub2() -> u32 {
    utils::x();
    sub1();
    420
}
