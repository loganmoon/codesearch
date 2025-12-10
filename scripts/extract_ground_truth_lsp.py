#!/usr/bin/env python3
"""
Extract ground truth for evaluation queries using rust-analyzer LSP.

This script uses rust-analyzer's LSP interface to extract semantically correct
ground truth for structural queries (references, implementations, etc.).

Usage:
    python scripts/extract_ground_truth_lsp.py \
        --repo /path/to/rust-analyzer \
        --queries crates/e2e-tests/fixtures/graph_eval_queries.json \
        --output crates/e2e-tests/fixtures/graph_eval_queries_with_gt.json

Requirements:
    pip install python-lsp-jsonrpc
"""

import argparse
import json
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass
class Position:
    """LSP position (0-indexed line and character)."""
    line: int
    character: int


@dataclass
class Location:
    """LSP location with file URI and range."""
    uri: str
    start: Position
    end: Position


class RustAnalyzerLSP:
    """Client for rust-analyzer LSP server."""

    def __init__(self, repo_path: str, ra_binary: str = "rust-analyzer"):
        self.repo_path = Path(repo_path).resolve()
        self.ra_binary = ra_binary
        self.process: Optional[subprocess.Popen] = None
        self.request_id = 0
        self._responses: dict = {}
        self._lock = threading.Lock()
        self._reader_thread: Optional[threading.Thread] = None

    def start(self) -> None:
        """Start the rust-analyzer LSP server."""
        self.process = subprocess.Popen(
            [self.ra_binary],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(self.repo_path),
        )

        # Start reader thread
        self._reader_thread = threading.Thread(target=self._read_responses, daemon=True)
        self._reader_thread.start()

        # Initialize LSP
        self._initialize()

    def stop(self) -> None:
        """Stop the LSP server."""
        if self.process:
            # Just terminate - shutdown protocol is complex and not needed for our use case
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except Exception:
                self.process.kill()

    def _read_responses(self) -> None:
        """Background thread to read LSP responses."""
        while self.process and self.process.stdout:
            try:
                # Read Content-Length header
                header = b""
                while not header.endswith(b"\r\n\r\n"):
                    byte = self.process.stdout.read(1)
                    if not byte:
                        return
                    header += byte

                # Parse content length
                content_length = 0
                for line in header.decode().split("\r\n"):
                    if line.startswith("Content-Length:"):
                        content_length = int(line.split(":")[1].strip())
                        break

                if content_length == 0:
                    continue

                # Read content
                content = self.process.stdout.read(content_length)
                message = json.loads(content.decode())

                # Store response
                if "id" in message:
                    with self._lock:
                        self._responses[message["id"]] = message

            except Exception as e:
                print(f"Error reading LSP response: {e}", file=sys.stderr)
                break

    def _send_message(self, message: dict) -> None:
        """Send a JSON-RPC message to the server."""
        if not self.process or not self.process.stdin:
            raise RuntimeError("LSP server not running")

        content = json.dumps(message)
        header = f"Content-Length: {len(content)}\r\n\r\n"
        self.process.stdin.write(header.encode() + content.encode())
        self.process.stdin.flush()

    def _send_request(self, method: str, params: dict, timeout: float = 30.0) -> dict:
        """Send a request and wait for response."""
        self.request_id += 1
        request_id = self.request_id

        message = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }

        self._send_message(message)

        # Wait for response
        start = time.time()
        while time.time() - start < timeout:
            with self._lock:
                if request_id in self._responses:
                    response = self._responses.pop(request_id)
                    if "error" in response:
                        raise RuntimeError(f"LSP error: {response['error']}")
                    return response.get("result", {})
            time.sleep(0.01)

        raise TimeoutError(f"LSP request {method} timed out")

    def _send_notification(self, method: str, params: dict) -> None:
        """Send a notification (no response expected)."""
        message = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }
        self._send_message(message)

    def _initialize(self) -> None:
        """Initialize the LSP connection."""
        result = self._send_request("initialize", {
            "processId": None,
            "rootUri": f"file://{self.repo_path}",
            "capabilities": {
                "textDocument": {
                    "references": {"dynamicRegistration": False},
                    "implementation": {"dynamicRegistration": False},
                    "definition": {"dynamicRegistration": False},
                },
            },
        }, timeout=120)

        self._send_notification("initialized", {})
        print(f"LSP initialized: {result.get('serverInfo', {}).get('name', 'unknown')}")

    def find_references(self, file_path: str, line: int, character: int) -> list[Location]:
        """Find all references to the symbol at the given position."""
        uri = f"file://{self.repo_path / file_path}"

        result = self._send_request("textDocument/references", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": False},
        })

        return self._parse_locations(result or [])

    def find_implementations(self, file_path: str, line: int, character: int) -> list[Location]:
        """Find all implementations of the trait/type at the given position."""
        uri = f"file://{self.repo_path / file_path}"

        result = self._send_request("textDocument/implementation", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
        })

        return self._parse_locations(result or [])

    def find_definition(self, file_path: str, line: int, character: int) -> list[Location]:
        """Find the definition of the symbol at the given position."""
        uri = f"file://{self.repo_path / file_path}"

        result = self._send_request("textDocument/definition", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
        })

        # Can be a single location or array
        if isinstance(result, dict):
            result = [result]

        return self._parse_locations(result or [])

    def workspace_symbol(self, query: str) -> list[dict]:
        """Search for symbols matching the query across the workspace."""
        result = self._send_request("workspace/symbol", {
            "query": query,
        }, timeout=60)

        return result or []

    def document_symbols(self, file_path: str) -> list[dict]:
        """Get all symbols in a document."""
        uri = f"file://{self.repo_path / file_path}"

        result = self._send_request("textDocument/documentSymbol", {
            "textDocument": {"uri": uri},
        }, timeout=30)

        return result or []

    def _parse_locations(self, locations: list) -> list[Location]:
        """Parse LSP location responses."""
        parsed = []
        for loc in locations:
            if "uri" not in loc:
                continue
            range_data = loc.get("range", {})
            start = range_data.get("start", {})
            end = range_data.get("end", {})
            parsed.append(Location(
                uri=loc["uri"],
                start=Position(start.get("line", 0), start.get("character", 0)),
                end=Position(end.get("line", 0), end.get("character", 0)),
            ))
        return parsed


def extract_qualified_name_from_location(lsp: RustAnalyzerLSP, location: Location) -> Optional[str]:
    """Extract a qualified name from an LSP location.

    This reads the file and extracts the symbol name at the location,
    then constructs a qualified name based on the file path and context.
    """
    # Convert URI to path
    uri = location.uri
    if uri.startswith("file://"):
        file_path = uri[7:]
    else:
        return None

    # Read the file to get the symbol name
    try:
        with open(file_path, "r") as f:
            lines = f.readlines()

        if location.start.line >= len(lines):
            return None

        line = lines[location.start.line]

        # Extract the identifier at the position
        start = location.start.character
        end = location.end.character

        # Expand to word boundaries
        while start > 0 and (line[start-1].isalnum() or line[start-1] == '_'):
            start -= 1
        while end < len(line) and (line[end].isalnum() or line[end] == '_'):
            end += 1

        symbol_name = line[start:end].strip()

        if not symbol_name:
            return None

        # Construct qualified name from file path
        # e.g., crates/hir/src/lib.rs -> hir
        # e.g., crates/hir-def/src/resolver.rs -> hir_def::resolver
        repo_root = str(lsp.repo_path)
        rel_path = file_path
        if file_path.startswith(repo_root):
            rel_path = file_path[len(repo_root):].lstrip("/")

        # Parse crate structure
        parts = rel_path.split("/")
        if parts[0] == "crates" and len(parts) > 1:
            crate_name = parts[1].replace("-", "_")

            # Get module path
            if len(parts) > 2:
                module_parts = parts[2:]  # After crates/<name>/
                if module_parts[0] == "src":
                    module_parts = module_parts[1:]

                # Remove .rs extension and lib/mod
                if module_parts:
                    last = module_parts[-1]
                    if last.endswith(".rs"):
                        last = last[:-3]
                    if last in ("lib", "mod"):
                        module_parts = module_parts[:-1]
                    else:
                        module_parts[-1] = last

                if module_parts:
                    module_path = "::".join(module_parts)
                    return f"{crate_name}::{module_path}::{symbol_name}"
                else:
                    return f"{crate_name}::{symbol_name}"
            else:
                return f"{crate_name}::{symbol_name}"

        return symbol_name

    except Exception as e:
        print(f"Error extracting qualified name: {e}", file=sys.stderr)
        return None


# Cache for document symbols to avoid repeated LSP calls
_doc_symbol_cache: dict[str, list[dict]] = {}


def find_enclosing_function(lsp: RustAnalyzerLSP, location: Location) -> Optional[str]:
    """Find the function/method that contains the given location.

    For reference queries (e.g., "What functions call X?"), we need to find
    the ENCLOSING function that contains the call, not the called symbol.

    Returns a qualified name like "crate::module::function_name" or
    "crate::module::Type::method_name".
    """
    uri = location.uri
    if not uri.startswith("file://"):
        return None

    file_path = uri[7:]
    repo_root = str(lsp.repo_path)
    rel_path = file_path
    if file_path.startswith(repo_root):
        rel_path = file_path[len(repo_root):].lstrip("/")

    ref_line = location.start.line
    ref_col = location.start.character

    # Get document symbols (with caching)
    if rel_path not in _doc_symbol_cache:
        try:
            symbols = lsp.document_symbols(rel_path)
            _doc_symbol_cache[rel_path] = symbols
        except Exception as e:
            print(f"Error getting document symbols for {rel_path}: {e}", file=sys.stderr)
            return None
    else:
        symbols = _doc_symbol_cache[rel_path]

    # Find the enclosing function
    # For flat SymbolInformation format, we need to find the innermost function
    # For nested DocumentSymbol format, we recurse into children
    def find_containing_function(symbols: list, parent_name: str = "") -> Optional[str]:
        """Recursively find the innermost function containing the location."""
        # For flat lists, find the smallest-range function containing the line
        best_match = None
        best_range_size = float('inf')

        for sym in symbols:
            # Handle both DocumentSymbol format (range at top level)
            # and SymbolInformation format (range inside location)
            range_data = sym.get("range")
            if range_data is None:
                location = sym.get("location", {})
                range_data = location.get("range", {})
            start = range_data.get("start", {}) if range_data else {}
            end = range_data.get("end", {}) if range_data else {}

            start_line = start.get("line", 0)
            end_line = end.get("line", 0)

            # Check if location is within this symbol's range
            if start_line <= ref_line <= end_line:
                name = sym.get("name", "")
                kind = sym.get("kind", 0)
                # SymbolKind: 6=Method, 12=Function
                is_function = kind in [6, 12]

                # For SymbolInformation, use containerName if available
                container = sym.get("containerName", "")
                if container:
                    full_name = f"{container}::{name}"
                elif parent_name:
                    full_name = f"{parent_name}::{name}"
                else:
                    full_name = name

                # Check children first (for DocumentSymbol format)
                children = sym.get("children", [])
                if children:
                    child_result = find_containing_function(children, full_name)
                    if child_result:
                        return child_result

                # For flat lists, find the innermost (smallest range) function
                if is_function:
                    range_size = end_line - start_line
                    if range_size < best_range_size:
                        best_range_size = range_size
                        best_match = full_name

        return best_match

    function_name = find_containing_function(symbols)
    if not function_name:
        return None

    # Construct full qualified name with crate/module path
    parts = rel_path.split("/")
    if parts[0] == "crates" and len(parts) > 1:
        crate_name = parts[1].replace("-", "_")

        if len(parts) > 2:
            module_parts = parts[2:]
            if module_parts[0] == "src":
                module_parts = module_parts[1:]

            if module_parts:
                last = module_parts[-1]
                if last.endswith(".rs"):
                    last = last[:-3]
                if last in ("lib", "mod"):
                    module_parts = module_parts[:-1]
                else:
                    module_parts[-1] = last

            if module_parts:
                module_path = "::".join(module_parts)
                return f"{crate_name}::{module_path}::{function_name}"
            else:
                return f"{crate_name}::{function_name}"
        else:
            return f"{crate_name}::{function_name}"

    return function_name


def process_single_hop_query(
    lsp: RustAnalyzerLSP,
    lsp_query: dict,
) -> list[str]:
    """Process a single-hop LSP query."""
    method = lsp_query.get("method", "")
    target = lsp_query.get("target", "")

    if not target:
        return []

    # Parse target: "path/to/file.rs:line:col"
    parts = target.rsplit(":", 2)
    if len(parts) != 3:
        print(f"Invalid target format: {target}", file=sys.stderr)
        return []

    file_path, line_str, col_str = parts
    try:
        line = int(line_str) - 1  # Convert to 0-indexed
        col = int(col_str) - 1
    except ValueError:
        print(f"Invalid line/col in target: {target}", file=sys.stderr)
        return []

    # Execute LSP method
    locations = []
    if method == "textDocument/references":
        locations = lsp.find_references(file_path, line, col)
    elif method == "textDocument/implementation":
        locations = lsp.find_implementations(file_path, line, col)
    elif method == "textDocument/definition":
        locations = lsp.find_definition(file_path, line, col)
    else:
        print(f"Unknown LSP method: {method}", file=sys.stderr)
        return []

    # Extract qualified names
    # For references queries (call sites), we want the ENCLOSING function,
    # not the symbol at the reference location.
    # For implementations/definitions, we want the symbol itself.
    qualified_names = []
    for loc in locations:
        if method == "textDocument/references":
            # Find the function that contains this reference (the caller)
            qn = find_enclosing_function(lsp, loc)
        else:
            # For implementations/definitions, extract the symbol directly
            qn = extract_qualified_name_from_location(lsp, loc)
        if qn:
            qualified_names.append(qn)

    return list(set(qualified_names))  # Deduplicate


def resolve_qualified_name_to_location(lsp: RustAnalyzerLSP, qualified_name: str) -> Optional[tuple[str, int, int]]:
    """Resolve a qualified name to a file:line:col using workspace/symbol.

    Returns (file_path, line, col) or None if not found.
    """
    # Extract the symbol name (last component)
    parts = qualified_name.split("::")
    symbol_name = parts[-1] if parts else qualified_name

    # Clean up symbol name (remove generic params, etc.)
    symbol_name = symbol_name.split("<")[0].split("(")[0].strip()

    if not symbol_name:
        return None

    # Search for the symbol
    try:
        results = lsp.workspace_symbol(symbol_name)

        for symbol in results:
            location = symbol.get("location", {})
            uri = location.get("uri", "")
            if not uri.startswith("file://"):
                continue

            file_path = uri[7:]  # Remove file://

            # Check if the file path matches expected crate structure
            # Build expected path from qualified name
            if len(parts) > 1:
                # e.g., "hir::has_source::HasSource" -> should be in crates/hir
                crate_name = parts[0].replace("_", "-")
                if f"crates/{crate_name}" not in file_path and f"crates/{parts[0]}" not in file_path:
                    continue

            range_data = location.get("range", {})
            start = range_data.get("start", {})
            line = start.get("line", 0)
            col = start.get("character", 0)

            # Convert to relative path
            repo_root = str(lsp.repo_path)
            if file_path.startswith(repo_root):
                file_path = file_path[len(repo_root):].lstrip("/")

            return (file_path, line, col)

    except Exception as e:
        print(f"Error resolving {qualified_name}: {e}", file=sys.stderr)

    return None


def process_workspace_symbol_query(
    lsp: RustAnalyzerLSP,
    lsp_query: dict,
) -> list[str]:
    """Process a workspace symbol query for module_structure queries."""
    directory = lsp_query.get("directory", "")
    module = lsp_query.get("module", "")

    if not directory and not module:
        return []

    qualified_names = []

    if directory:
        # For directory-based queries, we need to find all symbols defined in that directory
        # Use document symbols for each file in the directory
        import glob

        dir_path = lsp.repo_path / directory
        if dir_path.is_file():
            # Single file
            files = [directory]
        else:
            # Directory - find all .rs files
            pattern = str(dir_path / "**/*.rs")
            files = glob.glob(pattern, recursive=True)
            # Convert to relative paths
            files = [f[len(str(lsp.repo_path))+1:] for f in files]

        for file_path in files[:50]:  # Limit to avoid timeout
            try:
                symbols = lsp.document_symbols(file_path)
                for sym in flatten_document_symbols(symbols):
                    name = sym.get("name", "")
                    kind = sym.get("kind", 0)
                    # Only include types, functions, traits, etc. (not variables)
                    # SymbolKind: 5=Class, 6=Method, 11=Interface, 12=Function, 23=Struct, 10=Enum
                    if kind in [5, 6, 10, 11, 12, 23] and name:
                        # Construct qualified name from file path
                        qn = construct_qualified_name_from_file(lsp, file_path, name)
                        if qn:
                            qualified_names.append(qn)
            except Exception as e:
                print(f"Error getting symbols from {file_path}: {e}", file=sys.stderr)

    elif module:
        # For module-based queries, search workspace symbols
        # e.g., "syntax::ast" -> search for symbols in that module
        try:
            parts = module.split("::")
            search_term = parts[-1] if parts else module
            results = lsp.workspace_symbol(search_term)

            for sym in results:
                name = sym.get("name", "")
                container = sym.get("containerName", "")
                location = sym.get("location", {})
                uri = location.get("uri", "")

                # Check if symbol is in the expected module
                if container and module in container.lower():
                    qn = f"{container}::{name}" if container else name
                    qualified_names.append(qn)
                elif uri:
                    file_path = uri[7:] if uri.startswith("file://") else uri
                    # Check if file path matches module
                    expected_path = module.replace("::", "/").replace("_", "-")
                    if expected_path in file_path:
                        qn = construct_qualified_name_from_file(lsp, file_path, name)
                        if qn:
                            qualified_names.append(qn)
        except Exception as e:
            print(f"Error searching workspace symbols: {e}", file=sys.stderr)

    return list(set(qualified_names))


def flatten_document_symbols(symbols: list) -> list[dict]:
    """Flatten nested document symbols into a flat list."""
    result = []
    for sym in symbols:
        result.append(sym)
        children = sym.get("children", [])
        if children:
            result.extend(flatten_document_symbols(children))
    return result


def construct_qualified_name_from_file(lsp: RustAnalyzerLSP, file_path: str, symbol_name: str) -> Optional[str]:
    """Construct a qualified name from file path and symbol name."""
    repo_root = str(lsp.repo_path)
    rel_path = file_path
    if file_path.startswith(repo_root):
        rel_path = file_path[len(repo_root):].lstrip("/")

    parts = rel_path.split("/")
    if parts[0] == "crates" and len(parts) > 1:
        crate_name = parts[1].replace("-", "_")

        # Get module path
        if len(parts) > 2:
            module_parts = parts[2:]
            if module_parts[0] == "src":
                module_parts = module_parts[1:]

            if module_parts:
                last = module_parts[-1]
                if last.endswith(".rs"):
                    last = last[:-3]
                if last in ("lib", "mod"):
                    module_parts = module_parts[:-1]
                else:
                    module_parts[-1] = last

            if module_parts:
                module_path = "::".join(module_parts)
                return f"{crate_name}::{module_path}::{symbol_name}"
            else:
                return f"{crate_name}::{symbol_name}"
        else:
            return f"{crate_name}::{symbol_name}"

    return symbol_name


def process_chain_query(
    lsp: RustAnalyzerLSP,
    lsp_query: dict,
) -> list[str]:
    """Process a chained multi-hop LSP query."""
    chain = lsp_query.get("chain", [])
    if not chain:
        return []

    # Start with the first hop
    current_locations: list[tuple[str, int, int]] = []  # (file, line, col)
    current_names: list[str] = []

    for i, hop in enumerate(chain):
        method = hop.get("method", "")

        if i == 0:
            # First hop uses explicit target
            target = hop.get("target", "")
            if target:
                hop_query = {"method": method, "target": target}
                current_names = process_single_hop_query(lsp, hop_query)
                print(f"    Hop {i+1}: {len(current_names)} results from {method}", file=sys.stderr)
        else:
            # Subsequent hops use results from previous
            new_names = []
            # Limit to avoid explosion
            for prev_name in current_names[:20]:
                # Resolve the qualified name to a location
                loc = resolve_qualified_name_to_location(lsp, prev_name)
                if loc:
                    file_path, line, col = loc
                    hop_query = {"method": method, "target": f"{file_path}:{line+1}:{col+1}"}
                    hop_results = process_single_hop_query(lsp, hop_query)
                    new_names.extend(hop_results)

            current_names = list(set(new_names))
            print(f"    Hop {i+1}: {len(current_names)} results from {method}", file=sys.stderr)

    return current_names


def process_query(lsp: RustAnalyzerLSP, query: dict) -> list[str]:
    """Process a single evaluation query and return ground truth."""
    lsp_query = query.get("lsp_query")
    if not lsp_query:
        return []

    query_type = lsp_query.get("type", "single_hop")

    # If no explicit type but has method/target, treat as single_hop
    if query_type == "single_hop" or (lsp_query.get("method") and lsp_query.get("target")):
        return process_single_hop_query(lsp, lsp_query)
    elif query_type == "chain":
        return process_chain_query(lsp, lsp_query)
    elif query_type == "workspace_symbol":
        return process_workspace_symbol_query(lsp, lsp_query)
    else:
        print(f"Unknown query type: {query_type}", file=sys.stderr)
        return []


def main():
    parser = argparse.ArgumentParser(
        description="Extract ground truth using rust-analyzer LSP"
    )
    parser.add_argument(
        "--repo",
        required=True,
        help="Path to the rust-analyzer repository",
    )
    parser.add_argument(
        "--queries",
        required=True,
        help="Path to the evaluation queries JSON file",
    )
    parser.add_argument(
        "--output",
        help="Output path for updated queries (default: overwrite input)",
    )
    parser.add_argument(
        "--ra-binary",
        default="rust-analyzer",
        help="Path to rust-analyzer binary",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Don't write output, just print what would be extracted",
    )

    args = parser.parse_args()

    # Load queries
    with open(args.queries, "r") as f:
        data = json.load(f)

    queries = data.get("queries", [])
    print(f"Loaded {len(queries)} queries")

    # Filter queries that have lsp_query metadata
    queries_with_lsp = [q for q in queries if q.get("lsp_query")]
    print(f"Found {len(queries_with_lsp)} queries with LSP metadata")

    if not queries_with_lsp:
        print("No queries with lsp_query metadata. Add lsp_query to queries first.")
        return

    # Clear the document symbol cache for a fresh start
    global _doc_symbol_cache
    _doc_symbol_cache = {}

    # Start LSP server
    print(f"Starting rust-analyzer on {args.repo}...")
    lsp = RustAnalyzerLSP(args.repo, args.ra_binary)

    try:
        lsp.start()
        print("LSP server started")

        # Give it time to index
        print("Waiting for indexing (30s)...")
        time.sleep(30)

        # Process each query
        for query in queries_with_lsp:
            print(f"Processing {query['id']}...")
            results = process_query(lsp, query)

            if results:
                print(f"  Found {len(results)} results")
                if args.dry_run:
                    for r in results[:10]:
                        print(f"    - {r}")
                    if len(results) > 10:
                        print(f"    ... and {len(results) - 10} more")
                else:
                    # Update the query with exhaustive expected list
                    query["expected"] = results[:1000]  # Cap at 1000
            else:
                print("  No results found")

    finally:
        print("Stopping LSP server...")
        lsp.stop()

    # Write output
    if not args.dry_run:
        output_path = args.output or args.queries
        with open(output_path, "w") as f:
            json.dump(data, f, indent=2)
        print(f"Updated queries written to {output_path}")


if __name__ == "__main__":
    main()
