//! Property entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::define_handler;

use super::common::property_metadata;

define_handler!(JavaScript, handle_property_impl, "property", Property, metadata: property_metadata);
