#[derive(factomic::Entity)]
#[factomic(namespace = "first")]
#[factomic(namespace = "second")]
struct Model {
    #[factomic(name = "shared/value")]
    #[factomic(name = "shared/other")]
    #[factomic(indexed)]
    #[factomic(indexed)]
    #[factomic(component)]
    #[factomic(component)]
    target: factomic::EntityId,
}

fn main() {}
