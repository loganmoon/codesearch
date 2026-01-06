//! Class entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::define_handler;

use super::common::extract_extends_relationships;

define_handler!(JavaScript, handle_class_declaration_impl, "class", Class, relationships: extract_extends_relationships);
define_handler!(JavaScript, handle_class_expression_impl, "class", Class, relationships: extract_extends_relationships);
