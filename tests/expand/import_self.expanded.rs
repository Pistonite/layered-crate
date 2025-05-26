#![allow(unused)]
mod current {
    pub struct MyTypeInThisLayer;
    pub struct MyDepInThisLayer;
}
mod test {
    use crate::current::{MyTypeInThisLayer, MyDepInThisLayer};
}
mod test2 {
    use crate::current::{self, {MyTypeInThisLayer, MyDepInThisLayer}};
}
fn main() {}
