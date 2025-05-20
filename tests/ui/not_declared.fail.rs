use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(x)]
    pub extern crate w;
    pub extern crate x;
}

fn main() {
    w::do_something();
    x::do_something();
}
