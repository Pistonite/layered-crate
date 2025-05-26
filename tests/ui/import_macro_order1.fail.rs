mod current {
    pub struct MyTypeInThisLayer;
    pub struct MyDepInThisLayer;
    pub mod crate_ {
        pub mod my_dep {
            pub struct MyType;
        }
        pub mod my_other_dep {
            pub struct MyOtherType;
        }
        pub mod my_more_dep {
            pub struct MyMoreType;
        }
    }
}

mod test {
    #[layered_crate::import]
    use current::{
        self::MyTypeInThisLayer,
        super::my_dep::MyType,
        super::my_other_dep::MyOtherType,
        super::my_more_dep::MyMoreType,
        self::MyDepInThisLayer,
    };
}


fn main() {}
