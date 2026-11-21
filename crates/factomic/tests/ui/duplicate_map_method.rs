use std::collections::HashMap;

#[derive(factomic::Entity)]
struct Model {
    values: HashMap<String, i64>,
    values_seek: String,
}

fn main() {}
