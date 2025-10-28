//! Procedural macros for defining language extractors
//!
//! This crate provides the `define_language_extractor!` macro which generates
//! all the boilerplate code needed to create a new language extractor.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, Ident, LitStr, Result, Token,
};

/// Input structure for the define_language_extractor macro
struct LanguageExtractorInput {
    language_name: Ident,
    tree_sitter_language: Expr,
    extensions: Vec<LitStr>,
    entities: Vec<EntityExtractor>,
}

/// Configuration for a single entity type extractor
struct EntityExtractor {
    entity_name: Ident,
    query: Expr,
    handler: Expr,
}

impl Parse for LanguageExtractorInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut language_name = None;
        let mut tree_sitter_language = None;
        let mut extensions = None;
        let mut entities = None;

        while !input.is_empty() {
            let field_name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "language" => {
                    language_name = Some(input.parse::<Ident>()?);
                    if !input.is_empty() && !input.peek(syn::token::Brace) {
                        input.parse::<Token![,]>()?;
                    }
                }
                "tree_sitter" => {
                    tree_sitter_language = Some(input.parse::<Expr>()?);
                    if !input.is_empty() && !input.peek(syn::token::Brace) {
                        input.parse::<Token![,]>()?;
                    }
                }
                "extensions" => {
                    let content;
                    syn::bracketed!(content in input);
                    let ext_list: Punctuated<LitStr, Token![,]> =
                        Punctuated::parse_terminated(&content)?;
                    extensions = Some(ext_list.into_iter().collect());
                    if !input.is_empty() && !input.peek(syn::token::Brace) {
                        input.parse::<Token![,]>()?;
                    }
                }
                "entities" => {
                    let content;
                    braced!(content in input);
                    let mut entity_list = Vec::new();

                    while !content.is_empty() {
                        let entity_name: Ident = content.parse()?;
                        content.parse::<Token![=>]>()?;

                        let entity_content;
                        braced!(entity_content in content);

                        let mut query = None;
                        let mut handler = None;

                        while !entity_content.is_empty() {
                            let entity_field: Ident = entity_content.parse()?;
                            entity_content.parse::<Token![:]>()?;

                            match entity_field.to_string().as_str() {
                                "query" => {
                                    query = Some(entity_content.parse::<Expr>()?);
                                }
                                "handler" => {
                                    handler = Some(entity_content.parse::<Expr>()?);
                                }
                                _ => {
                                    return Err(syn::Error::new(
                                        entity_field.span(),
                                        format!("Unknown entity field: {entity_field}"),
                                    ))
                                }
                            }

                            if !entity_content.is_empty() {
                                entity_content.parse::<Token![,]>()?;
                            }
                        }

                        entity_list.push(EntityExtractor {
                            entity_name,
                            query: query.ok_or_else(|| {
                                syn::Error::new(input.span(), "Missing 'query' field")
                            })?,
                            handler: handler.ok_or_else(|| {
                                syn::Error::new(input.span(), "Missing 'handler' field")
                            })?,
                        });

                        if !content.is_empty() {
                            content.parse::<Token![,]>()?;
                        }
                    }

                    entities = Some(entity_list);
                }
                _ => {
                    return Err(syn::Error::new(
                        field_name.span(),
                        format!("Unknown field: {field_name}"),
                    ))
                }
            }
        }

        Ok(LanguageExtractorInput {
            language_name: language_name
                .ok_or_else(|| syn::Error::new(input.span(), "Missing 'language' field"))?,
            tree_sitter_language: tree_sitter_language
                .ok_or_else(|| syn::Error::new(input.span(), "Missing 'tree_sitter' field"))?,
            extensions: extensions
                .ok_or_else(|| syn::Error::new(input.span(), "Missing 'extensions' field"))?,
            entities: entities
                .ok_or_else(|| syn::Error::new(input.span(), "Missing 'entities' field"))?,
        })
    }
}

/// Define a language extractor with automatic code generation
///
/// This macro generates:
/// - An extractor struct
/// - A constructor that builds the language configuration
/// - An Extractor trait implementation
/// - Inventory registration for automatic discovery
/// - Handler wrapper functions
///
/// # Example
///
/// ```ignore
/// define_language_extractor! {
///     language: JavaScript,
///     tree_sitter: tree_sitter_javascript::LANGUAGE,
///     extensions: ["js", "jsx"],
///
///     entities: {
///         function => {
///             query: queries::FUNCTION_QUERY,
///             handler: handlers::handle_function_impl,
///         },
///         class => {
///             query: queries::CLASS_QUERY,
///             handler: handlers::handle_class_impl,
///         }
///     }
/// }
/// ```
#[proc_macro]
pub fn define_language_extractor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LanguageExtractorInput);

    let language_name = &input.language_name;
    let language_name_lower = language_name.to_string().to_lowercase();
    let extractor_name = quote::format_ident!("{language_name}Extractor");
    let tree_sitter_lang = &input.tree_sitter_language;
    let extensions = &input.extensions;

    // Generate add_extractor calls for each entity type
    let add_extractor_calls: Vec<_> = input
        .entities
        .iter()
        .map(|entity| {
            // Strip the "r#" prefix if present (for raw identifiers like r#enum)
            let entity_name_str = entity.entity_name.to_string();
            let entity_name_str = entity_name_str
                .strip_prefix("r#")
                .unwrap_or(&entity_name_str);
            let query = &entity.query;
            let handler_name = quote::format_ident!("handle_{}", entity.entity_name);

            quote! {
                .add_extractor(
                    #entity_name_str,
                    #query,
                    Box::new(handlers::#handler_name),
                )
            }
        })
        .collect();

    // Generate handler wrapper functions
    let handler_wrappers: Vec<_> = input
        .entities
        .iter()
        .map(|entity| {
            let handler_name = quote::format_ident!("handle_{}", entity.entity_name);
            let handler_impl = &entity.handler;

            quote! {
                pub fn #handler_name(
                    query_match: &tree_sitter::QueryMatch,
                    query: &tree_sitter::Query,
                    source: &str,
                    file_path: &std::path::Path,
                    repository_id: &str,
                ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
                    #handler_impl(query_match, query, source, file_path, repository_id)
                }
            }
        })
        .collect();

    // Generate the complete output
    let expanded = quote! {
        /// Language extractor for #language_name
        pub struct #extractor_name {
            repository_id: String,
            config: crate::extraction_framework::LanguageConfiguration,
        }

        impl #extractor_name {
            /// Create a new #language_name extractor
            pub fn new(repository_id: String) -> codesearch_core::error::Result<Self> {
                let language = #tree_sitter_lang.into();

                let config = crate::extraction_framework::LanguageConfigurationBuilder::new(language)
                    #(#add_extractor_calls)*
                    .build()?;

                Ok(Self {
                    repository_id,
                    config,
                })
            }
        }

        impl crate::Extractor for #extractor_name {
            fn extract(
                &self,
                source: &str,
                file_path: &std::path::Path,
            ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
                let mut extractor = crate::extraction_framework::GenericExtractor::new(
                    &self.config,
                    self.repository_id.clone(),
                )?;
                extractor.extract(source, file_path)
            }
        }

        // Register language with inventory
        inventory::submit! {
            crate::LanguageDescriptor {
                name: #language_name_lower,
                extensions: &[#(#extensions),*],
                factory: |repo_id| Ok(Box::new(#extractor_name::new(repo_id.to_string())?)),
            }
        }

        // Handler wrapper module
        mod handlers {
            use super::*;

            #(#handler_wrappers)*
        }
    };

    TokenStream::from(expanded)
}
