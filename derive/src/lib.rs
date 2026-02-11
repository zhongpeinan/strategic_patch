use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, PathArguments, Type, parse_macro_input};

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

    // Gap 1: Parse struct-level serde rename_all
    let rename_all = match parse_serde_rename_all(&input.attrs) {
        Ok(v) => v,
        Err(err) => return err.to_compile_error().into(),
    };

    // Gap 5: Parse struct-level patch(gvk)
    let gvk = match parse_struct_patch_attrs(&input.attrs) {
        Ok(g) => g,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut match_arms = Vec::new();
    let mut has_field_arms = Vec::new();

    for field in fields {
        let field_ident = match field.ident {
            Some(id) => id,
            None => continue,
        };

        // Gap 2: Parse patch attributes (skip, leaf, strategy, merge_key)
        let parsed = match parse_patch_attrs(&field.attrs) {
            Ok(v) => v,
            Err(err) => return err.to_compile_error().into(),
        };
        if parsed.skip {
            continue;
        }

        // Gap 1: Determine the JSON field name using serde rename / rename_all
        let field_name = match parse_serde_field_rename(&field.attrs) {
            Ok(Some(explicit)) => explicit,
            Ok(None) => {
                if let Some(ref strategy) = rename_all {
                    apply_rename_all(&field_ident.to_string(), strategy)
                } else {
                    field_ident.to_string()
                }
            }
            Err(err) => return err.to_compile_error().into(),
        };

        let meta_expr = parsed.meta_expr;
        let (base_ty, _is_vec) = unwrap_container_types(&field.ty);
        let schema_ty = &base_ty;

        // Gap 4: Use leaf flag to override schema expression
        let schema_expr = if parsed.leaf || is_primitive_type(schema_ty) {
            quote! { ::strategic_patch::schema::EmptySchema }
        } else {
            quote! { <#schema_ty as ::strategic_patch::schema::StrategicPatchResource>::schema().clone() }
        };

        match_arms.push(quote! {
            #field_name => Ok((Box::new(#schema_expr), #meta_expr)),
        });
        has_field_arms.push(quote! {
            #field_name => true,
        });
    }

    // Gap 5: Generate gvk() override when present
    let gvk_impl = if let Some(gvk_str) = gvk {
        quote! {
            fn gvk() -> Option<&'static str> {
                Some(#gvk_str)
            }
        }
    } else {
        quote! {}
    };

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

            #gvk_impl
        }

        impl #ident {
            pub fn schema() -> &'static #schema_ident {
                &#schema_static_ident
            }
        }
    };

    expanded.into()
}

// --- Gap 1: serde rename support ---

fn parse_serde_rename_all(attrs: &[syn::Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let mut result = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                result = Some(lit.value());
            }
            Ok(())
        })?;
        if result.is_some() {
            return Ok(result);
        }
    }
    Ok(None)
}

fn parse_serde_field_rename(attrs: &[syn::Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let mut result = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                result = Some(lit.value());
            }
            Ok(())
        })?;
        if result.is_some() {
            return Ok(result);
        }
    }
    Ok(None)
}

fn apply_rename_all(name: &str, strategy: &str) -> String {
    match strategy {
        "camelCase" => to_camel_case(name),
        "PascalCase" => to_pascal_case(name),
        "snake_case" => name.to_string(),
        "SCREAMING_SNAKE_CASE" => name.to_uppercase(),
        "kebab-case" => name.replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => name.to_uppercase().replace('_', "-"),
        _ => name.to_string(),
    }
}

fn to_camel_case(name: &str) -> String {
    let mut result = String::new();
    for (i, part) in name.split('_').enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            result.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.push_str(chars.as_str());
            }
        }
    }
    result
}

fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

// --- Gap 2 + 4: Unified patch attribute parsing ---

struct ParsedPatchAttrs {
    meta_expr: proc_macro2::TokenStream,
    skip: bool,
    leaf: bool,
}

fn parse_patch_attrs(attrs: &[syn::Attribute]) -> Result<ParsedPatchAttrs, syn::Error> {
    let mut strategies: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut merge_key: Option<String> = None;
    let mut skip = false;
    let mut leaf = false;

    for attr in attrs.iter().filter(|a| a.path().is_ident("patch")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                skip = true;
                Ok(())
            } else if meta.path.is_ident("leaf") {
                leaf = true;
                Ok(())
            } else if meta.path.is_ident("strategy") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                for part in lit.value().split(',').map(|s| s.trim()) {
                    match part {
                        "merge" => strategies
                            .push(quote! { ::strategic_patch::schema::PatchStrategy::Merge }),
                        "replace" => strategies
                            .push(quote! { ::strategic_patch::schema::PatchStrategy::Replace }),
                        "retainKeys" => strategies
                            .push(quote! { ::strategic_patch::schema::PatchStrategy::RetainKeys }),
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

    let meta_expr = if strategies.is_empty() && merge_key.is_none() {
        quote! { ::strategic_patch::schema::PatchMeta::default() }
    } else {
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
        quote! {
            ::strategic_patch::schema::PatchMeta {
                strategies: #strategies_expr,
                merge_key: #merge_key_expr,
            }
        }
    };

    Ok(ParsedPatchAttrs {
        meta_expr,
        skip,
        leaf,
    })
}

// --- Gap 5: struct-level patch attributes ---

fn parse_struct_patch_attrs(attrs: &[syn::Attribute]) -> Result<Option<String>, syn::Error> {
    let mut gvk = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("patch")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("gvk") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                gvk = Some(lit.value());
            }
            Ok(())
        })?;
    }
    Ok(gvk)
}

// --- Type helpers ---

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
                        // Gap 3: Map types are leaf types
                        | "BTreeMap"
                        | "HashMap"
                        | "IndexMap"
                        // Gap 4: Common Kubernetes / serde types
                        | "Timestamp"
                        | "MicroTime"
                        | "Quantity"
                        | "IntOrString"
                        | "Value"
                        | "DateTime"
                        | "ResourceList"
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
