//! LSP Client for spawning and communicating with language servers

use anyhow::{anyhow, Context, Result};
use lsp_types::{
    ClientCapabilities, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, InitializeParams, InitializeResult, Location, Position,
    ReferenceContext, ReferenceParams, SymbolInformation, TextDocumentIdentifier,
    TextDocumentPositionParams, Uri, WorkspaceFolder,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tracing::{debug, trace};

/// Supported language servers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspServer {
    TypeScript,
    Python,
    Rust,
}

impl LspServer {
    /// Get the command to spawn this language server
    ///
    /// For TypeScript, tries npx as fallback if global command not found
    pub fn command(&self) -> Vec<String> {
        match self {
            LspServer::TypeScript => {
                // Check if typescript-language-server is available globally
                if std::process::Command::new("typescript-language-server")
                    .arg("--version")
                    .output()
                    .is_ok()
                {
                    vec!["typescript-language-server".into(), "--stdio".into()]
                } else {
                    // Fallback to npx
                    vec![
                        "npx".into(),
                        "typescript-language-server".into(),
                        "--stdio".into(),
                    ]
                }
            }
            LspServer::Python => vec!["pyright-langserver".into(), "--stdio".into()],
            LspServer::Rust => vec!["rust-analyzer".into()],
        }
    }

    /// Get initialization options for this server
    pub fn init_options(&self) -> serde_json::Value {
        match self {
            LspServer::TypeScript => serde_json::json!({
                "preferences": {
                    "includeInlayHints": false,
                }
            }),
            LspServer::Python => serde_json::json!({}),
            LspServer::Rust => serde_json::json!({
                "cargo": { "buildScripts": { "enable": false } },
                "procMacro": { "enable": false },
            }),
        }
    }
}

/// JSON-RPC request structure
#[derive(Debug, Serialize)]
struct JsonRpcRequest<T: Serialize> {
    jsonrpc: &'static str,
    id: i64,
    method: &'static str,
    params: T,
}

/// JSON-RPC notification structure (no id)
#[derive(Debug, Serialize)]
struct JsonRpcNotification<T: Serialize> {
    jsonrpc: &'static str,
    method: &'static str,
    params: T,
}

/// JSON-RPC response structure
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    id: i64,
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Convert a file path to an LSP URI
fn path_to_uri(path: &Path) -> Result<Uri> {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // Build file:// URI manually
    let path_str = abs_path.to_string_lossy();
    let uri_str = if cfg!(windows) {
        format!("file:///{}", path_str.replace('\\', "/"))
    } else {
        format!("file://{path_str}")
    };

    uri_str.parse().map_err(|e| anyhow!("Invalid URI: {e}"))
}

/// Convert an LSP URI to a file path
pub fn uri_to_path(uri: &Uri) -> Result<PathBuf> {
    let uri_str = uri.as_str();
    if !uri_str.starts_with("file://") {
        return Err(anyhow!("Not a file URI: {uri_str}"));
    }

    let path_str = if cfg!(windows) {
        // file:///C:/path -> C:/path
        uri_str.strip_prefix("file:///").unwrap_or(uri_str)
    } else {
        // file:///path -> /path
        uri_str.strip_prefix("file://").unwrap_or(uri_str)
    };

    // URL-decode the path
    let decoded = percent_decode_str(path_str)?;
    Ok(PathBuf::from(decoded))
}

/// Percent-decode a string (handle URL encoding like %20 for space)
fn percent_decode_str(s: &str) -> Result<String> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (
                char::from(bytes[i + 1]).to_digit(16),
                char::from(bytes[i + 2]).to_digit(16),
            ) {
                result.push((h1 * 16 + h2) as u8);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(result).map_err(|e| anyhow!("Invalid UTF-8 in path: {e}"))
}

/// Client for communicating with a Language Server Protocol server
pub struct LspClient {
    process: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    request_id: i64,
    opened_files: HashSet<String>, // Store URI strings for comparison
}

impl LspClient {
    /// Spawn a new LSP client for the given server and workspace
    pub fn spawn(server: LspServer, workspace_root: &Path) -> Result<Self> {
        let cmd = server.command();
        let init_options = server.init_options();

        debug!("Spawning LSP server: {:?}", cmd);

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn LSP server: {:?}", cmd))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;

        let mut client = Self {
            process: child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            request_id: 0,
            opened_files: HashSet::new(),
        };

        client.initialize(workspace_root, init_options)?;
        Ok(client)
    }

    /// Initialize the LSP server
    fn initialize(&mut self, workspace_root: &Path, init_options: serde_json::Value) -> Result<()> {
        let root_uri = path_to_uri(workspace_root)?;

        let params = InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri,
                name: workspace_root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("workspace")
                    .to_string(),
            }]),
            capabilities: Self::client_capabilities(),
            initialization_options: Some(init_options),
            ..Default::default()
        };

        let _response: InitializeResult = self.send_request("initialize", params)?;
        debug!("LSP server initialized");

        // Send initialized notification
        self.send_notification("initialized", serde_json::json!({}))?;

        Ok(())
    }

    /// Get minimal client capabilities
    fn client_capabilities() -> ClientCapabilities {
        ClientCapabilities {
            text_document: Some(lsp_types::TextDocumentClientCapabilities {
                definition: Some(lsp_types::GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(true),
                }),
                references: Some(lsp_types::DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                document_symbol: Some(lsp_types::DocumentSymbolClientCapabilities {
                    dynamic_registration: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Send a request and wait for response
    fn send_request<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &'static str,
        params: P,
    ) -> Result<R> {
        self.request_id += 1;
        let id = self.request_id;

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        let body = serde_json::to_string(&request)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        trace!("Sending request: {}", body);
        self.stdin.write_all(message.as_bytes())?;
        self.stdin.flush()?;

        // Read response
        let response: JsonRpcResponse<R> = self.read_response(id)?;

        if let Some(error) = response.error {
            return Err(anyhow!("LSP error {}: {}", error.code, error.message));
        }

        response
            .result
            .ok_or_else(|| anyhow!("No result in response"))
    }

    /// Send a notification (no response expected)
    fn send_notification<P: Serialize>(&mut self, method: &'static str, params: P) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0",
            method,
            params,
        };

        let body = serde_json::to_string(&notification)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        trace!("Sending notification: {}", body);
        self.stdin.write_all(message.as_bytes())?;
        self.stdin.flush()?;

        Ok(())
    }

    /// Read a response with the given ID
    fn read_response<R: DeserializeOwned>(
        &mut self,
        expected_id: i64,
    ) -> Result<JsonRpcResponse<R>> {
        loop {
            // Read headers
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                self.stdout.read_line(&mut header)?;
                let header = header.trim();

                if header.is_empty() {
                    break;
                }

                if let Some(len) = header.strip_prefix("Content-Length: ") {
                    content_length = len.parse()?;
                }
            }

            if content_length == 0 {
                return Err(anyhow!("No Content-Length in response"));
            }

            // Read body
            let mut body = vec![0u8; content_length];
            self.stdout.read_exact(&mut body)?;
            let body_str = String::from_utf8(body)?;

            trace!("Received: {}", body_str);

            // Try to parse as our expected response type
            // Skip notifications/other messages
            if let Ok(response) = serde_json::from_str::<JsonRpcResponse<R>>(&body_str) {
                if response.id == expected_id {
                    return Ok(response);
                }
            }
            // If parsing failed or ID doesn't match, keep reading
        }
    }

    /// Open a file in the LSP server
    pub fn open_file(&mut self, file_path: &Path) -> Result<()> {
        let uri = path_to_uri(file_path)?;
        let uri_str = uri.as_str().to_string();

        if self.opened_files.contains(&uri_str) {
            return Ok(());
        }

        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {:?}", file_path))?;

        let params = lsp_types::DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem {
                uri: uri.clone(),
                language_id: self.detect_language(file_path),
                version: 1,
                text: content,
            },
        };

        self.send_notification("textDocument/didOpen", params)?;
        self.opened_files.insert(uri_str);

        Ok(())
    }

    /// Detect language ID from file extension
    fn detect_language(&self, file_path: &Path) -> String {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext {
                "ts" | "tsx" => "typescript",
                "js" | "jsx" => "javascript",
                "py" => "python",
                "rs" => "rust",
                _ => "plaintext",
            })
            .unwrap_or("plaintext")
            .to_string()
    }

    /// Go to definition at the given location
    ///
    /// Returns the definition location(s), or an empty vec if not found.
    pub fn goto_definition(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        // Ensure file is open
        self.open_file(file_path)?;

        let uri = path_to_uri(file_path)?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let response: Option<GotoDefinitionResponse> =
            self.send_request("textDocument/definition", params)?;

        match response {
            Some(GotoDefinitionResponse::Scalar(loc)) => Ok(vec![loc]),
            Some(GotoDefinitionResponse::Array(locs)) => Ok(locs),
            Some(GotoDefinitionResponse::Link(links)) => Ok(links
                .into_iter()
                .map(|l| Location {
                    uri: l.target_uri,
                    range: l.target_selection_range,
                })
                .collect()),
            None => Ok(vec![]),
        }
    }

    /// Find all references to the symbol at the given location
    ///
    /// Returns all locations that reference the symbol, or an empty vec if none found.
    pub fn find_references(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        // Ensure file is open
        self.open_file(file_path)?;

        let uri = path_to_uri(file_path)?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };

        let response: Option<Vec<Location>> =
            self.send_request("textDocument/references", params)?;

        Ok(response.unwrap_or_default())
    }

    /// Get all symbols in a document
    ///
    /// Returns a flat list of symbols (functions, classes, variables, etc.)
    pub fn get_document_symbols(&mut self, file_path: &Path) -> Result<Vec<SymbolInformation>> {
        // Ensure file is open
        self.open_file(file_path)?;

        let uri = path_to_uri(file_path)?;

        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let response: Option<DocumentSymbolResponse> =
            self.send_request("textDocument/documentSymbol", params)?;

        match response {
            Some(DocumentSymbolResponse::Flat(symbols)) => Ok(symbols),
            Some(DocumentSymbolResponse::Nested(nested)) => {
                // Flatten nested symbols, passing the file URI
                Ok(Self::flatten_nested_symbols(nested, &uri))
            }
            None => Ok(vec![]),
        }
    }

    /// Flatten nested document symbols into a flat list
    fn flatten_nested_symbols(
        nested: Vec<lsp_types::DocumentSymbol>,
        file_uri: &Uri,
    ) -> Vec<SymbolInformation> {
        let mut result = Vec::new();
        for symbol in nested {
            // Convert DocumentSymbol to SymbolInformation
            #[allow(deprecated)] // deprecated field is deprecated but required for struct init
            result.push(SymbolInformation {
                name: symbol.name.clone(),
                kind: symbol.kind,
                tags: symbol.tags.clone(),
                deprecated: None, // Don't use deprecated field
                location: Location {
                    uri: file_uri.clone(),
                    range: symbol.selection_range,
                },
                container_name: None,
            });

            // Recursively flatten children
            if let Some(children) = symbol.children {
                result.extend(Self::flatten_nested_symbols(children, file_uri));
            }
        }
        result
    }

    /// Shutdown the LSP server gracefully
    pub fn shutdown(mut self) -> Result<()> {
        // Send shutdown request
        let _: Option<serde_json::Value> =
            self.send_request("shutdown", serde_json::json!(null))?;

        // Send exit notification
        self.send_notification("exit", serde_json::json!(null))?;

        // Wait for process to exit
        self.process.wait()?;

        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Try to kill the process if it's still running
        let _ = self.process.kill();
    }
}
