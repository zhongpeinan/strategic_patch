use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, PathArguments, Type};

#[proc_macro_derive(PatchSchema, attributes(patch))]
pub fn derive_patch_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;
    let schema_ident = format_ident!("{}Schema", ident);
    let schema_static_ident = format_ident!("{}_SCHEMA", ident.to_string().to_uppercase());

    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(named) => named.named,
            _ => {
                return syn::Error::new_spanned(
                    ident,
                    "PatchSchema only supports structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(ident, "PatchSchema only supports structs")
                .to_compile_error()
                .into();
        }
    };

    let mut match_arms = Vec::new();
    let mut has_field_arms = Vec::new();

    for field in fields {
        let field_ident = match field.ident {
            Some(id) => id,
            None => continue,
        };
        let field_name = field_ident.to_string();
        let meta_expr = match build_patch_meta(&field.attrs) {
            Ok(value) => value,
            Err(err) => return err.to_compile_error().into(),
        };
        let (base_ty, is_vec) = unwrap_container_types(&field.ty);
        let schema_ty = if is_vec { &base_ty } else { &base_ty };
        let schema_expr = schema_expr_for_type(schema_ty);

        match_arms.push(quote! {
            #field_name => Ok((Box::new(#schema_expr), #meta_expr)),
        });
        has_field_arms.push(quote! {
            #field_name => true,
        });
    }

    let expanded = quote! {
        #[derive(Clone, Debug)]
        pub struct #schema_ident;

        static #schema_static_ident: #schema_ident = #schema_ident;

        impl ::strategic_patch::schema::LookupPatchMeta for #schema_ident {
            fn lookup_struct_meta(
                &self,
                key: &str,
            ) -> ::strategic_patch::error::Result<(
                Box<dyn ::strategic_patch::schema::LookupPatchMeta>,
                ::strategic_patch::schema::PatchMeta,
            )> {
                match key {
                    #(#match_arms)*
                    _ => Ok((
                        Box::new(::strategic_patch::schema::EmptySchema),
                        ::strategic_patch::schema::PatchMeta::default(),
                    )),
                }
            }

            fn lookup_slice_meta(
                &self,
                key: &str,
            ) -> ::strategic_patch::error::Result<(
                Box<dyn ::strategic_patch::schema::LookupPatchMeta>,
                ::strategic_patch::schema::PatchMeta,
            )> {
                self.lookup_struct_meta(key)
            }

            fn name(&self) -> &str {
                stringify!(#ident)
            }

            fn has_field(&self, key: &str) -> bool {
                match key {
                    #(#has_field_arms)*
                    _ => false,
                }
            }
        }

        impl ::strategic_patch::schema::StrategicPatchResource for #ident {
            type Schema = #schema_ident;

            fn schema() -> &'static Self::Schema {
                &#schema_static_ident
            }
        }

        impl #ident {
            pub fn schema() -> &'static #schema_ident {
                &#schema_static_ident
            }
        }
    };

    expanded.into()
}

fn build_patch_meta(attrs: &[syn::Attribute]) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut strategies: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut merge_key: Option<String> = None;

    for attr in attrs.iter().filter(|a| a.path().is_ident("patch")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("strategy") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                for part in lit.value().split(',').map(|s| s.trim()) {
                    match part {
                        "merge" => strategies
                            .push(quote! { ::strategic_patch::schema::PatchStrategy::Merge }),
                        "replace" => strategies
                            .push(quote! { ::strategic_patch::schema::PatchStrategy::Replace }),
                        "retainKeys" => strategies.push(
                            quote! { ::strategic_patch::schema::PatchStrategy::RetainKeys },
                        ),
                        "" => {}
                        other => {
                            return Err(syn::Error::new_spanned(
                                lit.clone(),
                                format!("unknown patch strategy '{other}'"),
                            ));
                        }
                    }
                }
                Ok(())
            } else if meta.path.is_ident("merge_key") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                merge_key = Some(lit.value());
                Ok(())
            } else {
                Ok(())
            }
        })?;
    }

    if strategies.is_empty() && merge_key.is_none() {
        return Ok(quote! { ::strategic_patch::schema::PatchMeta::default() });
    }

    let strategies_expr = if strategies.is_empty() {
        quote! { Vec::new() }
    } else {
        quote! { vec![#(#strategies),*] }
    };
    let merge_key_expr = if let Some(value) = merge_key {
        quote! { Some(#value.to_string()) }
    } else {
        quote! { None }
    };

    Ok(quote! {
        ::strategic_patch::schema::PatchMeta {
            strategies: #strategies_expr,
            merge_key: #merge_key_expr,
        }
    })
}

fn unwrap_container_types(ty: &Type) -> (Type, bool) {
    if let Type::Path(path) = ty {
        if let Some(seg) = path.path.segments.last() {
            if seg.ident == "Option" {
                if let Some(inner) = extract_angle_type(seg) {
                    return unwrap_container_types(&inner);
                }
            }
            if seg.ident == "Vec" {
                if let Some(inner) = extract_angle_type(seg) {
                    return (inner, true);
                }
            }
        }
    }
    (ty.clone(), false)
}

fn extract_angle_type(segment: &syn::PathSegment) -> Option<Type> {
    match &segment.arguments {
        PathArguments::AngleBracketed(args) => args.args.first().and_then(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn schema_expr_for_type(ty: &Type) -> proc_macro2::TokenStream {
    if is_primitive_type(ty) {
        quote! { ::strategic_patch::schema::EmptySchema }
    } else {
        quote! { <#ty as ::strategic_patch::schema::StrategicPatchResource>::schema().clone() }
    }
}

fn is_primitive_type(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            if let Some(seg) = path.path.segments.last() {
                matches!(
                    seg.ident.to_string().as_str(),
                    "bool"
                        | "i8"
                        | "i16"
                        | "i32"
                        | "i64"
                        | "isize"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "usize"
                        | "f32"
                        | "f64"
                        | "String"
                        | "str"
                )
            } else {
                false
            }
        }
        Type::Reference(r) => matches!(
            r.elem.as_ref(),
            Type::Path(path) if path.path.segments.last().map(|seg| seg.ident == "str").unwrap_or(false)
        ),
        _ => false,
    }
}
