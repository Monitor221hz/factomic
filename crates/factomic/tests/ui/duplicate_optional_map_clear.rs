use std::collections::HashMap;

#[derive(factomic::Entity)]
struct Model {
    values: HashMap<String, Option<i64>>,
    values_seek: Option<String>,
}

fn main() {}
