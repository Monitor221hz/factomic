use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DeriveInput, parse_macro_input};

mod expand;
mod model;
mod primitive;

use expand::expand;
use model::Model;

#[proc_macro_derive(Entity, attributes(factomic))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let model = match Model::parse(input) {
        Ok(model) => model,
        Err(error) => return error.into_compile_error().into(),
    };
    expand(&model, &factomic_path()).into()
}

/// Implements primitive support for a local type using the specified storage representation.
#[proc_macro_attribute]
pub fn primitive(args: TokenStream, input: TokenStream) -> TokenStream {
    let representation = parse_macro_input!(args as primitive::Representation);
    let input = parse_macro_input!(input as DeriveInput);
    match primitive::expand(&input, representation, &factomic_path()) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn factomic_path() -> TokenStream2 {
    match crate_name("factomic") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name.replace('-', "_"));
            quote!(::#ident)
        }
        Err(_) => quote!(::factomic),
    }
}
