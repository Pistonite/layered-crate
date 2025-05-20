use layered_crate::layers;

#[layers]
mod src {
    pub extern crate y;
    #[depends_on(y)]
    pub extern crate z;
}

fn main() {
    z::zzz();
    y::do_something();
}
