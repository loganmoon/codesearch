//! Method entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::method_metadata;

// JavaScript handler
define_handler!(JavaScript, handle_method_impl, "method", Method,
    metadata: method_metadata);

// TypeScript and TSX handlers
define_ts_family_handler!(handle_ts_method_impl, handle_tsx_method_impl, "method", Method,
    metadata: method_metadata);
