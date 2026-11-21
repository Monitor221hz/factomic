type List<T> = Vec<T>;

#[derive(factomic::Entity)]
struct Model {
    #[factomic(value)]
    values: List<String>,
}

fn main() {}
