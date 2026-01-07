//! Class entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::{JavaScript, TypeScript};
use crate::define_handler;

use super::common::{derive_class_expression_name, extract_extends_relationships};

// JavaScript handlers
define_handler!(JavaScript, handle_class_declaration_impl, "class", Class, relationships: extract_extends_relationships);
define_handler!(JavaScript, handle_class_expression_impl, "class", Class,
    name_ctx_fn: derive_class_expression_name,
    relationships: extract_extends_relationships);

// TypeScript handlers
define_handler!(TypeScript, handle_ts_class_declaration_impl, "class", Class, relationships: extract_extends_relationships);
define_handler!(TypeScript, handle_ts_class_expression_impl, "class", Class,
    name_ctx_fn: derive_class_expression_name,
    relationships: extract_extends_relationships);
