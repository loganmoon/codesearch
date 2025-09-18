use thiserror::Error;

/// Result type for codesearch operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for codesearch operations
#[derive(Error, Debug)]
pub enum Error {
    /// I/O related errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Parsing errors when processing source code
    #[error("Parse error in {file}: {message}")]
    Parse { file: String, message: String },

    /// Entity extraction errors
    #[error("Entity extraction error: {0}")]
    EntityExtraction(String),

    /// Embedding generation errors
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// Storage related errors
    #[error("Storage error: {0}")]
    Storage(String),

    /// HelixDB installation errors
    #[error("Installation error: {0}")]
    Installation(String),

    /// HelixDB initialization errors
    #[error("Initialization error: {0}")]
    Initialization(String),

    /// HelixDB schema validation errors
    #[error("Schema validation error: {0}")]
    SchemaValidation(String),

    /// File watching errors
    #[error("Watcher error: {0}")]
    Watcher(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Feature not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Process management errors
    #[error("Process management error: {0}")]
    ProcessManagement(String),

    /// Generic error with context
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Any other error
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// Creates a configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Creates a parse error
    pub fn parse(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Parse {
            file: file.into(),
            message: message.into(),
        }
    }

    /// Creates an entity extraction error
    pub fn entity_extraction(msg: impl Into<String>) -> Self {
        Self::EntityExtraction(msg.into())
    }

    /// Creates an embedding error
    pub fn embedding(msg: impl Into<String>) -> Self {
        Self::Embedding(msg.into())
    }

    /// Creates a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Creates a watcher error
    pub fn watcher(msg: impl Into<String>) -> Self {
        Self::Watcher(msg.into())
    }

    /// Creates an invalid input error
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Creates a process management error
    pub fn process_management(msg: impl Into<String>) -> Self {
        Self::ProcessManagement(msg.into())
    }

    /// Adds context to any error
    pub fn with_context<E>(context: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::WithContext {
            context: context.into(),
            source: Box::new(source),
        }
    }
}

/// Extension trait for adding context to Results
pub trait ResultExt<T> {
    /// Add context to an error
    fn context(self, context: impl Into<String>) -> Result<T>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|e| Error::with_context(context, e))
    }
}
