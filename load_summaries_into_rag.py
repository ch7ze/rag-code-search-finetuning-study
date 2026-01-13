"""
Load pre-generated function summaries into the RAG system.

This script demonstrates how to use function_summaries.json to enhance
the RAG system's retrieval performance.
"""

import json
from typing import Dict, List


class SummaryLoader:
    """Loads and provides access to pre-generated function summaries"""

    def __init__(self, summaries_file: str = "function_summaries.json"):
        """
        Load summaries from JSON file.

        Args:
            summaries_file: Path to function_summaries.json
        """
        print(f"Loading summaries from: {summaries_file}")

        with open(summaries_file, 'r', encoding='utf-8') as f:
            data = json.load(f)

        self.metadata = data['metadata']
        self.summaries = data['summaries']

        # Build lookup index: location -> summary
        self.summary_index = {
            s['location']: s['llm_summary']
            for s in self.summaries
            if s.get('llm_summary')
        }

        print(f"✓ Loaded {len(self.summary_index)} summaries")
        print(f"  Model used: {self.metadata.get('model_used', 'unknown')}")
        print(f"  Total functions: {self.metadata.get('total_functions', 0)}\n")

    def get_summary(self, location: str) -> str:
        """
        Get LLM summary for a specific function location.

        Args:
            location: Function location (e.g., "codebase/src/backend/auth.rs:create_jwt")

        Returns:
            LLM-generated summary or empty string if not found
        """
        # Normalize path separators
        location_normalized = location.replace('\\', '/')
        return self.summary_index.get(location_normalized, '')

    def has_summary(self, location: str) -> bool:
        """Check if a summary exists for this location"""
        location_normalized = location.replace('\\', '/')
        return location_normalized in self.summary_index

    def get_statistics(self) -> Dict:
        """Get statistics about the summaries"""
        return {
            'total': len(self.summaries),
            'with_summary': len(self.summary_index),
            'without_summary': len(self.summaries) - len(self.summary_index),
            'coverage': len(self.summary_index) / len(self.summaries) * 100 if self.summaries else 0
        }


# Example: How to integrate with RAG system
def enhance_chunk_with_summary(chunk: Dict, summary_loader: SummaryLoader) -> Dict:
    """
    Enhance a chunk with LLM summary if available.

    Args:
        chunk: Chunk dictionary from RAG system
        summary_loader: SummaryLoader instance

    Returns:
        Enhanced chunk with 'llm_summary' field
    """
    location = chunk['location']
    llm_summary = summary_loader.get_summary(location)

    # Add LLM summary to chunk
    chunk['llm_summary'] = llm_summary

    return chunk


def get_best_documentation(chunk: Dict) -> str:
    """
    Get the best available documentation for a chunk.
    Priority: LLM Summary > Original Docstring > Empty

    Args:
        chunk: Chunk with optional 'llm_summary' and 'docstring' fields

    Returns:
        Best available documentation string
    """
    return (
        chunk.get('llm_summary', '').strip() or
        chunk.get('docstring', '').strip() or
        ''
    )


# Example usage
if __name__ == "__main__":
    print("="*80)
    print("SUMMARY LOADER - Example Usage")
    print("="*80)
    print()

    # Load summaries
    try:
        loader = SummaryLoader("function_summaries.json")
    except FileNotFoundError:
        print("ERROR: function_summaries.json not found!")
        print("Please run: python generate_function_summaries.py")
        exit(1)

    # Show statistics
    stats = loader.get_statistics()
    print("Statistics:")
    print(f"  Total functions: {stats['total']}")
    print(f"  With LLM summary: {stats['with_summary']}")
    print(f"  Coverage: {stats['coverage']:.1f}%")
    print()

    # Example: Get summary for specific function
    test_location = "codebase/src/backend/auth.rs:create_jwt"
    if loader.has_summary(test_location):
        summary = loader.get_summary(test_location)
        print(f"Example summary for {test_location}:")
        print(f"  {summary}")
    else:
        print(f"No summary found for {test_location}")

    print()
    print("="*80)
    print("INTEGRATION WITH RAG SYSTEM")
    print("="*80)
    print()
    print("To use summaries in your RAG system, modify rag_system.py:")
    print()
    print("  # In index_codebase(), after extracting chunks:")
    print("  from load_summaries_into_rag import SummaryLoader")
    print("  summary_loader = SummaryLoader('function_summaries.json')")
    print()
    print("  # Enhance each chunk:")
    print("  for chunk in all_chunks:")
    print("      chunk['llm_summary'] = summary_loader.get_summary(chunk['location'])")
    print()
    print("  # Use in embeddings (line 461):")
    print("  documentation = chunk.get('llm_summary', '') or chunk.get('docstring', '')")
    print("  enriched = f\"{chunk['name']}\\n{documentation}\\n{chunk['code']}\"")
    print()
