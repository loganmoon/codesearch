//! Method entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::define_handler;

use super::common::method_metadata;

define_handler!(JavaScript, handle_method_impl, "method", Method, metadata: method_metadata);
