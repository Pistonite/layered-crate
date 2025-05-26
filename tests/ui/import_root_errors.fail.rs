mod f;

#[layered_crate::import]
use *;
#[layered_crate::import]
use {A, B};
#[layered_crate::import]
use c;
#[layered_crate::import]
use d as e;
#[layered_crate::import]
use ::f::{x, y};
#[layered_crate::import]
use crate::f;
#[layered_crate::import]
use w::*;
#[layered_crate::import]
use w::prog;
#[layered_crate::import]
use w::super;
#[layered_crate::import]
use w::prog as t;
#[layered_crate::import]
use w::super as ok;
#[layered_crate::import]
use w::{super, x};
#[layered_crate::import]
use w::crate_;
#[layered_crate::import]
use w::crate_ as nokay;
#[layered_crate::import]
use w::{crate_ as nokay, t};
#[layered_crate::import]
use w::{super as nokay, t};

fn main() {}
