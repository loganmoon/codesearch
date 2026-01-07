//! Function entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::{arrow_function_metadata, derive_function_expression_name, function_metadata};

// JavaScript handlers
define_handler!(JavaScript, handle_function_declaration_impl, "function", Function,
    metadata: function_metadata);
define_handler!(JavaScript, handle_arrow_function_impl, "function", Function,
    metadata: arrow_function_metadata);
define_handler!(JavaScript, handle_function_expression_impl, "function", Function,
    name_ctx_fn: derive_function_expression_name,
    metadata: function_metadata);

// TypeScript and TSX handlers
define_ts_family_handler!(handle_ts_function_declaration_impl, handle_tsx_function_declaration_impl, "function", Function,
    metadata: function_metadata);
define_ts_family_handler!(handle_ts_arrow_function_impl, handle_tsx_arrow_function_impl, "function", Function,
    metadata: arrow_function_metadata);
define_ts_family_handler!(handle_ts_function_expression_impl, handle_tsx_function_expression_impl, "function", Function,
    name_ctx_fn: derive_function_expression_name,
    metadata: function_metadata);
