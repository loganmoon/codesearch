//! Variable and constant entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::{JavaScript, TypeScript};
use crate::define_handler;

use super::common::const_metadata;

// JavaScript handlers
define_handler!(JavaScript, handle_const_impl, "const", Constant, metadata: const_metadata);
define_handler!(JavaScript, handle_let_impl, "let", Variable);
define_handler!(JavaScript, handle_var_impl, "var", Variable);

// TypeScript handlers
define_handler!(TypeScript, handle_ts_const_impl, "const", Constant, metadata: const_metadata);
define_handler!(TypeScript, handle_ts_let_impl, "let", Variable);
define_handler!(TypeScript, handle_ts_var_impl, "var", Variable);
