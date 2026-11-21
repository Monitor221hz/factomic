use std::collections::HashMap;

#[derive(factomic::Entity)]
struct Model {
    values: HashMap<bool, String>,
}

fn main() {}
