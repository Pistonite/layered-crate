use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(y)]
    pub extern crate x;
    #[depends_on(x)]
    pub extern crate y;
}

fn main() {
    x::do_something();
}
