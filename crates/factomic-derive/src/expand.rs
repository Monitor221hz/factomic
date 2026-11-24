use proc_macro2::{Group, Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Ident, LitStr, ext::IdentExt};

use crate::model::{Cell, Field, FieldKind, Model, PrimitiveKind};

pub fn expand(model: &Model, crate_path: &TokenStream) -> TokenStream {
    let schema = expand_schema(model);
    let read = expand_read(model);
    let mutation = expand_mutation(model);
    let queries = expand_queries(model);
    let output = quote! {
        #schema
        #read
        #mutation
        #queries
    };
    replace_crate_path(output, crate_path)
}

fn expand_schema(model: &Model) -> TokenStream {
    let Model {
        vis, ident, fields, ..
    } = model;
    let schema_ident = derived_ident("", ident, "Schema");
    let definitions = fields.iter().map(|field| {
        let ident = &field.ident;
        let def_ident = derived_ident("", ident, "_def");
        let name = LitStr::new(&field.name, Span::call_site());
        let kind = match field.kind {
            FieldKind::One { .. } => quote!(::factomic::CollectionKind::One),
            FieldKind::List(_) => quote!(::factomic::CollectionKind::List),
            FieldKind::Many(_) => quote!(::factomic::CollectionKind::Many),
            FieldKind::Map { .. } => quote!(::factomic::CollectionKind::Map),
        };
        let value_type = match &field.kind {
            FieldKind::One { cell, .. } | FieldKind::List(cell) | FieldKind::Many(cell) => {
                schema_type(cell)
            }
            FieldKind::Map { value, .. } => schema_type(value),
        };
        let key_type = match &field.kind {
            FieldKind::Map { key, .. } => {
                let key_type = schema_type(key);
                quote!(::core::option::Option::Some(#key_type))
            }
            _ => quote!(::core::option::Option::None),
        };
        let indexed = field.indexed;
        let component = field.component;
        quote! {
            fn #def_ident() -> ::factomic::AttributeDef {
                ::factomic::AttributeDef {
                    name: ::std::string::String::from(#name),
                    value_type: #value_type,
                    kind: #kind,
                    key_type: #key_type,
                    indexed: #indexed,
                    is_component: #component,
                }
            }
        }
    });
    let declarations = fields.iter().map(|field| {
        let def_ident = derived_ident("", &field.ident, "_def");
        quote!(Self::#def_ident())
    });
    let declaration_idents = fields.iter().map(|field| &field.ident);
    let declaration_init_idents = fields.iter().map(|field| &field.ident);
    let lookups = fields.iter().map(|field| {
        let ident = &field.ident;
        let name = LitStr::new(&field.name, Span::call_site());
        let def_ident = derived_ident("", ident, "_def");
        quote! {
            let ::core::option::Option::Some(#ident) = db.attribute_id(#name)? else {
                return ::core::result::Result::Ok(::core::option::Option::None);
            };
            if db.attribute_def(#ident)? != Self::#def_ident() {
                return ::core::result::Result::Err(::factomic::Error::SchemaConflict(
                    ::std::string::String::from(#name),
                ));
            }
        }
    });
    let struct_fields = fields.iter().map(|field| {
        let ident = &field.ident;
        quote!(#ident: ::factomic::AttributeId)
    });
    let init_idents = fields.iter().map(|field| &field.ident);
    quote! {
        #vis struct #schema_ident {
            #(#struct_fields),*
        }

        impl #schema_ident {
            #(#definitions)*
        }

        impl ::factomic::Entity for #ident {
            type Schema = #schema_ident;
        }

        impl ::factomic::Schema for #schema_ident {
            fn declare(store: &::factomic::Store) -> ::factomic::Result<Self> {
                let [#(#declaration_idents),*] =
                    store.declare_attributes([#(#declarations),*])?;
                ::core::result::Result::Ok(Self {
                    #(#declaration_init_idents),*
                })
            }

            fn get(
                db: &::std::sync::Arc<::factomic::Db>,
            ) -> ::factomic::Result<::core::option::Option<Self>> {
                #(#lookups)*
                ::core::result::Result::Ok(::core::option::Option::Some(Self {
                    #(#init_idents),*
                }))
            }
        }
    }
}

fn expand_read(model: &Model) -> TokenStream {
    let Model {
        vis, ident, fields, ..
    } = model;
    let trait_ident = derived_ident("", ident, "Entity");
    let (methods, implementations): (Vec<_>, Vec<_>) = fields.iter().map(expand_read_field).unzip();
    quote! {
        #vis trait #trait_ident {
            #(#methods)*
        }

        impl #trait_ident for ::factomic::DbHandle<#ident> {
            #(#implementations)*
        }
    }
}

fn expand_read_field(field: &Field) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    match &field.kind {
        FieldKind::One { cell, .. } => {
            let output = output_type(cell);
            let query = quote!(self.db.entity_value(self.entity, self.schema.#ident));
            let body = read_one_body(cell, query);
            (
                quote!(fn #ident(&self) -> ::factomic::Result<::core::option::Option<#output>>;),
                quote! {
                    fn #ident(&self) -> ::factomic::Result<::core::option::Option<#output>> {
                        #body
                    }
                },
            )
        }
        FieldKind::List(cell) | FieldKind::Many(cell) => {
            let output = output_type(cell);
            let raw = raw_type(cell);
            let body = read_many_body(cell);
            let at = if matches!(field.kind, FieldKind::List(_)) {
                let at_ident = derived_ident("", ident, "_at");
                let query = quote!(self.db.entity_value_at::<#raw>(
                    self.entity,
                    self.schema.#ident,
                    ordinal,
                ));
                let at_body = read_one_body(cell, query);
                (
                    quote! {
                        fn #at_ident(
                            &self,
                            ordinal: ::core::primitive::usize,
                        ) -> ::factomic::Result<::core::option::Option<#output>>;
                    },
                    quote! {
                        fn #at_ident(
                            &self,
                            ordinal: ::core::primitive::usize,
                        ) -> ::factomic::Result<::core::option::Option<#output>> {
                            #at_body
                        }
                    },
                )
            } else {
                (quote!(), quote!())
            };
            let (at_declaration, at_implementation) = at;
            (
                quote! {
                    fn #ident(&self) -> impl ::core::iter::Iterator<Item = ::factomic::Result<#output>> + '_;
                    #at_declaration
                },
                quote! {
                    fn #ident(&self) -> impl ::core::iter::Iterator<Item = ::factomic::Result<#output>> + '_ {
                        self.db.entity_values::<#raw>(self.entity, self.schema.#ident)
                            #body
                    }
                    #at_implementation
                },
            )
        }
        FieldKind::Map { key, value, .. } => {
            let key_output = output_type(key);
            let value_output = output_type(value);
            let raw_key = raw_type(key);
            let raw_value = raw_type(value);
            let body = read_map_body(key, value);
            let seek_ident = derived_ident("", ident, "_seek");
            let key_input = map_key_input(key, false);
            let key_expr = map_key_query(key);
            let query = quote!(self.db.entity_keyvalue::<#raw_value>(
                self.entity,
                self.schema.#ident,
                #key_expr,
            ));
            let seek_body = read_one_body(value, query);
            (
                quote! {
                    fn #ident(
                        &self,
                    ) -> impl ::core::iter::Iterator<Item = ::factomic::Result<(#key_output, #value_output)>> + '_;
                    fn #seek_ident(
                        &self,
                        key: #key_input,
                    ) -> ::factomic::Result<::core::option::Option<#value_output>>;
                },
                quote! {
                    fn #ident(
                        &self,
                    ) -> impl ::core::iter::Iterator<Item = ::factomic::Result<(#key_output, #value_output)>> + '_ {
                        self.db.entity_keyvalues::<#raw_key, #raw_value>(
                            self.entity,
                            self.schema.#ident,
                        )
                        #body
                    }

                    fn #seek_ident(
                        &self,
                        key: #key_input,
                    ) -> ::factomic::Result<::core::option::Option<#value_output>> {
                        #seek_body
                    }
                },
            )
        }
    }
}

fn read_one_body(cell: &Cell, query: TokenStream) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => query,
        Cell::Reference(_) => quote! {
            {
                let ::core::option::Option::Some(__factomic_entity) = #query? else {
                    return ::core::result::Result::Ok(::core::option::Option::None);
                };
                self.db.entity(__factomic_entity)
            }
        },
    }
}

fn read_many_body(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(),
        Cell::Reference(_) => quote! {
            .filter_map(|__factomic_result| match __factomic_result {
                ::core::result::Result::Ok(__factomic_entity) => {
                    self.db.entity(__factomic_entity).transpose()
                }
                ::core::result::Result::Err(__factomic_error) => {
                    ::core::option::Option::Some(::core::result::Result::Err(__factomic_error))
                }
            })
        },
    }
}

fn read_map_body(key: &Cell, value: &Cell) -> TokenStream {
    if matches!(key, Cell::Primitive { .. }) && matches!(value, Cell::Primitive { .. }) {
        return quote!();
    }
    let key = resolve_map_part(key, quote!(__factomic_key));
    let value = resolve_map_part(value, quote!(__factomic_value));
    quote! {
        .filter_map(|__factomic_result| {
            let (__factomic_key, __factomic_value) = match __factomic_result {
                ::core::result::Result::Ok(__factomic_pair) => __factomic_pair,
                ::core::result::Result::Err(__factomic_error) => {
                    return ::core::option::Option::Some(::core::result::Result::Err(
                        __factomic_error,
                    ));
                }
            };
            #key
            #value
            ::core::option::Option::Some(::core::result::Result::Ok((
                __factomic_key,
                __factomic_value,
            )))
        })
    }
}

fn resolve_map_part(cell: &Cell, raw: TokenStream) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(),
        Cell::Reference(_) => quote! {
            let #raw = match self.db.entity(#raw) {
                ::core::result::Result::Ok(::core::option::Option::Some(__factomic_resolved)) => {
                    __factomic_resolved
                }
                ::core::result::Result::Ok(::core::option::Option::None) => {
                    return ::core::option::Option::None;
                }
                ::core::result::Result::Err(__factomic_error) => {
                    return ::core::option::Option::Some(::core::result::Result::Err(
                        __factomic_error,
                    ));
                }
            };
        },
    }
}

fn expand_mutation(model: &Model) -> TokenStream {
    let Model {
        vis, ident, fields, ..
    } = model;
    let trait_ident = derived_ident("", ident, "EntityMut");
    let (methods, implementations): (Vec<_>, Vec<_>) =
        fields.iter().map(expand_mutation_field).unzip();
    quote! {
        #vis trait #trait_ident: ::core::marker::Sized {
            #(#methods)*
        }

        impl<'a> #trait_ident for ::factomic::EntityTransaction<'a, #ident> {
            #(#implementations)*
        }
    }
}

fn expand_mutation_field(field: &Field) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    let set_ident = derived_ident("set_", ident, "");
    match &field.kind {
        FieldKind::One { cell, optional } => {
            let input = mutation_value_input(cell, false);
            let body = mutation_value_conversion(cell);
            let context = mutation_context(field);
            let (clear_declaration, clear_implementation) = if *optional {
                clear_one_field(field)
            } else {
                (quote!(), quote!())
            };
            (
                quote! {
                    fn #set_ident(&mut self, value: #input) -> &mut Self;
                    #clear_declaration
                },
                quote! {
                    fn #set_ident(&mut self, value: #input) -> &mut Self {
                        #body
                        #context
                        self.transaction_mut().assert(
                            __factomic_entity,
                            __factomic_attr,
                            &__factomic_value,
                        );
                        self
                    }
                    #clear_implementation
                },
            )
        }
        FieldKind::List(cell) | FieldKind::Many(cell) => {
            let input = mutation_value_input(cell, true);
            let lifetime = string_lifetime(cell);
            let operation = if matches!(field.kind, FieldKind::List(_)) {
                quote!(replace_list)
            } else {
                quote!(replace_many)
            };
            let conversion = mutation_values_conversion(cell);
            let context = mutation_context(field);
            let at = if matches!(field.kind, FieldKind::List(_)) {
                let at_ident = derived_ident("set_", ident, "_at");
                let at_input = mutation_value_input(cell, false);
                let body = mutation_value_conversion(cell);
                let at_context = context.clone();
                (
                    quote! {
                        fn #at_ident(&mut self, ordinal: ::core::primitive::usize, value: #at_input) -> &mut Self;
                    },
                    quote! {
                        fn #at_ident(&mut self, ordinal: ::core::primitive::usize, value: #at_input) -> &mut Self {
                            #body
                            #at_context
                            self.transaction_mut().replace_list_at(
                                __factomic_entity,
                                __factomic_attr,
                                ordinal,
                                &__factomic_value,
                            );
                            self
                        }
                    },
                )
            } else {
                (quote!(), quote!())
            };
            let (at_declaration, at_implementation) = at;
            let (incremental_declaration, incremental_implementation) =
                incremental_collection_field(field);
            (
                quote! {
                    fn #set_ident #lifetime(&mut self, values: #input) -> &mut Self;
                    #at_declaration
                    #incremental_declaration
                },
                quote! {
                    fn #set_ident #lifetime(&mut self, values: #input) -> &mut Self {
                        #conversion
                        #context
                        self.transaction_mut().#operation(
                            __factomic_entity,
                            __factomic_attr,
                            __factomic_values,
                        );
                        self
                    }
                    #at_implementation
                    #incremental_implementation
                },
            )
        }
        FieldKind::Map {
            key,
            value,
            optional_values,
        } => expand_map_mutation_field(field, key, value, *optional_values, &set_ident),
    }
}

fn expand_map_mutation_field(
    field: &Field,
    key: &Cell,
    value: &Cell,
    optional_values: bool,
    set_ident: &Ident,
) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    let lifetime = string_lifetime(value);
    let key_input = map_key_input(key, true);
    let value_input = mutation_map_value_input(value);
    let value_input = if optional_values {
        quote!(::core::option::Option<#value_input>)
    } else {
        value_input
    };
    let key_conversion = mutation_map_key_conversion(key);
    let value_conversion = mutation_map_value_conversion(value);
    let entries = if optional_values {
        deduplicate_optional_map_entries(key, value)
    } else {
        quote! {
            let __factomic_entries = entries.map(|(__factomic_key, __factomic_value)| {
                (#key_conversion, #value_conversion)
            });
        }
    };
    let seek_ident = derived_ident("set_", ident, "_seek");
    let seek_key = map_key_input(key, false);
    let seek_value = mutation_value_input(value, false);
    let seek_key_conversion = map_key_mutation(key);
    let seek_value_conversion = mutation_value_conversion(value);
    let context = mutation_context(field);
    let (clear_declaration, clear_implementation) = if optional_values {
        clear_map_field(field, key)
    } else {
        (quote!(), quote!())
    };
    let (incremental_declaration, incremental_implementation) = incremental_collection_field(field);
    (
        quote! {
            fn #set_ident #lifetime(
                &mut self,
                entries: impl ::core::iter::Iterator<Item = (#key_input, #value_input)>,
            ) -> &mut Self;
            fn #seek_ident(
                &mut self,
                key: #seek_key,
                value: #seek_value,
            ) -> &mut Self;
            #clear_declaration
            #incremental_declaration
        },
        quote! {
            fn #set_ident #lifetime(
                &mut self,
                entries: impl ::core::iter::Iterator<Item = (#key_input, #value_input)>,
            ) -> &mut Self {
                #entries
                #context
                self.transaction_mut().replace_map(
                    __factomic_entity,
                    __factomic_attr,
                    __factomic_entries,
                );
                self
            }

            fn #seek_ident(
                &mut self,
                key: #seek_key,
                value: #seek_value,
            ) -> &mut Self {
                #seek_value_conversion
                #context
                self.transaction_mut().put(
                    __factomic_entity,
                    __factomic_attr,
                    #seek_key_conversion,
                    &__factomic_value,
                );
                self
            }
            #clear_implementation
            #incremental_implementation
        },
    )
}

fn incremental_collection_field(field: &Field) -> (TokenStream, TokenStream) {
    match &field.kind {
        FieldKind::List(cell) => incremental_list_field(field, cell),
        FieldKind::Many(cell) => {
            let ident = &field.ident;
            let add = incremental_value_method(
                derived_ident("add_", ident, ""),
                cell,
                field,
                quote!(assert),
            );
            let remove = incremental_value_method(
                derived_ident("remove_", ident, ""),
                cell,
                field,
                quote!(retract),
            );
            let (declarations, implementations): (Vec<_>, Vec<_>) =
                [add, remove].into_iter().unzip();
            (quote!(#(#declarations)*), quote!(#(#implementations)*))
        }
        FieldKind::Map { key, .. } => {
            let ident = &field.ident;
            let remove_ident = derived_ident("remove_", ident, "_key");
            let key_input = map_key_input(key, false);
            let key_conversion = map_key_mutation(key);
            let context = mutation_context(field);
            (
                quote!(fn #remove_ident(&mut self, key: #key_input) -> &mut Self;),
                quote! {
                    fn #remove_ident(&mut self, key: #key_input) -> &mut Self {
                        #context
                        self.transaction_mut().remove_key(
                            __factomic_entity,
                            __factomic_attr,
                            #key_conversion,
                        );
                        self
                    }
                },
            )
        }
        FieldKind::One { .. } => (quote!(), quote!()),
    }
}

fn incremental_list_field(field: &Field, cell: &Cell) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    let push =
        incremental_value_method(derived_ident("push_", ident, ""), cell, field, quote!(push));
    let prepend = incremental_value_method(
        derived_ident("prepend_", ident, ""),
        cell,
        field,
        quote!(prepend),
    );
    let insert_ident = derived_ident("insert_", ident, "_at");
    let insert_input = mutation_value_input(cell, false);
    let insert_conversion = mutation_value_conversion(cell);
    let insert_context = mutation_context(field);
    let insert = (
        quote!(fn #insert_ident(&mut self, ordinal: ::core::primitive::usize, value: #insert_input) -> &mut Self;),
        quote! {
            fn #insert_ident(&mut self, ordinal: ::core::primitive::usize, value: #insert_input) -> &mut Self {
                #insert_conversion
                #insert_context
                self.transaction_mut().insert_list_at(
                    __factomic_entity,
                    __factomic_attr,
                    ordinal,
                    &__factomic_value,
                );
                self
            }
        },
    );
    let remove_ident = derived_ident("remove_", ident, "_at");
    let remove_context = mutation_context(field);
    let remove = (
        quote!(fn #remove_ident(&mut self, ordinal: ::core::primitive::usize) -> &mut Self;),
        quote! {
            fn #remove_ident(&mut self, ordinal: ::core::primitive::usize) -> &mut Self {
                #remove_context
                self.transaction_mut().remove_list_at(__factomic_entity, __factomic_attr, ordinal);
                self
            }
        },
    );
    let move_ident = derived_ident("move_", ident, "_at");
    let move_context = mutation_context(field);
    let move_method = (
        quote!(fn #move_ident(&mut self, from_ordinal: ::core::primitive::usize, to_ordinal: ::core::primitive::usize) -> &mut Self;),
        quote! {
            fn #move_ident(&mut self, from_ordinal: ::core::primitive::usize, to_ordinal: ::core::primitive::usize) -> &mut Self {
                #move_context
                self.transaction_mut().move_list_at(
                    __factomic_entity,
                    __factomic_attr,
                    from_ordinal,
                    to_ordinal,
                );
                self
            }
        },
    );
    let rebalance_ident = derived_ident("rebalance_", ident, "");
    let rebalance_context = mutation_context(field);
    let rebalance = (
        quote!(fn #rebalance_ident(&mut self) -> &mut Self;),
        quote! {
            fn #rebalance_ident(&mut self) -> &mut Self {
                #rebalance_context
                self.transaction_mut().rebalance(__factomic_entity, __factomic_attr);
                self
            }
        },
    );
    let (declarations, implementations): (Vec<_>, Vec<_>) =
        [push, prepend, insert, remove, move_method, rebalance]
            .into_iter()
            .unzip();
    (quote!(#(#declarations)*), quote!(#(#implementations)*))
}

fn incremental_value_method(
    method_ident: Ident,
    cell: &Cell,
    field: &Field,
    operation: TokenStream,
) -> (TokenStream, TokenStream) {
    let input = mutation_value_input(cell, false);
    let conversion = mutation_value_conversion(cell);
    let context = mutation_context(field);
    (
        quote!(fn #method_ident(&mut self, value: #input) -> &mut Self;),
        quote! {
            fn #method_ident(&mut self, value: #input) -> &mut Self {
                #conversion
                #context
                self.transaction_mut().#operation(
                    __factomic_entity,
                    __factomic_attr,
                    &__factomic_value,
                );
                self
            }
        },
    )
}

fn clear_one_field(field: &Field) -> (TokenStream, TokenStream) {
    let clear_ident = derived_ident("clear_", &field.ident, "");
    let context = mutation_context(field);
    (
        quote!(fn #clear_ident(&mut self) -> &mut Self;),
        quote! {
            fn #clear_ident(&mut self) -> &mut Self {
                #context
                self.transaction_mut().clear(__factomic_entity, __factomic_attr);
                self
            }
        },
    )
}

fn clear_map_field(field: &Field, key: &Cell) -> (TokenStream, TokenStream) {
    let clear_ident = derived_ident("clear_", &field.ident, "_seek");
    let key_input = map_key_input(key, false);
    let key_conversion = map_key_mutation(key);
    let context = mutation_context(field);
    (
        quote!(fn #clear_ident(&mut self, key: #key_input) -> &mut Self;),
        quote! {
            fn #clear_ident(&mut self, key: #key_input) -> &mut Self {
                #context
                self.transaction_mut().remove_key(
                    __factomic_entity,
                    __factomic_attr,
                    #key_conversion,
                );
                self
            }
        },
    )
}

fn deduplicate_optional_map_entries(key: &Cell, value: &Cell) -> TokenStream {
    let insert_key = match key {
        Cell::Primitive { .. } => quote!(::factomic::MapKey::from(__factomic_key)),
        Cell::Reference(_) => quote!({
            let __factomic_key: ::factomic::EntityId = __factomic_key.into();
            ::factomic::MapKey::Ref(__factomic_key)
        }),
    };
    let output_value = mutation_map_value_conversion(value);
    quote! {
        let mut __factomic_entries = ::std::collections::BTreeMap::new();
        for (__factomic_key, __factomic_value) in entries {
            let __factomic_key = #insert_key;
            let __factomic_slot = ::factomic::Slot::Key(__factomic_key.clone());
            let mut __factomic_encoded_slot = ::std::vec::Vec::new();
            __factomic_slot.encode_key(&mut __factomic_encoded_slot);
            __factomic_entries.insert(
                __factomic_encoded_slot,
                (__factomic_key, __factomic_value),
            );
        }
        let __factomic_entries = __factomic_entries.into_values().filter_map(
            |(__factomic_key, __factomic_value)| {
                __factomic_value.map(|__factomic_value| (__factomic_key, #output_value))
            },
        );
    }
}

fn mutation_context(field: &Field) -> TokenStream {
    let ident = &field.ident;
    quote! {
        let __factomic_entity = self.entity();
        let __factomic_attr = self.schema().#ident;
    }
}

fn output_type(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { ty, .. } => quote!(#ty),
        Cell::Reference(ty) => quote!(::factomic::DbHandle<#ty>),
    }
}

fn raw_type(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { ty, .. } => quote!(#ty),
        Cell::Reference(_) => quote!(::factomic::EntityId),
    }
}

fn schema_type(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Reference(_) => quote!(::factomic::Type::Ref),
        Cell::Primitive { ty, kind } => match kind {
            PrimitiveKind::Custom => quote!(<#ty as ::factomic::Primitive>::TYPE),
            PrimitiveKind::Bool => quote!(::factomic::Type::Bool),
            PrimitiveKind::I8
            | PrimitiveKind::I16
            | PrimitiveKind::I32
            | PrimitiveKind::I64
            | PrimitiveKind::U8
            | PrimitiveKind::U16
            | PrimitiveKind::U32 => quote!(::factomic::Type::Long),
            PrimitiveKind::F32 | PrimitiveKind::F64 => quote!(::factomic::Type::Double),
            PrimitiveKind::String => quote!(::factomic::Type::String),
            PrimitiveKind::EntityId => quote!(::factomic::Type::Ref),
            PrimitiveKind::Timestamp => quote!(::factomic::Type::Timestamp),
        },
    }
}

fn map_key_input(cell: &Cell, iterator: bool) -> TokenStream {
    if is_string(cell) && !iterator {
        return quote!(&::core::primitive::str);
    }
    match cell {
        Cell::Primitive { ty, .. } => quote!(#ty),
        Cell::Reference(_) => quote!(impl ::core::convert::Into<::factomic::EntityId>),
    }
}

fn map_key_query(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(key),
        Cell::Reference(_) => quote!(::factomic::MapKey::Ref(key.into())),
    }
}

fn map_key_mutation(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(::factomic::MapKey::from(key)),
        Cell::Reference(_) => quote!(::factomic::MapKey::Ref(key.into())),
    }
}

fn mutation_value_input(cell: &Cell, iterator: bool) -> TokenStream {
    let item = mutation_item_type(cell, iterator);
    if iterator {
        quote!(impl ::core::iter::Iterator<Item = #item>)
    } else {
        item
    }
}

fn mutation_map_value_input(cell: &Cell) -> TokenStream {
    mutation_item_type(cell, true)
}

fn mutation_item_type(cell: &Cell, string_lifetime: bool) -> TokenStream {
    match cell {
        _ if is_string(cell) && string_lifetime => quote!(&'__factomic ::core::primitive::str),
        _ if is_string(cell) => quote!(&::core::primitive::str),
        Cell::Primitive { ty, .. } => quote!(#ty),
        Cell::Reference(_) => quote!(impl ::core::convert::Into<::factomic::EntityId>),
    }
}

fn is_string(cell: &Cell) -> bool {
    matches!(
        cell,
        Cell::Primitive {
            kind: PrimitiveKind::String,
            ..
        }
    )
}

fn mutation_value_conversion(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(let __factomic_value = value;),
        Cell::Reference(_) => quote!(let __factomic_value: ::factomic::EntityId = value.into();),
    }
}

fn mutation_values_conversion(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(let __factomic_values = values;),
        Cell::Reference(_) => quote! {
            let __factomic_values = values.map(|__factomic_value| {
                let __factomic_value: ::factomic::EntityId = __factomic_value.into();
                __factomic_value
            });
        },
    }
}

fn mutation_map_key_conversion(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(__factomic_key),
        Cell::Reference(_) => quote!(::factomic::MapKey::Ref(__factomic_key.into())),
    }
}

fn mutation_map_value_conversion(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive { .. } => quote!(__factomic_value),
        Cell::Reference(_) => quote!({
            let __factomic_value: ::factomic::EntityId = __factomic_value.into();
            __factomic_value
        }),
    }
}

fn string_lifetime(cell: &Cell) -> TokenStream {
    if is_string(cell) {
        quote!(<'__factomic>)
    } else {
        quote!()
    }
}

fn expand_queries(model: &Model) -> TokenStream {
    let Model {
        vis, ident, fields, ..
    } = model;
    let methods: Vec<_> = fields
        .iter()
        .filter_map(|field| expand_query_field(field, ident))
        .collect();
    if methods.is_empty() {
        return quote!();
    }
    let trait_ident = derived_ident("", ident, "Query");
    let (declarations, implementations): (Vec<_>, Vec<_>) = methods.into_iter().unzip();
    quote! {
        #vis trait #trait_ident {
            #(#declarations)*
        }

        impl #trait_ident for ::factomic::DbQuery<#ident> {
            #(#implementations)*
        }
    }
}

fn expand_query_field(field: &Field, model_ident: &Ident) -> Option<(TokenStream, TokenStream)> {
    let mut methods = Vec::new();
    if field.indexed {
        methods.push(expand_indexed_finder(field, model_ident));
    }
    if is_reference_value(field) {
        methods.push(expand_reverse_finder(field, model_ident));
    }
    if methods.is_empty() {
        return None;
    }
    let (declarations, implementations): (Vec<_>, Vec<_>) = methods.into_iter().unzip();
    Some((quote!(#(#declarations)*), quote!(#(#implementations)*)))
}

fn expand_indexed_finder(field: &Field, model_ident: &Ident) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    let method_ident = derived_ident("find_by_", ident, "");
    let cell = query_value_cell(field);
    let input = query_value_input(cell);
    let raw = raw_type(cell);
    let conversion = query_value_conversion(cell);
    (
        quote! {
            fn #method_ident(
                &self,
                value: #input,
            ) -> ::factomic::Result<::std::vec::Vec<::factomic::DbHandle<#model_ident>>>;
        },
        quote! {
            fn #method_ident(
                &self,
                value: #input,
            ) -> ::factomic::Result<::std::vec::Vec<::factomic::DbHandle<#model_ident>>> {
                #conversion
                let __factomic_entities = self.db().entities_with_value::<#raw>(
                    self.schema().#ident,
                    &__factomic_value,
                )?;
                ::core::result::Result::Ok(self.handles(__factomic_entities))
            }
        },
    )
}

fn expand_reverse_finder(field: &Field, model_ident: &Ident) -> (TokenStream, TokenStream) {
    let ident = &field.ident;
    let method_ident = derived_ident("find_referrers_by_", ident, "");
    (
        quote! {
            fn #method_ident(
                &self,
                target: impl ::core::convert::Into<::factomic::EntityId>,
            ) -> ::factomic::Result<::std::vec::Vec<::factomic::DbHandle<#model_ident>>>;
        },
        quote! {
            fn #method_ident(
                &self,
                target: impl ::core::convert::Into<::factomic::EntityId>,
            ) -> ::factomic::Result<::std::vec::Vec<::factomic::DbHandle<#model_ident>>> {
                let __factomic_target: ::factomic::EntityId = target.into();
                let __factomic_entities = self.db().referring_entities(
                    __factomic_target,
                    self.schema().#ident,
                )?;
                ::core::result::Result::Ok(self.handles(__factomic_entities))
            }
        },
    )
}

fn query_value_cell(field: &Field) -> &Cell {
    match &field.kind {
        FieldKind::One { cell, .. } | FieldKind::List(cell) | FieldKind::Many(cell) => cell,
        FieldKind::Map { value, .. } => value,
    }
}

fn query_value_input(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive {
            kind: PrimitiveKind::String,
            ..
        } => quote!(&::core::primitive::str),
        Cell::Primitive { ty, .. } => quote!(#ty),
        Cell::Reference(_) => quote!(impl ::core::convert::Into<::factomic::EntityId>),
    }
}

fn query_value_conversion(cell: &Cell) -> TokenStream {
    match cell {
        Cell::Primitive {
            kind: PrimitiveKind::String,
            ..
        } => quote!(let __factomic_value = ::std::string::String::from(value);),
        Cell::Primitive { .. } => quote!(let __factomic_value = value;),
        Cell::Reference(_) => {
            quote!(let __factomic_value: ::factomic::EntityId = value.into();)
        }
    }
}

fn is_reference_value(field: &Field) -> bool {
    let cell = query_value_cell(field);
    matches!(
        cell,
        Cell::Reference(_)
            | Cell::Primitive {
                kind: PrimitiveKind::EntityId,
                ..
            }
    )
}

fn derived_ident(prefix: &str, ident: &Ident, suffix: &str) -> Ident {
    Ident::new(&format!("{prefix}{}{suffix}", ident.unraw()), ident.span())
}

fn replace_crate_path(tokens: TokenStream, replacement: &TokenStream) -> TokenStream {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut output = TokenStream::new();
    let mut index = 0;
    while index < tokens.len() {
        if index + 2 < tokens.len()
            && is_colon(&tokens[index])
            && is_colon(&tokens[index + 1])
            && matches!(&tokens[index + 2], TokenTree::Ident(ident) if ident == "factomic")
        {
            output.extend(replacement.clone());
            index += 3;
            continue;
        }
        match &tokens[index] {
            TokenTree::Group(group) => {
                let mut replacement_group = Group::new(
                    group.delimiter(),
                    replace_crate_path(group.stream(), replacement),
                );
                replacement_group.set_span(group.span());
                output.extend([TokenTree::Group(replacement_group)]);
            }
            token => output.extend([token.clone()]),
        }
        index += 1;
    }
    output
}

fn is_colon(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == ':')
}
