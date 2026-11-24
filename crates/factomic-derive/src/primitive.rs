use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{DeriveInput, Error, Ident, Result};

pub enum Representation {
    String,
    Long,
    Timestamp,
    Bytes,
}

impl Parse for Representation {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.is_empty() {
            return Err(input.error("expected `string`, `long`, `timestamp`, or `bytes`"));
        }
        let ident = input.parse::<Ident>()?;
        if !input.is_empty() {
            return Err(input.error("expected one primitive representation"));
        }
        match ident.to_string().as_str() {
            "string" => Ok(Self::String),
            "long" => Ok(Self::Long),
            "timestamp" => Ok(Self::Timestamp),
            "bytes" => Ok(Self::Bytes),
            _ => Err(Error::new(
                ident.span(),
                "expected `string`, `long`, `timestamp`, or `bytes`",
            )),
        }
    }
}

pub fn expand(
    input: &DeriveInput,
    representation: Representation,
    crate_path: &TokenStream,
) -> Result<TokenStream> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(Error::new_spanned(
            &input.generics,
            "generic primitive types are not supported",
        ));
    }

    let ident = &input.ident;
    let implementations = match representation {
        Representation::String => quote! {
            fn to_string(value: &#ident) -> ::std::string::String {
                ::std::string::ToString::to_string(value)
            }

            fn from_string(value: ::std::string::String) -> #crate_path::Result<#ident> {
                <#ident as ::std::str::FromStr>::from_str(&value)
                    .map_err(|_| #crate_path::Error::CorruptPrimitive(#crate_path::Type::String))
            }

            #crate_path::impl_string_primitive!(#ident, to_string, from_string);
        },
        Representation::Long => quote! {
            fn as_i64(value: &#ident) -> i64 {
                <i64 as ::core::convert::From<&#ident>>::from(value)
            }

            fn from_i64(value: i64) -> #crate_path::Result<#ident> {
                <#ident as ::core::convert::TryFrom<i64>>::try_from(value)
                    .map_err(|_| #crate_path::Error::CorruptPrimitive(#crate_path::Type::Long))
            }

            #crate_path::impl_long_primitive!(#ident, as_i64, from_i64);
        },
        Representation::Timestamp => quote! {
            fn as_microsecond(value: &#ident) -> i64 {
                <i64 as ::core::convert::From<&#ident>>::from(value)
            }

            fn from_microsecond(micros: i64) -> #crate_path::Result<#ident> {
                <#ident as ::core::convert::TryFrom<i64>>::try_from(micros)
                    .map_err(|_| #crate_path::Error::CorruptPrimitive(#crate_path::Type::Timestamp))
            }

            #crate_path::impl_timestamp_primitive!(#ident, as_microsecond, from_microsecond);
        },
        Representation::Bytes => quote! {
            fn as_bytes(value: &#ident) -> &[u8] {
                <#ident as ::core::convert::AsRef<[u8]>>::as_ref(value)
            }

            fn from_bytes(value: ::std::vec::Vec<u8>) -> #crate_path::Result<#ident> {
                <#ident as ::core::convert::TryFrom<::std::vec::Vec<u8>>>::try_from(value)
                    .map_err(|_| #crate_path::Error::CorruptPrimitive(#crate_path::Type::Bytes))
            }

            #crate_path::impl_bytes_primitive!(#ident, as_bytes, from_bytes);
        },
    };

    Ok(quote! {
        #input

        const _: () = {
            #implementations
        };
    })
}
