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
mod test3 {
    use crate::current::{
        {},
        crate_::{MyTypeInThisLayer, MyDepInThisLayer},
    };
    use crate::current::crate_::{MyTypeInThisLayer, MyDepInThisLayer};
    use crate::current::{MyTypeInThisLayer, MyDepInThisLayer};
}
fn main() {}
