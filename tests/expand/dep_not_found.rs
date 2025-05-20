use layered_crate::layers;

#[layers]
mod src {
    #[depends_on(y)]
    pub extern crate x;
}

fn main() {}
