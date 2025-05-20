use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(y)]
    #[depends_on(x)]
    pub extern crate z;

    pub extern crate x;
    pub extern crate y;
}

fn main() {
    x::do_something();
    z::zzz();
    y::do_something();
}
