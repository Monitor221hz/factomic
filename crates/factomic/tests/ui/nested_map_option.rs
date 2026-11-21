use std::collections::HashMap;

#[derive(factomic::Entity)]
struct Model {
    values: HashMap<String, Option<Vec<String>>>,
}

fn main() {}
