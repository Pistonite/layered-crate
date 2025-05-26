#![allow(unused)]

mod current {
    pub struct MyTypeInThisLayer;
    pub struct MyDepInThisLayer;
}

mod test {
    #[layered_crate::import]
    use current::{
        MyTypeInThisLayer,
        self::MyDepInThisLayer,
    };

}

mod test2 {
    // this should not produce any error
    #[layered_crate::import]
    use current::{
        self,
        {MyTypeInThisLayer, MyDepInThisLayer},
    };
}

mod test3 {
    #[layered_crate::import]
    use current::{{}, super::{MyTypeInThisLayer, MyDepInThisLayer}};

    #[layered_crate::import]
    use current::super_::{MyTypeInThisLayer, MyDepInThisLayer};

    #[layered_crate::import]
    use current::{
        self::{MyTypeInThisLayer, MyDepInThisLayer},
    };
}


fn main() {}
