//! Property entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::{JavaScript, TypeScript};
use crate::define_handler;

use super::common::property_metadata;

// JavaScript handler
define_handler!(JavaScript, handle_property_impl, "property", Property, metadata: property_metadata);

// TypeScript handler
define_handler!(TypeScript, handle_ts_property_impl, "property", Property, metadata: property_metadata);
