use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(y)]
    #[depends_on(x)]
    extern crate z; // this should have warning since z is never used

    extern crate y;
    extern crate x;
}

fn main() {
    compile_error!("make it fail so we can check the warnings")
}
