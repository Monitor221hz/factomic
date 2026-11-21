use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use syn::{DeriveInput, parse_macro_input};

mod expand;
mod model;

use expand::expand;
use model::Model;

#[proc_macro_derive(Entity, attributes(factomic))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let model = match Model::parse(input) {
        Ok(model) => model,
        Err(error) => return error.into_compile_error().into(),
    };
    let crate_path = match crate_name("factomic") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name.replace('-', "_"));
            quote!(::#ident)
        }
        Err(_) => quote!(::factomic),
    };
    expand(&model, &crate_path).into()
}
