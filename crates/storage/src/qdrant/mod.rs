// Private module - not exposed in public API
mod client;
mod collections;
mod operations;
mod search;

pub(crate) use client::QdrantStorage;
