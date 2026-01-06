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
    fqn_config: Option<FqnConfig>,
    entities: Vec<EntityExtractor>,
}

/// FQN (Fully Qualified Name) configuration
struct FqnConfig {
    /// Optional language family for inherited defaults
    /// When specified, separator/relative_prefixes/external_prefixes default to family values
    family: Option<Ident>,
    /// Separator (required if no family, optional if family provided)
    separator: Option<LitStr>,
    module_path_fn: Option<Expr>,
    relative_prefixes: Vec<RelativePrefixEntry>,
    external_prefixes: Vec<LitStr>,
    edge_cases: Option<Expr>,
    /// Whether relative_prefixes was explicitly specified (vs inherited from family)
    has_explicit_relative_prefixes: bool,
    /// Whether external_prefixes was explicitly specified (vs inherited from family)
    has_explicit_external_prefixes: bool,
}

/// Entry for a relative prefix mapping
/// Parses: "crate::" => Root, "super::" => Parent { chainable: true }
struct RelativePrefixEntry {
    prefix: LitStr,
    semantics: Ident, // Root, Current, Parent
    chainable: bool,  // Only for Parent
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
        let mut fqn_config = None;
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
                "fqn" => {
                    let content;
                    braced!(content in input);

                    let mut family = None;
                    let mut separator = None;
                    let mut module_path_fn = None;
                    let mut relative_prefixes = Vec::new();
                    let mut external_prefixes = Vec::new();
                    let mut edge_cases = None;
                    let mut has_explicit_relative_prefixes = false;
                    let mut has_explicit_external_prefixes = false;

                    while !content.is_empty() {
                        let fqn_field: Ident = content.parse()?;
                        content.parse::<Token![:]>()?;

                        match fqn_field.to_string().as_str() {
                            "family" => {
                                // Parse: ModuleBased, CrateBased, or PackageBased
                                family = Some(content.parse::<Ident>()?);
                            }
                            "separator" => {
                                separator = Some(content.parse::<LitStr>()?);
                            }
                            "module_path_fn" => {
                                module_path_fn = Some(content.parse::<Expr>()?);
                            }
                            "relative_prefixes" => {
                                // Parse: { "crate::" => Root, "super::" => Parent { chainable: true } }
                                has_explicit_relative_prefixes = true;
                                let prefixes_content;
                                braced!(prefixes_content in content);

                                while !prefixes_content.is_empty() {
                                    let prefix = prefixes_content.parse::<LitStr>()?;
                                    prefixes_content.parse::<Token![=>]>()?;
                                    let semantics = prefixes_content.parse::<Ident>()?;

                                    // Check for optional { chainable: true }
                                    let chainable = if prefixes_content.peek(syn::token::Brace) {
                                        let opts_content;
                                        braced!(opts_content in prefixes_content);
                                        let mut is_chainable = false;

                                        while !opts_content.is_empty() {
                                            let opt_name: Ident = opts_content.parse()?;
                                            opts_content.parse::<Token![:]>()?;

                                            if opt_name == "chainable" {
                                                let value: syn::LitBool = opts_content.parse()?;
                                                is_chainable = value.value();
                                            }

                                            if !opts_content.is_empty() {
                                                opts_content.parse::<Token![,]>()?;
                                            }
                                        }
                                        is_chainable
                                    } else {
                                        false
                                    };

                                    relative_prefixes.push(RelativePrefixEntry {
                                        prefix,
                                        semantics,
                                        chainable,
                                    });

                                    if !prefixes_content.is_empty() {
                                        prefixes_content.parse::<Token![,]>()?;
                                    }
                                }
                            }
                            "external_prefixes" => {
                                // Parse: ["std", "core", "alloc"]
                                has_explicit_external_prefixes = true;
                                let ext_content;
                                syn::bracketed!(ext_content in content);
                                let ext_list: Punctuated<LitStr, Token![,]> =
                                    Punctuated::parse_terminated(&ext_content)?;
                                external_prefixes = ext_list.into_iter().collect();
                            }
                            "edge_cases" => {
                                // Parse: edge_case_handlers::RUST_EDGE_CASE_HANDLERS
                                edge_cases = Some(content.parse::<Expr>()?);
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    fqn_field.span(),
                                    format!("Unknown fqn field: {fqn_field}"),
                                ))
                            }
                        }

                        if !content.is_empty() {
                            content.parse::<Token![,]>()?;
                        }
                    }

                    // Validate: need either family or explicit separator
                    if family.is_none() && separator.is_none() {
                        return Err(syn::Error::new(
                            input.span(),
                            "Missing 'separator' in fqn block (required when no 'family' specified)",
                        ));
                    }

                    fqn_config = Some(FqnConfig {
                        family,
                        separator,
                        module_path_fn,
                        relative_prefixes,
                        external_prefixes,
                        edge_cases,
                        has_explicit_relative_prefixes,
                        has_explicit_external_prefixes,
                    });

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
            fqn_config,
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
/// - FQN (Fully Qualified Name) configuration (if `fqn:` block provided)
///
/// # Example
///
/// ```ignore
/// // Required: define SCOPE_PATTERNS if using fqn: block
/// const SCOPE_PATTERNS: &[ScopePattern] = &[
///     ScopePattern { node_kind: "mod_item", field_name: "name" },
/// ];
///
/// define_language_extractor! {
///     language: Rust,
///     tree_sitter: tree_sitter_rust::LANGUAGE,
///     extensions: ["rs"],
///
///     fqn: {
///         separator: "::",
///     },
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

    // Generate FQN config constant and scope configuration if fqn block is present
    let fqn_output = if let Some(ref fqn_config) = input.fqn_config {
        let module_path_fn_value = match &fqn_config.module_path_fn {
            Some(expr) => quote! { Some(#expr) },
            None => quote! { None },
        };

        // Generate edge case handlers value
        let edge_case_handlers_value = match &fqn_config.edge_cases {
            Some(expr) => quote! { Some(#expr) },
            None => quote! { None },
        };

        // Determine if using family-based configuration
        if let Some(ref family_ident) = fqn_config.family {
            // Family-based configuration: inherit from family with optional overrides
            let family_name = family_ident.to_string();

            // Validate family name
            let family_path = match family_name.as_str() {
                "ModuleBased" | "CrateBased" | "PackageBased" => {
                    quote! { crate::common::path_config::LanguageFamily::#family_ident }
                }
                unknown => {
                    let msg = format!(
                        "Unknown language family '{}'. Expected one of: ModuleBased, CrateBased, PackageBased",
                        unknown
                    );
                    return syn::Error::new(family_ident.span(), msg)
                        .to_compile_error()
                        .into();
                }
            };

            // Generate separator: explicit override or inherited from family
            let separator_value = match &fqn_config.separator {
                Some(sep) => quote! { #sep },
                None => {
                    quote! { crate::common::path_config::get_family_config(#family_path).separator }
                }
            };

            // Generate relative_prefixes: explicit override or inherited from family
            let relative_prefixes_value = if fqn_config.has_explicit_relative_prefixes {
                let entries: Vec<_> = fqn_config
                    .relative_prefixes
                    .iter()
                    .map(|entry| {
                        let prefix = &entry.prefix;
                        let semantics_name = &entry.semantics;
                        let chainable = entry.chainable;

                        let semantics_str = semantics_name.to_string();
                        let semantics_value = match semantics_str.as_str() {
                            "Root" => quote! { crate::common::path_config::RelativeSemantics::Root },
                            "Current" => quote! { crate::common::path_config::RelativeSemantics::Current },
                            "Parent" => quote! { crate::common::path_config::RelativeSemantics::Parent { levels: 1 } },
                            unknown => {
                                let msg = format!(
                                    "Unknown relative semantics '{}'. Expected one of: Root, Current, Parent",
                                    unknown
                                );
                                return syn::Error::new(semantics_name.span(), msg).to_compile_error();
                            }
                        };

                        quote! {
                            crate::common::path_config::RelativePrefix {
                                prefix: #prefix,
                                semantics: #semantics_value,
                                chainable: #chainable,
                            }
                        }
                    })
                    .collect();
                quote! { &[#(#entries),*] }
            } else {
                quote! { crate::common::path_config::get_family_config(#family_path).relative_prefixes }
            };

            // Generate external_prefixes: explicit override or inherited from family
            let external_prefixes_value = if fqn_config.has_explicit_external_prefixes {
                let prefixes = &fqn_config.external_prefixes;
                quote! { &[#(#prefixes),*] }
            } else {
                quote! { crate::common::path_config::get_family_config(#family_path).external_prefixes }
            };

            // unprefixed_is_external is always inherited from family (no override syntax yet)
            let unprefixed_is_external_value = quote! { crate::common::path_config::get_family_config(#family_path).unprefixed_is_external };

            // For the separator constant and scope config, we need a concrete value
            // Use explicit separator if provided, otherwise use a placeholder that references family
            let separator_for_const = match &fqn_config.separator {
                Some(sep) => quote! { #sep },
                None => {
                    // We need to generate code that references the family's separator
                    // Since this is a const context, we need to use the family config directly
                    quote! { crate::common::path_config::get_family_config(#family_path).separator }
                }
            };

            quote! {
                /// FQN separator for this language (inherited from #family_ident family if not overridden)
                pub const FQN_SEPARATOR: &str = #separator_for_const;

                /// Path configuration for this language
                /// Family: #family_ident (with optional overrides)
                pub const PATH_CONFIG: crate::common::path_config::PathConfig =
                    crate::common::path_config::PathConfig {
                        separator: #separator_value,
                        relative_prefixes: #relative_prefixes_value,
                        external_prefixes: #external_prefixes_value,
                        unprefixed_is_external: #unprefixed_is_external_value,
                    };

                // Register scope configuration with inventory
                inventory::submit! {
                    crate::qualified_name::ScopeConfiguration {
                        language: #language_name_lower,
                        separator: #separator_for_const,
                        patterns: SCOPE_PATTERNS,
                        module_path_fn: #module_path_fn_value,
                        path_config: &PATH_CONFIG,
                        edge_case_handlers: #edge_case_handlers_value,
                    }
                }
            }
        } else {
            // Explicit configuration: no family, all fields must be provided
            let separator = fqn_config
                .separator
                .as_ref()
                .expect("separator required when no family");

            // Generate RelativePrefix entries
            let relative_prefix_entries: Vec<_> = fqn_config
                .relative_prefixes
                .iter()
                .map(|entry| {
                    let prefix = &entry.prefix;
                    let semantics_name = &entry.semantics;
                    let chainable = entry.chainable;

                    let semantics_str = semantics_name.to_string();
                    let semantics_value = match semantics_str.as_str() {
                        "Root" => quote! { crate::common::path_config::RelativeSemantics::Root },
                        "Current" => quote! { crate::common::path_config::RelativeSemantics::Current },
                        "Parent" => quote! { crate::common::path_config::RelativeSemantics::Parent { levels: 1 } },
                        unknown => {
                            let msg = format!(
                                "Unknown relative semantics '{}'. Expected one of: Root, Current, Parent",
                                unknown
                            );
                            return syn::Error::new(semantics_name.span(), msg).to_compile_error();
                        }
                    };

                    quote! {
                        crate::common::path_config::RelativePrefix {
                            prefix: #prefix,
                            semantics: #semantics_value,
                            chainable: #chainable,
                        }
                    }
                })
                .collect();

            let external_prefixes = &fqn_config.external_prefixes;

            quote! {
                /// FQN separator for this language
                pub const FQN_SEPARATOR: &str = #separator;

                /// Path configuration for this language
                pub const PATH_CONFIG: crate::common::path_config::PathConfig =
                    crate::common::path_config::PathConfig {
                        separator: #separator,
                        relative_prefixes: &[
                            #(#relative_prefix_entries),*
                        ],
                        external_prefixes: &[#(#external_prefixes),*],
                        // Default: unprefixed paths are NOT external (crate-based behavior)
                        // Languages using module-based resolution should use family: ModuleBased
                        unprefixed_is_external: false,
                    };

                // Register scope configuration with inventory
                inventory::submit! {
                    crate::qualified_name::ScopeConfiguration {
                        language: #language_name_lower,
                        separator: #separator,
                        patterns: SCOPE_PATTERNS,
                        module_path_fn: #module_path_fn_value,
                        path_config: &PATH_CONFIG,
                        edge_case_handlers: #edge_case_handlers_value,
                    }
                }
            }
        }
    } else {
        quote! {}
    };

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
                pub(crate) fn #handler_name(
                    ctx: &crate::common::entity_building::ExtractionContext,
                ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
                    #handler_impl(ctx)
                }
            }
        })
        .collect();

    // Generate the complete output
    let expanded = quote! {
        #fqn_output

        /// Language extractor for #language_name
        pub struct #extractor_name {
            repository_id: String,
            package_name: Option<String>,
            source_root: Option<std::path::PathBuf>,
            repo_root: std::path::PathBuf,
            config: crate::extraction_framework::LanguageConfiguration,
        }

        impl #extractor_name {
            /// Create a new #language_name extractor
            ///
            /// # Arguments
            /// * `repository_id` - Repository identifier
            /// * `package_name` - Optional package/crate name from manifest
            /// * `source_root` - Optional source root for module path derivation
            /// * `repo_root` - Repository root for deriving repo-relative paths
            pub fn new(
                repository_id: String,
                package_name: Option<String>,
                source_root: Option<std::path::PathBuf>,
                repo_root: std::path::PathBuf,
            ) -> codesearch_core::error::Result<Self> {
                let language = #tree_sitter_lang.into();

                let config = crate::extraction_framework::LanguageConfigurationBuilder::new(language)
                    #(#add_extractor_calls)*
                    .build()?;

                Ok(Self {
                    repository_id,
                    package_name,
                    source_root,
                    repo_root,
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
                    self.package_name.as_deref(),
                    self.source_root.as_deref(),
                    &self.repo_root,
                )?;
                extractor.extract(source, file_path)
            }
        }

        // Register language with inventory
        inventory::submit! {
            crate::LanguageDescriptor {
                name: #language_name_lower,
                extensions: &[#(#extensions),*],
                factory: |repo_id, pkg_name, src_root, repo_root| Ok(Box::new(#extractor_name::new(
                    repo_id.to_string(),
                    pkg_name.map(String::from),
                    src_root.map(std::path::PathBuf::from),
                    repo_root.to_path_buf(),
                )?)),
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
