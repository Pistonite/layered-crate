use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(x)]
    pub extern crate x;
}

fn main() {
    x::do_something();
}
