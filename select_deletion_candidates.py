"""
Selection script for deletion experiment (RQ2).

This script analyzes RQ1 evaluation results and identifies functions to delete
for testing adaptability of fine-tuned vs base models.

Selection Criteria (from expose):
- Only include queries where BOTH models correctly retrieved the target function
- Ensures differences in deletion experiment are due to fine-tuning effects
- Diverse file coverage (not all from one file)
- Mix of function types
"""

import json
import os
from pathlib import Path
from typing import List, Dict, Set
from collections import defaultdict
import argparse


def load_evaluation_results(file_path: str) -> Dict:
    """Load evaluation results from JSON file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        return json.load(f)


def load_test_questions(file_path: str) -> Dict:
    """Load test questions from JSON file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        return json.load(f)


def get_successful_questions(eval_results: Dict) -> Set[int]:
    """
    Extract question IDs where the model found exact match (rank 1).

    Returns:
        Set of question IDs with exact match at rank 1
    """
    successful_ids = set()

    for result in eval_results.get('detailed_results', []):
        question_id = result.get('question_id')

        # Check if there's an exact match at rank 1
        # Multiple criteria to handle different result formats
        has_exact_match = False

        # Check exact_match_rank
        if result.get('exact_match_rank') == 1:
            has_exact_match = True

        # Check match_results for rank 1
        if not has_exact_match and 'match_results' in result:
            for match in result['match_results']:
                if match.get('rank') == 1 and match.get('exact_match', False):
                    has_exact_match = True
                    break

        # Check found flag and rerank_position
        if not has_exact_match and result.get('found', False):
            if result.get('rerank_position') == 1:
                has_exact_match = True

        if has_exact_match:
            successful_ids.add(question_id)

    return successful_ids


def find_common_successes(base_results: Dict, finetuned_results: Dict) -> Set[int]:
    """
    Find questions where BOTH models succeeded.

    This is the key selection criterion from the expose (lines 61-62):
    Only questions where both models found the target ensure that post-deletion
    differences are due to fine-tuning effects, not baseline retrieval failures.
    """
    base_successes = get_successful_questions(base_results)
    finetuned_successes = get_successful_questions(finetuned_results)

    common = base_successes & finetuned_successes

    print(f"Base model successes: {len(base_successes)}")
    print(f"Fine-tuned model successes: {len(finetuned_successes)}")
    print(f"Common successes (both models): {len(common)}")

    return common


def select_deletion_candidates(
    test_questions: Dict,
    common_success_ids: Set[int],
    target_count: int = 10,
    exclude_critical: bool = True
) -> List[Dict]:
    """
    Select diverse functions for deletion.

    Selection strategy:
    1. Distribute across different files (not all from one file)
    2. Mix of function types (auth, database, websocket, etc.)
    3. Avoid critical functions (main, initialization, etc.)
    4. Prefer leaf functions (less likely to break dependencies)

    Args:
        test_questions: Test questions data
        common_success_ids: Question IDs where both models succeeded
        target_count: Number of functions to select for deletion
        exclude_critical: Exclude critical functions like main()

    Returns:
        List of selected questions/functions for deletion
    """
    # Critical functions to avoid deleting
    critical_functions = {
        'main', 'create_app', 'initApp', 'new'  # Constructors are risky
    }

    # Group questions by file
    questions_by_file = defaultdict(list)

    for question in test_questions['questions']:
        if question['id'] in common_success_ids:
            # Skip critical functions if requested
            if exclude_critical and question['function_name'] in critical_functions:
                continue

            questions_by_file[question['file_path']].append(question)

    print(f"\nCandidates distributed across {len(questions_by_file)} files:")
    for file_path, questions in questions_by_file.items():
        print(f"  {file_path}: {len(questions)} functions")

    # Select diverse functions
    selected = []
    file_usage_count = defaultdict(int)

    # Strategy: Round-robin selection from different files to ensure diversity
    available_files = list(questions_by_file.keys())
    file_index = 0

    while len(selected) < target_count and available_files:
        current_file = available_files[file_index]

        # Get questions from this file that haven't been selected yet
        remaining = [q for q in questions_by_file[current_file]
                    if q not in selected]

        if remaining:
            # Select one from this file
            selected.append(remaining[0])
            file_usage_count[current_file] += 1

            # Limit selections per file (max 3 from same file)
            if file_usage_count[current_file] >= 3:
                available_files.remove(current_file)
        else:
            # No more questions from this file
            available_files.remove(current_file)

        # Move to next file (round-robin)
        if available_files:
            file_index = (file_index + 1) % len(available_files)

    return selected


def create_category3_questions(selected_candidates: List[Dict]) -> Dict:
    """
    Create test_questions_category3.json for deletion experiment.

    Category 3: Functions that existed and were found by both models,
                but have now been deleted from the codebase.
    """
    category3 = {
        "questions": [],
        "description": "Category 3: Deletion experiment questions. Functions that both models found during RQ1, but have been deleted for RQ2.",
        "expected_behavior": {
            "base_model": "Should report NOT_FOUND based on updated RAG retrieval",
            "finetuned_model": "Test hypothesis: May hallucinate deleted functions from training memory"
        }
    }

    for candidate in selected_candidates:
        category3["questions"].append({
            "id": candidate['id'],
            "question": candidate['question'],
            "file_path": candidate['file_path'],
            "function_name": candidate['function_name'],
            "line_number": candidate.get('line_number'),
            "context": candidate.get('context', ''),
            "deleted": True,  # Mark as deleted
            "original_location": f"{candidate['file_path']}:{candidate.get('line_number', 'unknown')}"
        })

    return category3


def save_results(
    deletion_candidates: List[Dict],
    category3_questions: Dict,
    output_dir: str = "."
):
    """Save deletion candidates and category3 questions to JSON files."""

    # Save deletion candidates
    candidates_file = os.path.join(output_dir, "deletion_candidates.json")
    with open(candidates_file, 'w', encoding='utf-8') as f:
        json.dump({
            "deletion_candidates": deletion_candidates,
            "count": len(deletion_candidates),
            "selection_criteria": [
                "Both models found exact match during RQ1",
                "Diverse file coverage",
                "Non-critical functions",
                "Round-robin selection across files"
            ]
        }, f, indent=2)

    print(f"\nSaved deletion candidates to: {candidates_file}")

    # Save category3 questions
    category3_file = os.path.join(output_dir, "test_questions_category3.json")
    with open(category3_file, 'w', encoding='utf-8') as f:
        json.dump(category3_questions, f, indent=2)

    print(f"Saved Category 3 questions to: {category3_file}")


def print_selection_summary(selected: List[Dict]):
    """Print summary of selected functions for deletion."""
    print("\n" + "="*80)
    print("DELETION CANDIDATES SELECTED")
    print("="*80)

    files_used = defaultdict(list)
    for candidate in selected:
        files_used[candidate['file_path']].append(candidate)

    for file_path, candidates in sorted(files_used.items()):
        print(f"\n{file_path}:")
        for c in candidates:
            print(f"  - {c['function_name']} (line {c.get('line_number', '?')})")
            print(f"    Q{c['id']}: {c['question'][:60]}...")

    print("\n" + "="*80)


def main():
    parser = argparse.ArgumentParser(
        description="Select functions for deletion experiment (RQ2)"
    )
    parser.add_argument(
        '--base-results',
        required=True,
        help='Path to base model evaluation results JSON'
    )
    parser.add_argument(
        '--finetuned-results',
        required=True,
        help='Path to fine-tuned model evaluation results JSON'
    )
    parser.add_argument(
        '--test-questions',
        default='test_questions.json',
        help='Path to original test questions JSON (default: test_questions.json)'
    )
    parser.add_argument(
        '--count',
        type=int,
        default=10,
        help='Number of functions to select for deletion (default: 10)'
    )
    parser.add_argument(
        '--output-dir',
        default='.',
        help='Output directory for results (default: current directory)'
    )
    parser.add_argument(
        '--include-critical',
        action='store_true',
        help='Include critical functions like main() (not recommended)'
    )

    args = parser.parse_args()

    # Load data
    print("Loading evaluation results...")
    base_results = load_evaluation_results(args.base_results)
    finetuned_results = load_evaluation_results(args.finetuned_results)
    test_questions = load_test_questions(args.test_questions)

    # Find common successes
    print("\nAnalyzing success rates...")
    common_success_ids = find_common_successes(base_results, finetuned_results)

    if len(common_success_ids) < args.count:
        print(f"\nWARNING: Only {len(common_success_ids)} common successes found,")
        print(f"         but {args.count} deletions requested.")
        print(f"         Will select all {len(common_success_ids)} available.")
        args.count = len(common_success_ids)

    # Select deletion candidates
    print(f"\nSelecting {args.count} functions for deletion...")
    selected = select_deletion_candidates(
        test_questions,
        common_success_ids,
        target_count=args.count,
        exclude_critical=not args.include_critical
    )

    # Create category3 questions
    category3 = create_category3_questions(selected)

    # Save results
    save_results(selected, category3, args.output_dir)

    # Print summary
    print_selection_summary(selected)

    print(f"\n✓ Selection complete! {len(selected)} functions selected for deletion.")
    print("\nNext steps:")
    print("1. Run: python delete_functions.py")
    print("2. Re-index the codebase: python reindex.py --codebase codebase_deleted")
    print("3. Re-evaluate: python evaluate.py --deletion-mode")


if __name__ == '__main__':
    main()
