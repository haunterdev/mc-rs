//! Writes the icon as a `.ico` file, for the website's favicon and logo:
//! `cargo run --release -p brand --example icon -- <out.ico>`.

fn main() {
    let out = std::env::args().nth(1).expect("usage: icon <out.ico>");
    std::fs::write(&out, brand::ico(&[16, 32, 48, 64, 128, 256])).expect("writing the icon");
}
