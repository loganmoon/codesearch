//! Property entity handlers for JavaScript and TypeScript

use crate::common::js_ts_shared::JavaScript;
use crate::{define_handler, define_ts_family_handler};

use super::common::property_metadata;

// JavaScript handler
define_handler!(JavaScript, handle_property_impl, "property", Property,
    metadata: property_metadata);

// TypeScript and TSX handlers
define_ts_family_handler!(handle_ts_property_impl, handle_tsx_property_impl, "property", Property,
    metadata: property_metadata);
