//! Default values and functions for configuration

// Default constants
pub(crate) const DEFAULT_DEVICE: &str = "cpu";
pub(crate) const DEFAULT_PROVIDER: &str = "jina";
pub(crate) const DEFAULT_MODEL: &str = "jina-embeddings-v3";
pub(crate) const DEFAULT_API_BASE_URL: &str = "http://localhost:8000/v1";
pub(crate) const DEFAULT_BGE_INSTRUCTION: &str = "Represent this code search query for retrieving semantically similar code snippets, function implementations, type definitions, and code patterns";
pub(crate) const DEFAULT_QDRANT_HOST: &str = "localhost";
pub(crate) const DEFAULT_POSTGRES_HOST: &str = "localhost";
pub(crate) const DEFAULT_POSTGRES_DATABASE: &str = "codesearch";
pub(crate) const DEFAULT_POSTGRES_USER: &str = "codesearch";
pub(crate) const DEFAULT_POSTGRES_PASSWORD: &str = "codesearch";

pub(crate) fn default_enabled_languages() -> Vec<String> {
    vec![
        "rust".to_string(),
        // "python".to_string(),
        // "javascript".to_string(),
        // "typescript".to_string(),
        // "go".to_string(),
    ]
}

pub(crate) fn default_texts_per_api_request() -> usize {
    64
}

pub(crate) fn default_device() -> String {
    DEFAULT_DEVICE.to_string()
}

pub(crate) fn default_provider() -> String {
    DEFAULT_PROVIDER.to_string()
}

pub(crate) fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

pub(crate) fn default_api_base_url() -> Option<String> {
    Some(DEFAULT_API_BASE_URL.to_string())
}

pub(crate) fn default_embedding_dimension() -> usize {
    1024
}

pub(crate) fn default_max_concurrent_api_requests() -> usize {
    4 // Reduced from 64 to prevent vLLM OOM
}

pub(crate) fn default_bge_instruction() -> String {
    DEFAULT_BGE_INSTRUCTION.to_string()
}

pub(crate) fn default_embedding_retry_attempts() -> usize {
    5
}

pub(crate) fn default_debounce_ms() -> u64 {
    500
}

pub(crate) fn default_ignore_patterns() -> Vec<String> {
    vec![
        "*.log".to_string(),
        "node_modules".to_string(),
        "target".to_string(),
        ".git".to_string(),
        "*.pyc".to_string(),
        "__pycache__".to_string(),
    ]
}

pub(crate) fn default_main_branch_poll_interval_secs() -> u64 {
    30
}

pub(crate) fn default_qdrant_host() -> String {
    DEFAULT_QDRANT_HOST.to_string()
}

pub(crate) fn default_qdrant_port() -> u16 {
    6334
}

pub(crate) fn default_qdrant_rest_port() -> u16 {
    6333
}

pub(crate) fn default_auto_start_deps() -> bool {
    true
}

pub(crate) fn default_postgres_host() -> String {
    DEFAULT_POSTGRES_HOST.to_string()
}

pub(crate) fn default_postgres_port() -> u16 {
    5432
}

pub(crate) fn default_postgres_database() -> String {
    DEFAULT_POSTGRES_DATABASE.to_string()
}

pub(crate) fn default_postgres_user() -> String {
    DEFAULT_POSTGRES_USER.to_string()
}

pub(crate) fn default_postgres_password() -> String {
    DEFAULT_POSTGRES_PASSWORD.to_string()
}

pub(crate) fn default_postgres_pool_size() -> u32 {
    20 // Increased from SQLx default of 5 for better concurrency
}

pub(crate) fn default_neo4j_host() -> String {
    "localhost".to_string()
}

pub(crate) fn default_neo4j_http_port() -> u16 {
    7474
}

pub(crate) fn default_neo4j_bolt_port() -> u16 {
    7687
}

pub(crate) fn default_neo4j_user() -> String {
    "neo4j".to_string()
}

pub(crate) fn default_neo4j_password() -> String {
    "codesearch".to_string()
}

pub(crate) fn default_entities_per_embedding_batch() -> usize {
    500 // Reduced from 2000 to prevent vLLM OOM
}

pub fn default_max_entities_per_db_operation() -> usize {
    10000
}

pub(crate) fn default_server_port() -> u16 {
    3000
}

pub(crate) fn default_allowed_origins() -> Vec<String> {
    Vec::new() // Empty by default = CORS disabled
}

pub(crate) fn default_files_per_discovery_batch() -> usize {
    50
}

pub(crate) fn default_pipeline_channel_capacity() -> usize {
    20
}

pub(crate) fn default_max_concurrent_file_extractions() -> usize {
    32
}

pub(crate) fn default_max_concurrent_snapshot_updates() -> usize {
    16
}

pub(crate) fn default_enable_reranking() -> bool {
    false
}

pub(crate) fn default_reranking_provider() -> String {
    "jina".to_string()
}

pub(crate) fn default_reranking_model() -> String {
    "jina-reranker-v3".to_string()
}

pub(crate) fn default_reranking_candidates() -> usize {
    100 // Reduced from 350 for Jina rate limits (vLLM can handle more)
}

pub(crate) fn default_reranking_top_k() -> usize {
    10
}

pub(crate) fn default_reranking_timeout_secs() -> u64 {
    15
}

pub(crate) fn default_reranking_max_concurrent_requests() -> usize {
    16
}

pub(crate) fn default_prefetch_multiplier() -> usize {
    5
}

pub(crate) fn default_outbox_poll_interval_ms() -> u64 {
    1000
}

pub(crate) fn default_outbox_entries_per_poll() -> i64 {
    500
}

pub(crate) fn default_outbox_max_retries() -> i32 {
    3
}

pub(crate) fn default_outbox_max_embedding_dim() -> usize {
    100_000
}

pub(crate) fn default_outbox_max_cached_collections() -> usize {
    200
}

pub(crate) fn default_outbox_drain_timeout_secs() -> u64 {
    600 // 10 minutes - sufficient for ~100k entries at 200 entries/sec
}

pub(crate) fn default_sparse_provider() -> String {
    "granite".to_string()
}

pub(crate) fn default_sparse_device() -> String {
    "auto".to_string()
}

pub(crate) fn default_sparse_top_k() -> usize {
    256
}

pub(crate) fn default_sparse_batch_size() -> usize {
    32
}
