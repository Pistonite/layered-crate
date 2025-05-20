use crate::api::crate_;

use crate_::{utils, sub_system_1, sub_system_2};

pub fn add(a: i32, b: i32) -> i32 {
    utils::x();
    sub_system_1::sub1();
    sub_system_2::sub2();
    a + b
}

pub fn sub(a: i32, b: i32) -> i32 {
    a - b
}
