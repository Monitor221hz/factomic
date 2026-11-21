use std::collections::HashSet;

use syn::{
    Attribute, Data, DeriveInput, Error, Fields, GenericArgument, Ident, LitStr, PathArguments,
    Result, Type, TypePath, Visibility, ext::IdentExt,
};

pub struct Model {
    pub vis: Visibility,
    pub ident: Ident,
    pub fields: Vec<Field>,
}

pub struct Field {
    pub ident: Ident,
    pub name: String,
    pub indexed: bool,
    pub component: bool,
    pub kind: FieldKind,
}

pub enum FieldKind {
    One {
        cell: Cell,
        optional: bool,
    },
    List(Cell),
    Many(Cell),
    Map {
        key: Cell,
        value: Box<Cell>,
        optional_values: bool,
    },
}

pub enum Cell {
    Primitive { ty: Type, kind: PrimitiveKind },
    Reference(Type),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveKind {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    F64,
    String,
    EntityId,
    Timestamp,
    Custom,
}

#[derive(Default)]
struct Attributes {
    namespace: Option<String>,
    name: Option<String>,
    indexed: bool,
    component: bool,
    value: bool,
    reference: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StandardType {
    Option,
    Vec,
    HashSet,
    HashMap,
    Arc,
    String,
    EntityId,
    Timestamp,
}

impl Model {
    pub fn parse(input: DeriveInput) -> Result<Self> {
        let mut errors = None;
        let mut attributes = Attributes::default();
        for attr in &input.attrs {
            if let Err(error) = parse_attributes(attr, false, &mut attributes) {
                combine_error(&mut errors, error);
            }
        }

        let ident = input.ident;
        let vis = input.vis;
        if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
            combine_error(
                &mut errors,
                Error::new_spanned(&input.generics, "generic entities are not supported"),
            );
        }

        let named_fields = match input.data {
            Data::Struct(data) => match data.fields {
                Fields::Named(fields) => fields.named,
                _ => {
                    combine_error(
                        &mut errors,
                        Error::new_spanned(&ident, "Entity requires a struct with named fields"),
                    );
                    Default::default()
                }
            },
            _ => {
                combine_error(
                    &mut errors,
                    Error::new_spanned(&ident, "Entity can only be derived for structs"),
                );
                Default::default()
            }
        };

        let namespace = attributes.namespace.unwrap_or_else(|| {
            heck::ToSnakeCase::to_snake_case(ident.unraw().to_string().as_str())
        });
        let mut fields = Vec::new();
        let mut attribute_names = HashSet::new();
        for input_field in named_fields {
            let ident = input_field.ident.expect("named field");
            let mut field_attributes = Attributes::default();
            for attr in &input_field.attrs {
                if let Err(error) = parse_attributes(attr, true, &mut field_attributes) {
                    combine_error(&mut errors, error);
                }
            }
            if field_attributes.value && field_attributes.reference {
                combine_error(
                    &mut errors,
                    Error::new_spanned(
                        &ident,
                        "#[factomic(value)] and #[factomic(ref)] cannot be combined",
                    ),
                );
            }
            match parse_field_kind(
                &input_field.ty,
                field_attributes.value,
                field_attributes.reference,
            ) {
                Ok(kind) => {
                    if field_attributes.component && !kind.is_reference_valued() {
                        combine_error(
                            &mut errors,
                            Error::new_spanned(
                                &input_field.ty,
                                "component fields must contain entity references",
                            ),
                        );
                    }
                    let name = field_attributes
                        .name
                        .unwrap_or_else(|| format!("{namespace}/{}", ident.unraw()));
                    if !attribute_names.insert(name.clone()) {
                        combine_error(
                            &mut errors,
                            Error::new_spanned(
                                &ident,
                                format!("duplicate effective attribute name `{name}`"),
                            ),
                        );
                    }
                    fields.push(Field {
                        ident,
                        name,
                        indexed: field_attributes.indexed,
                        component: field_attributes.component,
                        kind,
                    });
                }
                Err(error) => combine_error(&mut errors, error),
            }
        }
        validate_method_names(&fields, &mut errors);

        if let Some(errors) = errors {
            return Err(errors);
        }
        Ok(Self { vis, ident, fields })
    }
}

impl FieldKind {
    fn is_reference_valued(&self) -> bool {
        match self {
            Self::One { cell, .. } | Self::List(cell) | Self::Many(cell) => cell.is_reference(),
            Self::Map { value, .. } => value.is_reference(),
        }
    }
}

impl Cell {
    fn is_reference(&self) -> bool {
        matches!(
            self,
            Self::Reference(_)
                | Self::Primitive {
                    kind: PrimitiveKind::EntityId,
                    ..
                }
        )
    }
}

fn parse_attributes(attr: &Attribute, field: bool, output: &mut Attributes) -> Result<()> {
    if !attr.path().is_ident("factomic") {
        return Ok(());
    }
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("namespace") && !field {
            if output.namespace.is_some() {
                return Err(meta.error("duplicate namespace attribute"));
            }
            output.namespace = Some(parse_string_value(&meta)?);
            return Ok(());
        }
        if meta.path.is_ident("name") && field {
            if output.name.is_some() {
                return Err(meta.error("duplicate name attribute"));
            }
            output.name = Some(parse_string_value(&meta)?);
            return Ok(());
        }
        if meta.path.is_ident("indexed") && field {
            if output.indexed {
                return Err(meta.error("duplicate indexed attribute"));
            }
            output.indexed = true;
            return Ok(());
        }
        if meta.path.is_ident("component") && field {
            if output.component {
                return Err(meta.error("duplicate component attribute"));
            }
            output.component = true;
            return Ok(());
        }
        if meta.path.is_ident("value") && field {
            if output.value {
                return Err(meta.error("duplicate value attribute"));
            }
            output.value = true;
            return Ok(());
        }
        if meta.path.is_ident("ref") && field {
            if output.reference {
                return Err(meta.error("duplicate ref attribute"));
            }
            output.reference = true;
            return Ok(());
        }
        Err(meta.error("unsupported factomic attribute"))
    })
}

fn parse_string_value(meta: &syn::meta::ParseNestedMeta<'_>) -> Result<String> {
    let value = meta.value()?;
    Ok(value.parse::<LitStr>()?.value())
}

fn parse_field_kind(ty: &Type, value: bool, reference: bool) -> Result<FieldKind> {
    let (_, arguments) = type_name_and_arguments(ty)?;
    match standard_type(ty) {
        Some(StandardType::Vec) => {
            let inner = one_argument(ty, arguments)?;
            if primitive_kind(inner) == Some(PrimitiveKind::U8) {
                return Err(Error::new_spanned(
                    ty,
                    "Vec<u8> is ambiguous; bytes and lists of bytes are not currently supported",
                ));
            }
            Ok(FieldKind::List(parse_cell(inner, value, reference)?))
        }
        Some(StandardType::HashSet) => Ok(FieldKind::Many(parse_cell(
            one_argument(ty, arguments)?,
            value,
            reference,
        )?)),
        Some(StandardType::HashMap) => {
            let (key, value_ty) = two_arguments(ty, arguments)?;
            let key = parse_cell(key, value, reference)?;
            if !matches!(
                key,
                Cell::Primitive {
                    kind: PrimitiveKind::String
                        | PrimitiveKind::I64
                        | PrimitiveKind::EntityId
                        | PrimitiveKind::Custom,
                    ..
                } | Cell::Reference(_)
            ) {
                return Err(Error::new_spanned(
                    key.ty(),
                    "map keys must be String, i64, EntityId, Arc<T>, or an entity type",
                ));
            }
            let (value_cell, optional_values) = parse_optional_cell(value_ty, value, reference)?;
            Ok(FieldKind::Map {
                key,
                value: Box::new(value_cell),
                optional_values,
            })
        }
        Some(StandardType::Option) => {
            let inner = one_argument(ty, arguments)?;
            match standard_type(inner) {
                Some(StandardType::Option) => Err(Error::new_spanned(
                    ty,
                    "nested Option fields are not supported",
                )),
                Some(StandardType::Vec | StandardType::HashSet | StandardType::HashMap) => Err(
                    Error::new_spanned(ty, "optional collections are only supported as map values"),
                ),
                _ => Ok(FieldKind::One {
                    cell: parse_cell(inner, value, reference)?,
                    optional: true,
                }),
            }
        }
        _ => Ok(FieldKind::One {
            cell: parse_cell(ty, value, reference)?,
            optional: false,
        }),
    }
}

fn parse_optional_cell(ty: &Type, value: bool, reference: bool) -> Result<(Cell, bool)> {
    if standard_type(ty) != Some(StandardType::Option) {
        return Ok((parse_cell(ty, value, reference)?, false));
    }
    let (_, arguments) = type_name_and_arguments(ty)?;
    let inner = one_argument(ty, arguments)?;
    match standard_type(inner) {
        Some(StandardType::Option) => Err(Error::new_spanned(
            ty,
            "nested Option fields are not supported",
        )),
        Some(StandardType::Vec | StandardType::HashSet | StandardType::HashMap) => Err(
            Error::new_spanned(ty, "nested optional collections are not supported"),
        ),
        _ => Ok((parse_cell(inner, value, reference)?, true)),
    }
}

fn parse_cell(ty: &Type, value: bool, reference: bool) -> Result<Cell> {
    let (name, arguments) = type_name_and_arguments(ty)?;
    if standard_type(ty) == Some(StandardType::Arc) && !value {
        let inner = one_argument(ty, arguments)?;
        let (inner_name, inner_arguments) = type_name_and_arguments(inner)?;
        if !inner_arguments.is_empty() {
            return Err(Error::new_spanned(
                inner,
                format!("generic reference type `{inner_name}` is not supported"),
            ));
        }
        return Ok(Cell::Reference(inner.clone()));
    }
    if value && standard_type(ty) == Some(StandardType::Arc) {
        return Err(Error::new_spanned(
            ty,
            "#[factomic(value)] cannot be applied to Arc references",
        ));
    }
    if !arguments.is_empty() {
        let message = if value {
            "container aliases and other generic value types are not supported".to_string()
        } else {
            format!("unsupported generic field type `{name}`")
        };
        return Err(Error::new_spanned(ty, message));
    }
    if reference {
        return Ok(Cell::Reference(ty.clone()));
    }
    if value {
        return Ok(Cell::Primitive {
            ty: ty.clone(),
            kind: PrimitiveKind::Custom,
        });
    }
    let Some(kind) = primitive_kind(ty) else {
        return Ok(Cell::Reference(ty.clone()));
    };
    Ok(Cell::Primitive {
        ty: ty.clone(),
        kind,
    })
}

fn primitive_kind(ty: &Type) -> Option<PrimitiveKind> {
    let path = type_path(ty).ok()?;
    let segments: Vec<_> = path.path.segments.iter().collect();
    let name = segments.last()?.ident.to_string();
    let primitive_path = segments.len() == 1
        || (segments.len() == 3
            && matches!(segments[0].ident.to_string().as_str(), "core" | "std")
            && segments[1].ident == "primitive");
    if primitive_path {
        match name.as_str() {
            "bool" => return Some(PrimitiveKind::Bool),
            "i8" => return Some(PrimitiveKind::I8),
            "i16" => return Some(PrimitiveKind::I16),
            "i32" => return Some(PrimitiveKind::I32),
            "i64" => return Some(PrimitiveKind::I64),
            "u8" => return Some(PrimitiveKind::U8),
            "u16" => return Some(PrimitiveKind::U16),
            "u32" => return Some(PrimitiveKind::U32),
            "f64" => return Some(PrimitiveKind::F64),
            _ => {}
        }
    }
    match standard_type(ty) {
        Some(StandardType::String) => Some(PrimitiveKind::String),
        Some(StandardType::EntityId) => Some(PrimitiveKind::EntityId),
        Some(StandardType::Timestamp) => Some(PrimitiveKind::Timestamp),
        _ => None,
    }
}

fn type_path(ty: &Type) -> Result<&TypePath> {
    let Type::Path(type_path) = ty else {
        return Err(Error::new_spanned(ty, "unsupported field type"));
    };
    if type_path.qself.is_some() {
        return Err(Error::new_spanned(
            ty,
            "qualified field types are not supported",
        ));
    }
    Ok(type_path)
}

fn standard_type(ty: &Type) -> Option<StandardType> {
    let path = type_path(ty).ok()?;
    let segments: Vec<_> = path.path.segments.iter().collect();
    let name = segments.last()?.ident.to_string();
    match segments.as_slice() {
        [_] => match name.as_str() {
            "Option" => Some(StandardType::Option),
            "Vec" => Some(StandardType::Vec),
            "HashSet" => Some(StandardType::HashSet),
            "HashMap" => Some(StandardType::HashMap),
            "Arc" => Some(StandardType::Arc),
            "String" => Some(StandardType::String),
            "EntityId" => Some(StandardType::EntityId),
            "Timestamp" => Some(StandardType::Timestamp),
            _ => None,
        },
        [root, module, _]
            if (root.ident == "std" || root.ident == "core")
                && module.ident == "option"
                && name == "Option" =>
        {
            Some(StandardType::Option)
        }
        [root, module, _]
            if (root.ident == "std" || root.ident == "alloc")
                && module.ident == "vec"
                && name == "Vec" =>
        {
            Some(StandardType::Vec)
        }
        [root, module, _]
            if root.ident == "std" && module.ident == "collections" && name == "HashSet" =>
        {
            Some(StandardType::HashSet)
        }
        [root, module, _]
            if root.ident == "std" && module.ident == "collections" && name == "HashMap" =>
        {
            Some(StandardType::HashMap)
        }
        [root, module, _]
            if (root.ident == "std" || root.ident == "alloc")
                && module.ident == "sync"
                && name == "Arc" =>
        {
            Some(StandardType::Arc)
        }
        [root, module, _]
            if (root.ident == "std" || root.ident == "alloc")
                && module.ident == "string"
                && name == "String" =>
        {
            Some(StandardType::String)
        }
        [root, _] if root.ident == "factomic" && name == "EntityId" => Some(StandardType::EntityId),
        [root, _] if root.ident == "jiff" && name == "Timestamp" => Some(StandardType::Timestamp),
        _ => None,
    }
}

fn type_name_and_arguments(ty: &Type) -> Result<(String, &PathArguments)> {
    let type_path = type_path(ty)?;
    let Some(segment) = type_path.path.segments.last() else {
        return Err(Error::new_spanned(ty, "unsupported field type"));
    };
    Ok((segment.ident.to_string(), &segment.arguments))
}

fn one_argument<'a>(ty: &Type, arguments: &'a PathArguments) -> Result<&'a Type> {
    let PathArguments::AngleBracketed(arguments) = arguments else {
        return Err(Error::new_spanned(ty, "expected one type argument"));
    };
    if arguments.args.len() != 1 {
        return Err(Error::new_spanned(ty, "expected one type argument"));
    }
    match arguments.args.first().expect("checked length") {
        GenericArgument::Type(inner) => Ok(inner),
        other => Err(Error::new_spanned(other, "expected a type argument")),
    }
}

fn two_arguments<'a>(ty: &Type, arguments: &'a PathArguments) -> Result<(&'a Type, &'a Type)> {
    let PathArguments::AngleBracketed(arguments) = arguments else {
        return Err(Error::new_spanned(ty, "expected two type arguments"));
    };
    if arguments.args.len() != 2 {
        return Err(Error::new_spanned(ty, "expected two type arguments"));
    }
    let mut arguments = arguments.args.iter();
    let key = match arguments.next().expect("checked length") {
        GenericArgument::Type(inner) => inner,
        other => return Err(Error::new_spanned(other, "expected a type argument")),
    };
    let value = match arguments.next().expect("checked length") {
        GenericArgument::Type(inner) => inner,
        other => return Err(Error::new_spanned(other, "expected a type argument")),
    };
    Ok((key, value))
}

fn validate_method_names(fields: &[Field], errors: &mut Option<Error>) {
    let mut read = HashSet::new();
    let mut mutation = HashSet::new();
    for field in fields {
        let ident = field.ident.unraw().to_string();
        let mut read_methods = vec![ident.clone()];
        let mut mutation_methods = vec![format!("set_{ident}")];
        match &field.kind {
            FieldKind::One { optional, .. } => {
                if *optional {
                    mutation_methods.push(format!("clear_{ident}"));
                }
            }
            FieldKind::List(_) => {
                read_methods.push(format!("{ident}_at"));
                mutation_methods.extend([
                    format!("set_{ident}_at"),
                    format!("push_{ident}"),
                    format!("prepend_{ident}"),
                    format!("insert_{ident}_at"),
                    format!("remove_{ident}_at"),
                    format!("move_{ident}_at"),
                    format!("rebalance_{ident}"),
                ]);
            }
            FieldKind::Many(_) => {
                mutation_methods.extend([format!("add_{ident}"), format!("remove_{ident}")]);
            }
            FieldKind::Map {
                optional_values, ..
            } => {
                read_methods.push(format!("{ident}_seek"));
                mutation_methods
                    .extend([format!("set_{ident}_seek"), format!("remove_{ident}_key")]);
                if *optional_values {
                    mutation_methods.push(format!("clear_{ident}_seek"));
                }
            }
        }
        for method in read_methods {
            if matches!(
                method.as_str(),
                "exists" | "read" | "read_at" | "read_at_time" | "transact" | "weak_from_pointee"
            ) {
                combine_error(
                    errors,
                    Error::new_spanned(
                        &field.ident,
                        format!(
                            "generated field accessor `{method}` collides with an inherent handle method"
                        ),
                    ),
                );
            }
            if !read.insert(method.clone()) {
                combine_error(
                    errors,
                    Error::new_spanned(
                        &field.ident,
                        format!("duplicate generated read method `{method}`"),
                    ),
                );
            }
        }
        for method in mutation_methods {
            if !mutation.insert(method.clone()) {
                combine_error(
                    errors,
                    Error::new_spanned(
                        &field.ident,
                        format!("duplicate generated mutation method `{method}`"),
                    ),
                );
            }
        }
    }
}

fn combine_error(errors: &mut Option<Error>, error: Error) {
    if let Some(errors) = errors {
        errors.combine(error);
    } else {
        *errors = Some(error);
    }
}

impl Cell {
    pub fn ty(&self) -> &Type {
        match self {
            Self::Primitive { ty, .. } => ty,
            Self::Reference(ty) => ty,
        }
    }
}
