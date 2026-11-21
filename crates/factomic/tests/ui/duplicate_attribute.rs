#[derive(factomic::Entity)]
struct Model {
    #[factomic(name = "shared/name")]
    first: String,
    #[factomic(name = "shared/name")]
    second: String,
}

fn main() {}
