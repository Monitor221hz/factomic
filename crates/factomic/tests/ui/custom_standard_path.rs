mod custom {
    pub struct Option<T>(pub T);
}

#[derive(factomic::Entity)]
struct Model {
    value: custom::Option<String>,
}

fn main() {}
