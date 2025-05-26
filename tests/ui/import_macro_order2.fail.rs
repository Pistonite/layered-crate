mod current {
    pub struct MyTypeInThisLayer;
    pub struct MyDepInThisLayer;
    pub mod crate_ {
        pub mod m {
            pub fn a() {}
            pub fn b() {}
        }
        pub mod my_other_dep {
            pub struct MyOtherType;
        }
    }
}

mod test {
    #[layered_crate::import]
    use current::{
        super::my_other_dep::MyOtherType,
        self::MyTypeInThisLayer,
        super::m::{a, b},
        self::MyDepInThisLayer,
    };
}

fn main() {}
