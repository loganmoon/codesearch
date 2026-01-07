//! Class entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::{derive_class_expression_name, extract_extends_relationships};

// JavaScript handlers
define_handler!(JavaScript, handle_class_declaration_impl, "class", Class,
    relationships: extract_extends_relationships);
define_handler!(JavaScript, handle_class_expression_impl, "class", Class,
    name_ctx_fn: derive_class_expression_name,
    relationships: extract_extends_relationships);

// TypeScript and TSX handlers
define_ts_family_handler!(handle_ts_class_declaration_impl, handle_tsx_class_declaration_impl, "class", Class,
    relationships: extract_extends_relationships);
define_ts_family_handler!(handle_ts_class_expression_impl, handle_tsx_class_expression_impl, "class", Class,
    name_ctx_fn: derive_class_expression_name,
    relationships: extract_extends_relationships);
