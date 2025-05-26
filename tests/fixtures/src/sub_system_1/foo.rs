#[layered_crate::import]
use sub_system_1::super::utils;

pub fn sub1() -> u32 {
    utils::x();
    37
}
