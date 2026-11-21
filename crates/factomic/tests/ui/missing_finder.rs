#[derive(factomic::Entity)]
struct Model {
    value: String,
}

fn find(query: &factomic::DbQuery<Model>) {
    query.find_by_value("x");
}

fn main() {}
