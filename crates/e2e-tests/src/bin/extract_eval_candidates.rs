//! Entity candidate extraction for semantic search evaluation.
//!
//! This tool queries the entity_metadata table for entities that are suitable
//! for semantic search evaluation ground truth. It filters for entities that:
//! - Are not too small (sufficient semantic content)
//! - Have meaningful types (function, method, struct, trait, impl)
//! - Don't have overly generic names
//!
//! Usage:
//!   cargo run --manifest-path crates/e2e-tests/Cargo.toml --bin extract_eval_candidates -- --repository nushell
//!
//! Or specify the repository name pattern:
//!   cargo run --manifest-path crates/e2e-tests/Cargo.toml --bin extract_eval_candidates -- --repository "nushell"

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::FromRow;
use std::io::Write;

/// Command line arguments
#[derive(Debug)]
struct Args {
    /// Repository name pattern to search for (e.g., "nushell")
    repository: String,
    /// Minimum token count for entities
    min_tokens: i32,
    /// Maximum number of candidates to extract
    limit: i64,
    /// Output format: "json" or "csv"
    format: String,
    /// Randomize candidate selection instead of ordering by size
    randomize: bool,
    /// Seed for random selection (for reproducibility)
    seed: Option<f64>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            repository: String::new(),
            min_tokens: 100,
            limit: 200,
            format: "json".to_string(),
            randomize: false,
            seed: None,
        }
    }
}

fn parse_args() -> Result<Args> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--repository" | "-r" => {
                args.repository = iter
                    .next()
                    .context("--repository requires a value")?;
            }
            "--min-tokens" | "-t" => {
                args.min_tokens = iter
                    .next()
                    .context("--min-tokens requires a value")?
                    .parse()
                    .context("--min-tokens must be a number")?;
            }
            "--limit" | "-l" => {
                args.limit = iter
                    .next()
                    .context("--limit requires a value")?
                    .parse()
                    .context("--limit must be a number")?;
            }
            "--format" | "-f" => {
                args.format = iter
                    .next()
                    .context("--format requires a value")?;
            }
            "--randomize" | "-R" => {
                args.randomize = true;
            }
            "--seed" | "-s" => {
                args.seed = Some(
                    iter.next()
                        .context("--seed requires a value")?
                        .parse()
                        .context("--seed must be a number")?,
                );
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                print_help();
                std::process::exit(1);
            }
        }
    }

    if args.repository.is_empty() {
        eprintln!("Error: --repository is required");
        print_help();
        std::process::exit(1);
    }

    Ok(args)
}

fn print_help() {
    eprintln!(
        r#"
extract_eval_candidates - Extract entity candidates for semantic search evaluation

USAGE:
    extract_eval_candidates --repository <NAME> [OPTIONS]

OPTIONS:
    -r, --repository <NAME>   Repository name pattern (required, e.g., "nushell")
    -t, --min-tokens <N>      Minimum bm25_token_count (default: 100)
    -l, --limit <N>           Maximum candidates to extract (default: 200)
    -f, --format <FMT>        Output format: "json" or "csv" (default: json)
    -R, --randomize           Randomize candidate selection (instead of by size)
    -s, --seed <N>            Seed for random selection (0.0-1.0, for reproducibility)
    -h, --help                Show this help message

EXAMPLES:
    # Extract candidates from nushell repository (ordered by size)
    extract_eval_candidates --repository nushell

    # Extract random candidates for diverse sampling
    extract_eval_candidates --repository nushell --randomize --limit 50

    # Reproducible random extraction
    extract_eval_candidates --repository nushell --randomize --seed 0.42
"#
    );
}

/// Candidate entity for evaluation
#[derive(Debug, Serialize, Deserialize, FromRow)]
struct EntityCandidate {
    entity_id: String,
    qualified_name: String,
    name: String,
    entity_type: String,
    file_path: String,
    bm25_token_count: Option<i32>,
    #[sqlx(default)]
    documentation_summary: Option<String>,
    #[sqlx(default)]
    content_preview: Option<String>,
}

/// Generic names to exclude - these are too common to be useful for evaluation
const GENERIC_NAMES: &[&str] = &[
    "new",
    "default",
    "from",
    "into",
    "clone",
    "drop",
    "fmt",
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    "hash",
    "build",
    "run",
    "call",
    "get",
    "set",
    "len",
    "is_empty",
    "iter",
    "next",
    "deref",
    "as_ref",
    "as_mut",
    "try_from",
    "try_into",
    "serialize",
    "deserialize",
];

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;

    // Connect to database with default credentials
    let database_url = format!(
        "postgresql://{}:{}@{}:{}/{}",
        "codesearch", "codesearch", "localhost", 5432, "codesearch"
    );

    eprintln!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    // Find the repository
    eprintln!("Looking for repository matching '{}'...", args.repository);
    let repo: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT repository_id, repository_name, collection_name
         FROM repositories
         WHERE repository_name ILIKE $1 OR collection_name ILIKE $1
         LIMIT 1",
    )
    .bind(format!("%{}%", args.repository))
    .fetch_optional(&pool)
    .await
    .context("Failed to query repositories")?;

    let (repository_id, repository_name, collection_name) = repo.context(format!(
        "No repository found matching '{}'",
        args.repository
    ))?;

    eprintln!(
        "Found repository: {} (collection: {}, id: {})",
        repository_name, collection_name, repository_id
    );

    // Build the exclusion list for SQL
    let generic_names_list = GENERIC_NAMES
        .iter()
        .map(|s| format!("'{}'", s))
        .collect::<Vec<_>>()
        .join(", ");

    // First, let's see what entity types and counts exist
    eprintln!("Checking entity distribution...");
    let count_query = r#"
        SELECT entity_type, COUNT(*) as cnt
        FROM entity_metadata
        WHERE repository_id = $1 AND deleted_at IS NULL
        GROUP BY entity_type
        ORDER BY cnt DESC
    "#;
    let type_counts: Vec<(String, i64)> = sqlx::query_as(count_query)
        .bind(repository_id)
        .fetch_all(&pool)
        .await
        .context("Failed to query entity types")?;

    eprintln!("Entity type distribution:");
    for (entity_type, count) in &type_counts {
        eprintln!("  {}: {}", entity_type, count);
    }

    // Check how many have token counts
    let token_query = r#"
        SELECT
            SUM(CASE WHEN bm25_token_count IS NULL THEN 1 ELSE 0 END) as null_count,
            SUM(CASE WHEN bm25_token_count IS NOT NULL THEN 1 ELSE 0 END) as non_null_count,
            COALESCE(AVG(bm25_token_count)::float8, 0.0) as avg_tokens
        FROM entity_metadata
        WHERE repository_id = $1 AND deleted_at IS NULL
    "#;
    let (null_count, non_null_count, avg_tokens): (i64, i64, f64) =
        sqlx::query_as(token_query)
            .bind(repository_id)
            .fetch_one(&pool)
            .await
            .context("Failed to query token stats")?;

    eprintln!("\nToken count stats:");
    eprintln!("  Entities with token count: {}", non_null_count);
    eprintln!("  Entities without token count: {}", null_count);
    if avg_tokens > 0.0 {
        eprintln!("  Average tokens: {:.1}", avg_tokens);
    }

    // Query for candidate entities - adjusted to handle missing token counts
    eprintln!("\nQuerying for candidate entities...");

    if args.randomize {
        eprintln!("  Mode: Random sampling{}",
            args.seed.map_or(String::new(), |s| format!(" (seed: {s})")));

        // Set random seed if specified
        if let Some(seed) = args.seed {
            sqlx::query(&format!("SELECT SETSEED({seed})"))
                .execute(&pool)
                .await
                .context("Failed to set random seed")?;
        }
    } else {
        eprintln!("  Mode: Ordered by content size");
    }

    // Build ORDER BY clause based on randomization setting
    let order_clause = if args.randomize {
        "RANDOM()".to_string()
    } else {
        "COALESCE(bm25_token_count, LENGTH(content) / 5) DESC".to_string()
    };

    let query = format!(
        r#"
        SELECT
            entity_id,
            qualified_name,
            name,
            entity_type,
            file_path,
            bm25_token_count,
            entity_data->>'documentation_summary' as documentation_summary,
            LEFT(content, 500) as content_preview
        FROM entity_metadata
        WHERE repository_id = $1
          AND deleted_at IS NULL
          AND entity_type IN ('function', 'method', 'struct', 'trait', 'impl', 'enum', 'Function', 'Method', 'Struct', 'Trait', 'Impl', 'Enum')
          AND (bm25_token_count >= $2 OR (bm25_token_count IS NULL AND LENGTH(content) > 300))
          AND LOWER(name) NOT IN ({})
          AND name !~ '^_'
          AND content IS NOT NULL
        ORDER BY {}
        LIMIT $3
        "#,
        generic_names_list, order_clause
    );

    let candidates: Vec<EntityCandidate> = sqlx::query_as(&query)
        .bind(repository_id)
        .bind(args.min_tokens)
        .bind(args.limit)
        .fetch_all(&pool)
        .await
        .context("Failed to query entities")?;

    eprintln!("Found {} candidate entities", candidates.len());

    // Output results
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    match args.format.as_str() {
        "json" => {
            let output = serde_json::to_string_pretty(&candidates)?;
            writeln!(handle, "{output}")?;
        }
        "csv" => {
            writeln!(
                handle,
                "entity_id,qualified_name,name,entity_type,file_path,bm25_token_count,has_docs"
            )?;
            for c in &candidates {
                writeln!(
                    handle,
                    "{},{},{},{},{},{},{}",
                    c.entity_id,
                    c.qualified_name.replace(',', ";"),
                    c.name,
                    c.entity_type,
                    c.file_path.replace(',', ";"),
                    c.bm25_token_count.unwrap_or(0),
                    c.documentation_summary.is_some()
                )?;
            }
        }
        _ => {
            eprintln!("Unknown format: {}, using json", args.format);
            let output = serde_json::to_string_pretty(&candidates)?;
            writeln!(handle, "{output}")?;
        }
    }

    eprintln!("\nEntity type distribution:");
    let mut type_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in &candidates {
        *type_counts.entry(&c.entity_type).or_default() += 1;
    }
    for (entity_type, count) in type_counts {
        eprintln!("  {}: {}", entity_type, count);
    }

    Ok(())
}
