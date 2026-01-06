//! Function entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::{JavaScript, TypeScript};
use crate::define_handler;

use super::common::{arrow_function_metadata, function_metadata};

// JavaScript handlers
define_handler!(JavaScript, handle_function_declaration_impl, "function", Function, metadata: function_metadata);
define_handler!(JavaScript, handle_function_expression_impl, "function", Function, metadata: function_metadata);
define_handler!(JavaScript, handle_arrow_function_impl, "function", Function, metadata: arrow_function_metadata);

// TypeScript handlers
define_handler!(TypeScript, handle_ts_function_declaration_impl, "function", Function, metadata: function_metadata);
define_handler!(TypeScript, handle_ts_function_expression_impl, "function", Function, metadata: function_metadata);
define_handler!(TypeScript, handle_ts_arrow_function_impl, "function", Function, metadata: arrow_function_metadata);
