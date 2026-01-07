//! Handler for extracting JavaScript/TypeScript module definitions
//!
//! Each JS/TS file is treated as its own module, establishing the containment
//! hierarchy for entities defined within the file.

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::derive_module_name_from_ctx;

// JavaScript module handler
define_handler!(JavaScript, handle_module_impl, "program",
    module_name_fn: derive_module_name_from_ctx);

// TypeScript and TSX module handlers
define_ts_family_handler!(handle_ts_module_impl, handle_tsx_module_impl, "program",
    module_name_fn: derive_module_name_from_ctx);
