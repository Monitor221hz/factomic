use std::sync::Arc;

#[derive(factomic::Entity)]
struct Model {
    #[factomic(value)]
    value: Arc<String>,
}

fn main() {}
