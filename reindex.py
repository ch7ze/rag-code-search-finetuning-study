"""
Re-index the codebase after improving chunking.
This deletes the old ChromaDB and creates a new index.

Modified to support deletion experiment (RQ2):
- Accepts --codebase parameter to index different codebase versions
- Supports --db-path to use different ChromaDB locations
"""

import shutil
import os
import argparse
from pathlib import Path
from rag_system import ImprovedRAGSystem

def main():
    parser = argparse.ArgumentParser(
        description="Re-index codebase with ChromaDB"
    )
    parser.add_argument(
        '--codebase',
        default='./codebase',
        help='Path to codebase directory (default: ./codebase)'
    )
    parser.add_argument(
        '--db-path',
        default='./chroma_db',
        help='Path to ChromaDB directory (default: ./chroma_db)'
    )
    parser.add_argument(
        '--docstring-only',
        action='store_true',
        help='Index only docstrings/comments (not full code)'
    )
    parser.add_argument(
        '--keep-old',
        action='store_true',
        help='Keep old database (append mode, not recommended)'
    )

    args = parser.parse_args()

    codebase_path = Path(args.codebase)
    db_path = Path(args.db_path)

    # Validate codebase exists
    if not codebase_path.exists():
        print(f"ERROR: Codebase directory not found: {codebase_path}")
        return

    # Delete old ChromaDB unless --keep-old specified
    if not args.keep_old and db_path.exists():
        print(f"Deleting old database: {db_path}")
        shutil.rmtree(db_path)
        print("✓ Old database deleted\n")

    # Create new RAG system
    print("Creating new RAG system...")
    print(f"  Codebase: {codebase_path}")
    print(f"  Database: {db_path}")
    print(f"  Mode: {'docstring-only' if args.docstring_only else 'full-code'}")

    rag = ImprovedRAGSystem(
        persist_directory=str(db_path),
        use_docstring_only=args.docstring_only
    )

    # Index codebase
    print(f"\nIndexing codebase: {codebase_path}")
    rag.index_codebase(str(codebase_path))

    print(f"\n✓ Re-indexing complete!")
    print(f"  Total chunks: {rag.collection.count()}")

    # Test a few queries to verify
    print("\n--- Test Queries ---")
    test_queries = [
        "create JWT token",
        "ESP32 TCP connection manager",
        "loadTemplate frontend",
    ]

    for query in test_queries:
        print(f"\nQuery: {query}")
        results = rag.retrieve(query, top_k=3, hybrid=True)
        for i, result in enumerate(results, 1):
            print(f"  [{i}] {result['name']} in {result['location']}")

if __name__ == "__main__":
    main()
