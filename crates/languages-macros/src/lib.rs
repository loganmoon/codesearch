//! Proc macros for codesearch entity extraction
//!
//! This crate provides the `#[entity_handler]` attribute macro for registering
//! entity extraction handlers with automatic capture injection.

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Attribute, FnArg, Ident, ItemFn, Lit, Meta, Pat, PatType, Token, Type,
};

/// Arguments for the entity_handler attribute
///
/// Supports two forms:
/// - `#[entity_handler(entity_type = Function, capture = "func")]`
/// - `#[entity_handler(entity_type = Function, capture = "func", language = "rust")]`
struct EntityHandlerArgs {
    entity_type: Ident,
    capture: String,
    language: Option<String>,
}

impl Parse for EntityHandlerArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut entity_type = None;
        let mut capture = None;
        let mut language = None;

        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;

        for meta in args {
            match meta {
                Meta::NameValue(nv) => {
                    let name = nv
                        .path
                        .get_ident()
                        .ok_or_else(|| syn::Error::new_spanned(&nv.path, "expected identifier"))?
                        .to_string();

                    match name.as_str() {
                        "entity_type" => {
                            if let syn::Expr::Path(p) = &nv.value {
                                entity_type = p.path.get_ident().cloned();
                            } else {
                                return Err(syn::Error::new_spanned(
                                    &nv.value,
                                    "expected entity type identifier",
                                ));
                            }
                        }
                        "capture" => {
                            if let syn::Expr::Lit(syn::ExprLit {
                                lit: Lit::Str(s), ..
                            }) = &nv.value
                            {
                                capture = Some(s.value());
                            } else {
                                return Err(syn::Error::new_spanned(
                                    &nv.value,
                                    "expected string literal for capture",
                                ));
                            }
                        }
                        "language" => {
                            if let syn::Expr::Lit(syn::ExprLit {
                                lit: Lit::Str(s), ..
                            }) = &nv.value
                            {
                                language = Some(s.value());
                            } else {
                                return Err(syn::Error::new_spanned(
                                    &nv.value,
                                    "expected string literal for language",
                                ));
                            }
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &nv.path,
                                format!("unknown attribute: {name}"),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(meta, "expected name = value"));
                }
            }
        }

        let entity_type =
            entity_type.ok_or_else(|| input.error("missing required attribute: entity_type"))?;
        let capture = capture.ok_or_else(|| input.error("missing required attribute: capture"))?;

        Ok(EntityHandlerArgs {
            entity_type,
            capture,
            language,
        })
    }
}

/// Information about a capture parameter
struct CaptureParam {
    name: String,
    param_name: Ident,
    is_optional: bool,
    is_node: bool,
}

/// Parse #[capture] or #[capture(name = "...")] attribute
fn parse_capture_attr(attr: &Attribute) -> syn::Result<Option<String>> {
    if !attr.path().is_ident("capture") {
        return Ok(None);
    }

    // Check if it's #[capture] (no args) or #[capture(...)]
    match &attr.meta {
        Meta::Path(_) => Ok(Some(String::new())), // No custom name, use param name
        Meta::List(list) => {
            // Parse #[capture(name = "...")]
            let nested: Meta = syn::parse2(list.tokens.clone())?;
            if let Meta::NameValue(nv) = nested {
                if nv.path.is_ident("name") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                    {
                        return Ok(Some(s.value()));
                    }
                }
            }
            Err(syn::Error::new_spanned(
                list,
                "expected #[capture] or #[capture(name = \"...\")]",
            ))
        }
        _ => Err(syn::Error::new_spanned(
            attr,
            "expected #[capture] or #[capture(name = \"...\")]",
        )),
    }
}

/// Check if a type is Option<T>
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

/// Check if type contains "Node"
fn is_node_type(ty: &Type) -> bool {
    ty.to_token_stream().to_string().contains("Node")
}

/// Extract capture parameters from function signature
fn extract_capture_params(func: &ItemFn) -> syn::Result<Vec<CaptureParam>> {
    let mut captures = Vec::new();

    for arg in &func.sig.inputs {
        if let FnArg::Typed(PatType { attrs, pat, ty, .. }) = arg {
            // Check for #[capture] attribute
            for attr in attrs {
                if let Some(custom_name) = parse_capture_attr(attr)? {
                    let param_name = if let Pat::Ident(pat_ident) = pat.as_ref() {
                        pat_ident.ident.clone()
                    } else {
                        return Err(syn::Error::new_spanned(
                            pat,
                            "capture parameter must be a simple identifier",
                        ));
                    };

                    let name = if custom_name.is_empty() {
                        param_name.to_string()
                    } else {
                        custom_name
                    };

                    captures.push(CaptureParam {
                        name,
                        param_name,
                        is_optional: is_option_type(ty),
                        is_node: is_node_type(ty),
                    });
                }
            }
        }
    }

    Ok(captures)
}

/// Generate capture extraction code
fn generate_capture_extraction(captures: &[CaptureParam]) -> proc_macro2::TokenStream {
    let extractions: Vec<_> = captures
        .iter()
        .map(|cap| {
            let param_name = &cap.param_name;
            let capture_name = &cap.name;

            match (cap.is_optional, cap.is_node) {
                // Optional node: Option<Node>
                (true, true) => {
                    quote! {
                        let #param_name = __ctx.capture_node_opt(#capture_name);
                    }
                }
                // Required node: Node
                (false, true) => {
                    quote! {
                        let #param_name = __ctx.capture_node(#capture_name)?;
                    }
                }
                // Optional text: Option<&str>
                (true, false) => {
                    quote! {
                        let #param_name = __ctx.capture_text_opt(#capture_name);
                    }
                }
                // Required text: &str
                (false, false) => {
                    quote! {
                        let #param_name = __ctx.capture_text(#capture_name)?;
                    }
                }
            }
        })
        .collect();

    quote! {
        #(#extractions)*
    }
}

/// Entity handler attribute macro
///
/// This macro registers an entity extraction handler with the handler registry.
/// It supports automatic capture injection via the `#[capture]` parameter attribute.
///
/// # Arguments
///
/// - `entity_type` - The EntityType variant this handler produces (e.g., Function, Struct)
/// - `capture` - The primary capture name from the query (e.g., "func")
/// - `language` (optional) - The language this handler applies to (defaults to "rust")
///
/// # Example
///
/// ```ignore
/// #[entity_handler(entity_type = Function, capture = "func", language = "rust")]
/// fn free_function(
///     #[capture] name: &str,
///     #[capture] params: Option<Node>,
///     ctx: &ExtractContext,
/// ) -> Result<Option<CodeEntity>> {
///     // Handler implementation
/// }
/// ```
///
/// The macro generates:
/// 1. A wrapper function that extracts captures from the context
/// 2. An inventory::submit! registration for the handler
#[proc_macro_attribute]
pub fn entity_handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as EntityHandlerArgs);
    let input_fn = parse_macro_input!(item as ItemFn);

    match expand_entity_handler(args, input_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_entity_handler(
    args: EntityHandlerArgs,
    mut input_fn: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let entity_type = &args.entity_type;
    let capture = &args.capture;
    let language = args.language.as_deref().unwrap_or("rust");

    // Extract capture parameters
    let captures = extract_capture_params(&input_fn)?;

    // Generate capture extraction code
    let capture_extraction = generate_capture_extraction(&captures);

    // Generate call arguments (just the capture param names)
    let call_args: Vec<_> = captures.iter().map(|c| &c.param_name).collect();

    // Find the context parameter name
    let ctx_param = input_fn
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(PatType { pat, ty, attrs, .. }) = arg {
                // Skip capture-attributed params
                if attrs.iter().any(|a| a.path().is_ident("capture")) {
                    return None;
                }
                // Check if type looks like &ExtractContext
                let ty_str = ty.to_token_stream().to_string();
                if ty_str.contains("ExtractContext") {
                    if let Pat::Ident(pat_ident) = pat.as_ref() {
                        return Some(pat_ident.ident.clone());
                    }
                }
                None
            } else {
                None
            }
        })
        .next();

    // Remove #[capture] attributes from the original function
    for arg in &mut input_fn.sig.inputs {
        if let FnArg::Typed(PatType { attrs, .. }) = arg {
            attrs.retain(|a| !a.path().is_ident("capture"));
        }
    }

    // Handler name for registry
    let handler_name = format!("{language}::{fn_name}");

    // Generate wrapper function name
    let wrapper_name = Ident::new(&format!("__{fn_name}_wrapper"), fn_name.span());

    // Generate the impl function name
    let impl_name = Ident::new(&format!("__{fn_name}_impl"), fn_name.span());

    // Rename original function to impl version
    let mut impl_fn = input_fn.clone();
    impl_fn.sig.ident = impl_name.clone();

    // Generate the context call if there's a ctx parameter
    let ctx_call = if ctx_param.is_some() {
        quote! { , __ctx }
    } else {
        quote! {}
    };

    // Note: We use paths that require the user to have the following imports:
    // - use codesearch_languages::extract_context::ExtractContext;
    // - use codesearch_languages::handler_registry::HandlerRegistration;
    // - use codesearch_core::entities::EntityType;
    // - use codesearch_core::error::Result;
    // - use codesearch_core::CodeEntity;
    //
    // For use within codesearch_languages itself, use crate:: paths in a module.

    let output = quote! {
        // Original function (renamed to _impl)
        #impl_fn

        // Wrapper function for the registry
        #fn_vis fn #wrapper_name<'a>(
            __ctx: &ExtractContext<'a>
        ) -> Result<Option<CodeEntity>> {
            #capture_extraction
            #impl_name(#(#call_args),* #ctx_call)
        }

        // Register with inventory
        inventory::submit! {
            HandlerRegistration {
                name: #handler_name,
                language: #language,
                entity_type: EntityType::#entity_type,
                primary_capture: #capture,
                handler: #wrapper_name,
            }
        }
    };

    Ok(output)
}
