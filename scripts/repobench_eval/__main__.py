"""CLI entry point for RepoBench-R evaluation harness.

Usage:
    python -m scripts.repobench_eval setup [OPTIONS]   # Create and index synthetic repo
    python -m scripts.repobench_eval run [OPTIONS]     # Run evaluation queries

Workflow:
    1. Setup: Create synthetic repository and index it
       $ python -m scripts.repobench_eval setup --repo-path ~/repos/repobench-eval

    2. Start server (in another terminal):
       $ cd ~/repos/repobench-eval && codesearch serve

    3. Run evaluation:
       $ python -m scripts.repobench_eval run --repo-path ~/repos/repobench-eval

Examples:
    # Quick test with 50 cases
    python -m scripts.repobench_eval setup --repo-path ~/repos/repobench-eval --max-cases 50
    # Then start server, then:
    python -m scripts.repobench_eval run --repo-path ~/repos/repobench-eval --max-cases 50
"""

import argparse
import json
import sys
from pathlib import Path

from .evaluate import print_summary, run_evaluation_queries, save_report
from .dataset import (
    create_synthetic_repository,
    index_synthetic_repository,
    load_repobench_dataset,
)


def cmd_setup(args: argparse.Namespace) -> int:
    """Setup command: create and index synthetic repository."""
    repo_path = args.repo_path.expanduser().resolve()

    print(f"Loading RepoBench-R dataset: {args.subset}/{args.split}")
    dataset = load_repobench_dataset(args.subset, args.split, args.max_cases)
    print(f"Loaded {len(dataset)} test cases")

    language = "python" if "python" in args.subset else "java"

    print(f"\nCreating synthetic repository at {repo_path}")
    entity_mapping = create_synthetic_repository(dataset, repo_path, language)
    print(f"Created {len(entity_mapping)} snippet files")

    print("\nIndexing repository with codesearch...")
    repository_id = index_synthetic_repository(repo_path, args.codesearch_bin)
    print(f"Repository indexed successfully!")
    print(f"  Repository ID: {repository_id}")
    print(f"  Path: {repo_path}")

    # Save metadata for the run phase
    metadata_path = repo_path / ".repobench_eval.json"
    metadata = {
        "repository_id": repository_id,
        "subset": args.subset,
        "split": args.split,
        "max_cases": args.max_cases,
        "num_snippets": len(entity_mapping),
    }
    metadata_path.write_text(json.dumps(metadata, indent=2))

    print(f"\nSetup complete. Next steps:")
    print(f"  1. Start the server:  cd {repo_path} && codesearch serve")
    print(f"  2. Run evaluation:    python -m scripts.repobench_eval run --repo-path {repo_path}")

    return 0


def cmd_run(args: argparse.Namespace) -> int:
    """Run command: execute evaluation queries against running server."""
    repo_path = args.repo_path.expanduser().resolve()

    # Load metadata from setup phase
    metadata_path = repo_path / ".repobench_eval.json"
    if not metadata_path.exists():
        print(f"Error: No setup metadata found at {metadata_path}", file=sys.stderr)
        print("Run 'python -m scripts.repobench_eval setup' first.", file=sys.stderr)
        return 1

    metadata = json.loads(metadata_path.read_text())
    repository_id = metadata["repository_id"]
    subset = args.subset or metadata["subset"]
    split = args.split or metadata["split"]
    max_cases = args.max_cases or metadata["max_cases"]

    print(f"Loading RepoBench-R dataset: {subset}/{split}")
    dataset = load_repobench_dataset(subset, split, max_cases)
    print(f"Loaded {len(dataset)} test cases")

    print(f"\nRunning evaluation against {args.base_url}")
    print(f"Repository ID: {repository_id}")

    report = run_evaluation_queries(
        dataset=dataset,
        repository_id=repository_id,
        subset=subset,
        split=split,
        base_url=args.base_url,
    )

    # Save report
    save_report(report, args.output)

    # Print summary
    if not args.quiet:
        print_summary(report)

    return 0


def main() -> int:
    """Main entry point for CLI."""
    parser = argparse.ArgumentParser(
        description="RepoBench-R evaluation harness for codesearch retrieval benchmarking",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # Setup subcommand
    setup_parser = subparsers.add_parser(
        "setup",
        help="Create and index synthetic repository from RepoBench-R dataset",
    )
    setup_parser.add_argument(
        "--repo-path",
        type=Path,
        required=True,
        help="Directory for synthetic repository (will be created)",
    )
    setup_parser.add_argument(
        "--subset",
        default="python_cff",
        choices=["python_cff", "python_cfr", "java_cff", "java_cfr"],
        help="RepoBench-R dataset subset (default: python_cff)",
    )
    setup_parser.add_argument(
        "--split",
        default="test_hard",
        choices=["train_easy", "train_hard", "test_easy", "test_hard"],
        help="Dataset split (default: test_hard)",
    )
    setup_parser.add_argument(
        "--max-cases",
        type=int,
        default=500,
        help="Maximum number of test cases (default: 500)",
    )
    setup_parser.add_argument(
        "--codesearch-bin",
        help="Path to codesearch binary (default: uses PATH)",
    )

    # Run subcommand
    run_parser = subparsers.add_parser(
        "run",
        help="Run evaluation queries against running codesearch server",
    )
    run_parser.add_argument(
        "--repo-path",
        type=Path,
        required=True,
        help="Path to synthetic repository (from setup phase)",
    )
    run_parser.add_argument(
        "--subset",
        choices=["python_cff", "python_cfr", "java_cff", "java_cfr"],
        help="Override dataset subset from setup",
    )
    run_parser.add_argument(
        "--split",
        choices=["train_easy", "train_hard", "test_easy", "test_hard"],
        help="Override dataset split from setup",
    )
    run_parser.add_argument(
        "--max-cases",
        type=int,
        help="Override max cases from setup",
    )
    run_parser.add_argument(
        "--output",
        type=Path,
        default=Path("target/repobench_eval_report.json"),
        help="Output JSON report path (default: target/repobench_eval_report.json)",
    )
    run_parser.add_argument(
        "--base-url",
        default="http://localhost:8080",
        help="Codesearch REST API base URL (default: http://localhost:8080)",
    )
    run_parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress progress output",
    )

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        print("\nUse 'setup' to create repository, then 'run' to evaluate.")
        return 1

    try:
        if args.command == "setup":
            return cmd_setup(args)
        elif args.command == "run":
            return cmd_run(args)
        else:
            parser.print_help()
            return 1

    except KeyboardInterrupt:
        print("\nInterrupted by user")
        return 130

    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())
