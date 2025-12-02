"""Dataset loading and synthetic repository creation for RepoBench-R evaluation."""

import gzip
import json
import pickle
import shutil
import subprocess
import textwrap
from pathlib import Path
from typing import Iterator

from datasets import Dataset
from huggingface_hub import hf_hub_download


def load_repobench_dataset(
    subset: str = "python_cff",
    split: str = "test_hard",
    max_cases: int | None = None,
) -> Dataset:
    """Load RepoBench-R dataset from HuggingFace.

    Args:
        subset: Dataset subset (python_cff, python_cfr, java_cff, java_cfr)
        split: Dataset split (test_hard, test_easy, train_hard, train_easy)
        max_cases: Maximum number of cases to load (None for all)

    Returns:
        HuggingFace Dataset object
    """
    repo_id = "tianyang/repobench-r"

    # The data files are organized as: data/{subset}.gz
    # Each file contains all splits, with split info in each record
    data_file = f"data/{subset}.gz"

    print(f"Downloading {data_file}...")
    local_path = hf_hub_download(
        repo_id=repo_id,
        filename=data_file,
        repo_type="dataset",
    )

    # The .gz file is actually a gzipped pickle file
    print(f"Loading data (this may take a moment for large files)...")
    with gzip.open(local_path, "rb") as f:
        data = pickle.load(f)

    # Data is organized as {split: {difficulty: [records]}}
    # e.g., data['test']['hard'] or data['train']['easy']
    if not isinstance(data, dict):
        raise ValueError(f"Expected dict with split keys, got {type(data)}")

    # Parse split name like "test_hard" -> ("test", "hard")
    if "_" not in split:
        raise ValueError(f"Split must be like 'test_hard' or 'train_easy', got '{split}'")

    split_name, difficulty = split.rsplit("_", 1)

    if split_name not in data:
        available = list(data.keys())
        raise KeyError(f"Split '{split_name}' not found. Available: {available}")

    if difficulty not in data[split_name]:
        available = list(data[split_name].keys())
        raise KeyError(f"Difficulty '{difficulty}' not found. Available: {available}")

    records = data[split_name][difficulty]
    print(f"Loaded {len(records)} records for split '{split}'")

    # Convert to HuggingFace Dataset
    ds = Dataset.from_list(records)

    if max_cases is not None and max_cases < len(ds):
        ds = ds.select(range(max_cases))

    return ds


def create_snippet_file(case_idx: int, snippet_idx: int, snippet: str) -> str:
    """Create a Python file content wrapping a snippet in a function.

    This ensures each snippet becomes a Function entity with predictable naming.

    Args:
        case_idx: Test case index
        snippet_idx: Snippet index within the case
        snippet: Raw code snippet content

    Returns:
        Python file content as string
    """
    # Indent the snippet to be inside the function body
    # Handle empty snippets
    if not snippet.strip():
        snippet = "pass"

    indented_snippet = textwrap.indent(snippet, "    ")

    return f'''"""RepoBench-R evaluation snippet: case {case_idx}, snippet {snippet_idx}."""


def snippet_{case_idx}_{snippet_idx}():
    """Context snippet {snippet_idx} from RepoBench-R case {case_idx}.

    This function wraps a code context snippet for retrieval evaluation.
    The original snippet content follows in the function body.
    """
{indented_snippet}
'''


def create_synthetic_repository(
    dataset: Dataset,
    output_dir: Path,
    language: str = "python",
) -> dict[str, str]:
    """Create a synthetic repository with code files from dataset snippets.

    Args:
        dataset: RepoBench-R dataset
        output_dir: Directory to create the synthetic repository in
        language: Programming language (python or java)

    Returns:
        Dict mapping entity_id patterns to file paths
    """
    # Clean and create output directory
    if output_dir.exists():
        shutil.rmtree(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    # Initialize as git repo (required for codesearch index)
    subprocess.run(
        ["git", "init"],
        cwd=output_dir,
        check=True,
        capture_output=True,
    )

    # Configure git user for the repo
    subprocess.run(
        ["git", "config", "user.email", "eval@repobench.local"],
        cwd=output_dir,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "RepoBench Evaluation"],
        cwd=output_dir,
        check=True,
        capture_output=True,
    )

    entity_mapping = {}
    ext = ".py" if language == "python" else ".java"

    for case_idx, case in enumerate(dataset):
        context_snippets = case["context"]

        # Create a directory for each case
        case_dir = output_dir / f"case_{case_idx}"
        case_dir.mkdir(exist_ok=True)

        for snippet_idx, snippet in enumerate(context_snippets):
            # Create file for this snippet
            file_name = f"snippet_{snippet_idx}{ext}"
            file_path = case_dir / file_name

            if language == "python":
                content = create_snippet_file(case_idx, snippet_idx, snippet)
            else:
                # Java support - wrap in class
                content = create_java_snippet_file(case_idx, snippet_idx, snippet)

            file_path.write_text(content)

            # Track entity mapping
            entity_id = f"snippet_{case_idx}_{snippet_idx}"
            entity_mapping[entity_id] = str(file_path.relative_to(output_dir))

    # Create initial commit
    subprocess.run(
        ["git", "add", "."],
        cwd=output_dir,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "commit", "-m", "Initial RepoBench-R evaluation dataset"],
        cwd=output_dir,
        check=True,
        capture_output=True,
    )

    return entity_mapping


def create_java_snippet_file(case_idx: int, snippet_idx: int, snippet: str) -> str:
    """Create a Java file content wrapping a snippet in a method.

    Args:
        case_idx: Test case index
        snippet_idx: Snippet index within the case
        snippet: Raw code snippet content

    Returns:
        Java file content as string
    """
    if not snippet.strip():
        snippet = "// empty snippet"

    indented_snippet = textwrap.indent(snippet, "        ")

    return f'''/**
 * RepoBench-R evaluation snippet: case {case_idx}, snippet {snippet_idx}.
 */
public class Snippet_{case_idx}_{snippet_idx} {{

    /**
     * Context snippet {snippet_idx} from RepoBench-R case {case_idx}.
     *
     * This method wraps a code context snippet for retrieval evaluation.
     * The original snippet content follows in the method body.
     */
    public void snippet_{case_idx}_{snippet_idx}() {{
{indented_snippet}
    }}
}}
'''


def index_synthetic_repository(repo_path: Path, codesearch_bin: str | None = None) -> str:
    """Run codesearch index on the synthetic repository.

    Args:
        repo_path: Path to the synthetic repository
        codesearch_bin: Path to codesearch binary (uses PATH if None)

    Returns:
        Repository UUID assigned by codesearch

    Raises:
        RuntimeError: If indexing fails
    """
    cmd = [codesearch_bin or "codesearch", "index"]

    result = subprocess.run(
        cmd,
        cwd=repo_path,
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        raise RuntimeError(
            f"Codesearch indexing failed with exit code {result.returncode}.\n"
            f"stdout: {result.stdout}\n"
            f"stderr: {result.stderr}"
        )

    # Parse repository UUID from output
    # The indexer outputs: "Repository ID: <uuid>"
    for line in result.stdout.splitlines():
        if "Repository ID:" in line:
            return line.split(":")[-1].strip()

    # If not found in stdout, try stderr
    for line in result.stderr.splitlines():
        if "Repository ID:" in line or "repository_id" in line.lower():
            # Try to extract UUID pattern
            import re
            uuid_pattern = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
            match = re.search(uuid_pattern, line, re.IGNORECASE)
            if match:
                return match.group(0)

    raise RuntimeError(
        f"Could not find repository UUID in codesearch output.\n"
        f"stdout: {result.stdout}\n"
        f"stderr: {result.stderr}"
    )


def iterate_cases(dataset: Dataset) -> Iterator[tuple[int, str, int, list[str]]]:
    """Iterate over dataset cases with relevant fields.

    Yields:
        Tuple of (case_idx, query_code, golden_snippet_index, context_snippets)
    """
    for idx, case in enumerate(dataset):
        yield (
            idx,
            case["code"],
            case["golden_snippet_index"],
            case["context"],
        )
