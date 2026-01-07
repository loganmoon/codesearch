//! Variable and constant entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::const_metadata;

// JavaScript handlers
define_handler!(JavaScript, handle_const_impl, "const", Constant,
    metadata: const_metadata);
define_handler!(JavaScript, handle_let_impl, "let", Variable);
define_handler!(JavaScript, handle_var_impl, "var", Variable);

// TypeScript and TSX handlers
define_ts_family_handler!(handle_ts_const_impl, handle_tsx_const_impl, "const", Constant,
    metadata: const_metadata);
define_ts_family_handler!(handle_ts_let_impl, handle_tsx_let_impl, "let", Variable);
define_ts_family_handler!(handle_ts_var_impl, handle_tsx_var_impl, "var", Variable);
