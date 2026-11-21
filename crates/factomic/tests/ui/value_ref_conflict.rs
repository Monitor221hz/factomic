#[derive(factomic::Entity)]
struct Model {
    #[factomic(value, ref)]
    value: String,
}

fn main() {}
