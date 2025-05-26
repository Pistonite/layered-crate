#[layered_crate::import]
use sub_system_2::super::utils;

pub fn sub2() -> u32 {
    utils::x();
    42
}
