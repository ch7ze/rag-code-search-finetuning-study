"""
Automatic Evaluation Script for RAG-based Code Search
Evaluates retrieval accuracy according to the research methodology in the exposé
"""

# ============================================================================
# EVALUATION CONFIGURATION
# ============================================================================

# ============================================================================
# MULTI-MODE TESTING - Test all LLM selection modes automatically
# ============================================================================
TEST_ALL_MODES = True  # Set to True to test all modes on all datasets automatically
                        # False = Use single mode specified in rag_chat.py

# If TEST_ALL_MODES = True, these modes will be tested:
MODES_TO_TEST = ["conservative", "aggressive", "aggressive_no_fewshot"]

# If TEST_ALL_MODES = True, these datasets will be tested:
DATASETS_TO_TEST = [
    "test_questions_category2.json",
    "top10_questions/top5_questions_reranking-only_20251124_145425.json"
]

# ============================================================================

# Specify which test questions file to use (relative path from evaluate.py location)
# Examples:
#   "test_questions.json"                                              -> file in same directory
#   "test_questions_v2.json"                                           -> another file in same directory
#   "top10_questions/top10_questions_docstring_20251118_143022.json"  -> file in subdirectory

#TEST_QUESTIONS_FILE = "top10_questions/top5_questions_reranking-only_20251124_145425.json"
#TEST_QUESTIONS_FILE = "test_questions_category2.json"
TEST_QUESTIONS_FILE = "test_questions_rq2.json"

MAX_QUESTIONS = None

# Global variable to track current output path for incremental saving
_CURRENT_OUTPUT_PATH = None

# TOP-N QUESTIONS EXPORT CONFIGURATION
# Specify which top-N questions should be saved to a separate file (based on re-ranking results)
# Valid options: 1, 5, 10, 20, or None to disable top-N export
SAVE_TOP_N_QUESTIONS = None  # Questions where correct answer was found in top-N re-ranking results

# LLM USAGE CONFIGURATION
USE_LLM = True  # Set to False to evaluate only the RAG system (Re-Ranking) without LLM scoring
                # True  = Use LLM to score each function individually (slower, more accurate)
                # False = Use only Re-Ranking scores (faster, baseline performance)

# INDEXING MODE CONFIGURATION
USE_FULLCODE = True      # True  = Evaluate with full code indexing (complete function code in embeddings)
                         # False = Skip full code evaluation
USE_DOCSTRING_ONLY = False  # True  = Evaluate with docstring-only indexing (signature + docstring in embeddings)
                           # False = Skip docstring-only evaluation

# NOTE: If both are True, both modes will be evaluated and compared
#       If both are False, an error will occur
#       If only one is True, only that mode will be evaluated

# DATABASE CONFIGURATION
USE_FRESH_INDEX = True  # True  = Delete existing ChromaDB and re-index from scratch
                        # False = Use existing index if available (faster, recommended)

# ============================================================================
# DELETION EXPERIMENT (RQ2) CONFIGURATION
# ============================================================================
DELETION_EXPERIMENT_MODE = False  # True  = Evaluate deletion experiment (Category 3)
                                   # False = Standard evaluation (Category 1 or 2)

DELETED_CODEBASE_PATH = "./codebase_deleted"  # Path to codebase with deleted functions
DELETED_DB_PATH = "./chroma_db_deleted"        # ChromaDB for deleted codebase
CATEGORY_3_QUESTIONS = "test_questions_category3.json"  # Questions for deleted functions

# When DELETION_EXPERIMENT_MODE = True:
# - Uses DELETED_CODEBASE_PATH for RAG retrieval
# - Uses DELETED_DB_PATH for ChromaDB
# - Expects questions in CATEGORY_3_QUESTIONS format
# - Evaluates Training Memory Interference:
#   * Base model should report NOT_FOUND (adapts to RAG)
#   * Fine-tuned model may hallucinate deleted functions (training memory)
# ============================================================================

import json
import time
import re
import sys
import io
from pathlib import Path
from typing import Dict, List, Tuple
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer
from rag_system import ImprovedRAGSystem
from rag_chat import load_model, rag_query, extract_structured_response, MODEL_CHOICE, USE_FINETUNED, FINETUNED_MODEL_PATH, USE_BATCH_RANKING, BATCH_RANKING_SIZE, LLM_SELECTION_MODE

# Windows console encoding for emoji support
if sys.platform == 'win32':
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')


def shorten_path(path: str) -> str:
    """
    Shorten path to start from 'codebase' folder.
    Example: c:\\Users\\...\\codebase\\src\\auth.rs -> codebase/src/auth.rs
    """
    if 'codebase' in path.lower():
        # Find the position of 'codebase' (case-insensitive)
        parts = path.replace('\\', '/').split('/')
        try:
            # Find 'codebase' in the parts
            for i, part in enumerate(parts):
                if part.lower() == 'codebase':
                    # Return everything from 'codebase' onwards
                    return '/'.join(parts[i:])
        except (ValueError, IndexError):
            pass
    return path


def load_test_questions(json_path: str) -> List[Dict]:
    """Load test questions with ground truth from JSON file"""
    with open(json_path, 'r', encoding='utf-8') as f:
        data = json.load(f)

    questions = data['questions']

    # Detect if this is a Category 2 (non-existent functionality) dataset
    # by checking if any question has 'exists' field
    category_2_mode = any('exists' in q for q in questions)

    # Detect if this is a Category 3 (deletion experiment) dataset
    # by checking if any question has 'deleted' field
    category_3_mode = any('deleted' in q and q.get('deleted') == True for q in questions)

    if category_2_mode:
        print(f"\n⚠️  Category 2 dataset detected (Non-Existent Functionality)")
        print(f"   Hallucination resistance evaluation mode enabled")
        print(f"   Expected behavior: Model should respond 'not found' for all queries\n")

    if category_3_mode:
        print(f"\n⚠️  Category 3 dataset detected (Deletion Experiment - RQ2)")
        print(f"   Training memory interference evaluation mode enabled")
        print(f"   Expected behavior:")
        print(f"     - Base model: Should report 'not found' (adapts to RAG)")
        print(f"     - Fine-tuned model: May hallucinate deleted functions (training memory)\n")

    return questions


def normalize_path(path: str) -> str:
    """Normalize file path for comparison (handle Windows/Linux differences)"""
    # Convert to forward slashes and lowercase
    normalized = path.replace('\\', '/').lower()

    # Remove all absolute path prefixes - keep only relative path from codebase
    if 'codebase/' in normalized:
        # Extract everything after the last occurrence of 'codebase/'
        parts = normalized.split('codebase/')
        normalized = parts[-1]  # Take last part after codebase/

    # Also handle paths like "src/backend/..." without codebase prefix
    # by ensuring consistency
    normalized = normalized.lstrip('/')

    return normalized


def extract_location_parts(location: str) -> Tuple[str, str]:
    """
    Extract file path and function/line from location string.
    Handles formats like:
    - file.rs:function_name
    - file.rs:42
    - file.rs
    """
    if ':' in location:
        file_part, func_part = location.rsplit(':', 1)
        return normalize_path(file_part), func_part.lower()
    return normalize_path(location), ""


def is_not_found_response(location: str) -> bool:
    """
    Check if the response indicates "not found" or "does not exist".
    Used for Category 2 (non-existent functionality) evaluation.

    Returns True if the model correctly refused to provide a location.
    """
    if not location or location.strip() == "":
        return True

    location_upper = location.upper()

    # Common "not found" patterns
    not_found_patterns = [
        "NOT_FOUND",
        "NOT FOUND",
        "NOTFOUND",
        "NOT EXIST",
        "DOES NOT EXIST",
        "DOESN'T EXIST",
        "NO SUCH",
        "CANNOT FIND",
        "CAN'T FIND",
        "UNABLE TO FIND",
        "NOT AVAILABLE",
        "NOT IMPLEMENTED",
        "NO IMPLEMENTATION",
        "N/A",
    ]

    return any(pattern in location_upper for pattern in not_found_patterns)


def check_location_match(predicted: str, ground_truth_file: str, ground_truth_func: str,
                         ground_truth_line: int) -> Dict[str, bool]:
    """
    Check if predicted location matches ground truth.
    Returns dict with: file_match, function_match, exact_match
    """
    if is_not_found_response(predicted):
        return {'file_match': False, 'function_match': False, 'exact_match': False}

    pred_file, pred_func = extract_location_parts(predicted)
    gt_file = normalize_path(ground_truth_file)
    gt_func = ground_truth_func.lower() if ground_truth_func else ""

    # Check file match - use exact path match (not substring)
    # Both paths should end with same file path after normalization
    file_match = pred_file == gt_file or pred_file.endswith(gt_file) or gt_file.endswith(pred_file)

    # Check function/line match
    function_match = False
    if pred_func:
        # Check if it matches function name or line number
        if gt_func:
            # Exact match or word-boundary match (not substring)
            # "create_jwt" == "create_jwt" -> TRUE
            # "create" in "create_logout_cookie" -> FALSE
            function_match = (pred_func == gt_func or
                            gt_func == pred_func or
                            f":{gt_func}" in f":{pred_func}:")  # Word boundary check
        elif pred_func.isdigit() and ground_truth_line and abs(int(pred_func) - ground_truth_line) <= 5:
            function_match = True  # Allow 5 line tolerance

    exact_match = file_match and function_match

    return {
        'file_match': file_match,
        'function_match': function_match,
        'exact_match': exact_match
    }


def calculate_mrr(results: List[Dict]) -> float:
    """
    Calculate Mean Reciprocal Rank.

    For each query, MRR = 1/rank where rank is the position of the first correct result.
    If not found in top-k, MRR = 0 for that query.

    Example:
    - Found at rank 1: MRR = 1.0
    - Found at rank 2: MRR = 0.5
    - Found at rank 3: MRR = 0.333...
    - Not found: MRR = 0.0
    """
    reciprocal_ranks = []
    for result in results:
        # Get the rank where exact match was found (if any)
        exact_match_rank = result.get('exact_match_rank', None)

        if exact_match_rank:
            # MRR = 1 / rank (e.g., rank 1 -> 1.0, rank 2 -> 0.5, rank 3 -> 0.33)
            reciprocal_ranks.append(1.0 / exact_match_rank)
        else:
            # Not found in any rank
            reciprocal_ranks.append(0.0)

    return sum(reciprocal_ranks) / len(reciprocal_ranks) if reciprocal_ranks else 0.0


def calculate_recall_at_k(results: List[Dict], k: int = 5) -> float:
    """
    Calculate Recall@K based on whether correct answer was in top-K retrieved chunks.
    Note: This checks retrieval before LLM selection.
    """
    # This would require checking if ground truth was in retrieved chunks
    # For now, we use exact_match as approximation (conservative)
    correct = sum(1 for r in results if r['exact_match'])
    return correct / len(results) if results else 0.0


def calculate_task_success_rate(results: List[Dict]) -> float:
    """Calculate percentage of queries where correct location was identified"""
    success = sum(1 for r in results if r['exact_match'])
    return (success / len(results) * 100) if results else 0.0


def evaluate_model(model, tokenizer, rag: ImprovedRAGSystem, questions: List[Dict],
                   model_name: str = "base") -> Dict:
    """
    Evaluate model on all test questions with Top-3 ranking.
    Also evaluates Re-Ranking performance (before LLM selection).
    Returns dict with metrics and detailed results.
    """
    global _CURRENT_OUTPUT_PATH

    print(f"\n{'='*80}")
    print(f"EVALUATING {model_name.upper()} MODEL")
    if USE_LLM:
        if USE_BATCH_RANKING:
            print(f"MODE: LLM Batch Selection ({BATCH_RANKING_SIZE} candidates, 1 call)")
        else:
            print(f"MODE: LLM Individual Scoring (10 candidates, 10 calls)")
    else:
        print(f"MODE: Re-Ranking Only (No LLM)")
    print(f"{'='*80}\n")

    results = []
    total_latency = 0.0

    # Category 2 (Hallucination) counters
    hallucination_count = 0  # Model provided specific location for non-existent feature
    correct_refusal_count = 0  # Model correctly said "not found"
    category_2_questions = 0  # Total Category 2 questions

    # Category 3 (Deletion Experiment - RQ2) counters
    category_3_questions = 0  # Total Category 3 questions (deleted functions)
    deleted_function_hallucination = 0  # Model provided EXACT deleted function location
    false_positive_other = 0  # Model provided wrong location (not the deleted function)
    true_negative = 0  # Model correctly said "not found"
    rag_retrieval_contained_deleted = 0  # RAG retrieved the deleted function (shouldn't happen)

    # Counters for LLM selection (after re-ranking)
    found_at_rank1 = 0
    found_at_rank2 = 0
    found_at_rank3 = 0
    found_at_rank4 = 0
    found_at_rank5 = 0
    found_in_top2 = 0
    found_in_top3 = 0
    found_in_top4 = 0
    found_in_top5 = 0

    # File match counters (LLM selection)
    file_match_at_rank1 = 0
    file_match_at_rank2 = 0
    file_match_at_rank3 = 0
    file_match_at_rank4 = 0
    file_match_at_rank5 = 0
    file_match_in_top2 = 0
    file_match_in_top3 = 0
    file_match_in_top4 = 0
    file_match_in_top5 = 0

    # Re-Ranking counters (before LLM)
    rerank_found_at_1 = 0
    rerank_found_at_3 = 0
    rerank_found_at_5 = 0
    rerank_found_at_10 = 0
    rerank_found_at_20 = 0
    rerank_found_at_40 = 0
    rerank_file_at_1 = 0
    rerank_file_at_3 = 0
    rerank_file_at_5 = 0
    rerank_file_at_10 = 0
    rerank_file_at_20 = 0
    rerank_file_at_40 = 0

    for i, question in enumerate(questions, 1):
        print(f"[{i}/{len(questions)}] {question['question']}")

        # Clear GPU cache to prevent memory leak between questions
        torch.cuda.empty_cache()

        # First, get Re-Ranking results (Top-40 before LLM selection)
        rerank_results = rag.retrieve(question['question'], top_k=40, hybrid=True)

        # Check where ground truth appears in re-ranking (if at all)
        rerank_position = None
        rerank_file_position = None

        # Check if this is a Category 2 question
        is_category_2 = question.get('exists', True) == False

        # Check if this is a Category 3 question (deletion experiment)
        is_category_3 = question.get('deleted', False) == True

        # Print top-5 re-ranking results
        print(f"  Re-Ranking Top-5:")
        for rank_idx, chunk in enumerate(rerank_results[:5], 1):
            score = chunk.get('rerank_score', 0)
            display_loc = shorten_path(chunk['location'])
            print(f"    [{rank_idx}] {display_loc} (score: {score:.3f})")

        # Only check ground truth matching for Category 1 questions
        # Category 2 questions have no ground truth (non-existent features)
        if not is_category_2:
            for rank_idx, chunk in enumerate(rerank_results, 1):
                match_result = check_location_match(
                    chunk['location'],
                    question['file_path'],
                    question.get('function_name', ''),
                    question.get('line_number', 0)
                )

                if match_result['exact_match'] and rerank_position is None:
                    rerank_position = rank_idx
                if match_result['file_match'] and rerank_file_position is None:
                    rerank_file_position = rank_idx

        # Update re-ranking counters (only for Category 1 questions with ground truth)
        if not is_category_2:
            if rerank_position == 1:
                rerank_found_at_1 += 1
            if rerank_position is not None and rerank_position <= 3:
                rerank_found_at_3 += 1
            if rerank_position is not None and rerank_position <= 5:
                rerank_found_at_5 += 1
            if rerank_position is not None and rerank_position <= 10:
                rerank_found_at_10 += 1
            if rerank_position is not None and rerank_position <= 40:
                rerank_found_at_40 += 1

            if rerank_file_position == 1:
                rerank_file_at_1 += 1
            if rerank_file_position is not None and rerank_file_position <= 3:
                rerank_file_at_3 += 1
            if rerank_file_position is not None and rerank_file_position <= 5:
                rerank_file_at_5 += 1
            if rerank_file_position is not None and rerank_file_position <= 10:
                rerank_file_at_10 += 1
            if rerank_file_position is not None and rerank_file_position <= 40:
                rerank_file_at_40 += 1

        # Measure response latency (only LLM time, not reranking)
        if USE_LLM:
            start_time = time.time()
            # Use LLM to score each function (top-10 from RAG system)
            response = rag_query(question['question'], rag, model, tokenizer)
            latency = time.time() - start_time
            total_latency += latency

            # Extract ranked locations - use all_scores to get top 5
            all_scores = response.get('all_scores', [])
            ranked_locations = []

            # Get top 5 locations from all_scores if available
            if all_scores:
                for i in range(5):
                    if i < len(all_scores):
                        ranked_locations.append(all_scores[i]['location'])
                    else:
                        ranked_locations.append('NOT_FOUND')
            else:
                # Fallback to old format (top 3)
                ranked_locations = response.get('ranked_locations', [response['location'], 'NOT_FOUND', 'NOT_FOUND'])
                # Extend to 5
                while len(ranked_locations) < 5:
                    ranked_locations.append('NOT_FOUND')
        else:
            # Use only Re-Ranking (no LLM scoring)
            # Just use the rerank_results from above (already computed)
            # No additional latency for this mode (reranking already done above)
            latency = 0.0

            ranked_locations = []
            all_scores = []

            # Extract top 5 from re-ranking results
            for i in range(5):
                if i < len(rerank_results):
                    ranked_locations.append(rerank_results[i]['location'])
                    rerank_score = rerank_results[i].get('rerank_score', 0)
                    # Convert numpy/torch float32 to Python float for JSON serialization
                    rerank_score = float(rerank_score)
                    all_scores.append({
                        'location': rerank_results[i]['location'],
                        'function_name': rerank_results[i].get('name', 'unknown'),
                        'score': rerank_score,  # Raw cross-encoder score (not scaled)
                        'rerank_score': rerank_score
                    })
                else:
                    ranked_locations.append('NOT_FOUND')

            # Create dummy response for consistency
            response = {
                'found': len(rerank_results) > 0,
                'location': ranked_locations[0],
                'ranked_locations': ranked_locations,
                'all_scores': all_scores,
                'raw_response': 'Re-Ranking only (no LLM)'
            }

        # Category 2 evaluation: Check for hallucination (is_category_2 already defined above)
        hallucinated = False
        if is_category_2:
            category_2_questions += 1

            # Check if rank 1 response is "not found" or provides a specific location
            rank1_location = ranked_locations[0] if ranked_locations else "NOT_FOUND"

            if is_not_found_response(rank1_location):
                # Correct refusal - model said "not found"
                correct_refusal_count += 1
                hallucinated = False
            else:
                # Hallucination - model provided a specific file/location for non-existent feature
                hallucination_count += 1
                hallucinated = True

        # Category 3 evaluation: Deletion experiment (RQ2 - Training Memory Interference)
        deletion_hallucination = False
        if is_category_3:
            category_3_questions += 1

            # Get the rank 1 response
            rank1_location = ranked_locations[0] if ranked_locations else "NOT_FOUND"

            # Check if RAG retrieved the deleted function (shouldn't happen if indexing worked)
            original_location = question.get('original_location', '')
            for chunk in rerank_results[:40]:
                chunk_match = check_location_match(
                    chunk['location'],
                    question['file_path'],
                    question.get('function_name', ''),
                    question.get('line_number', 0)
                )
                if chunk_match['exact_match']:
                    rag_retrieval_contained_deleted += 1
                    print(f"  ⚠️  WARNING: RAG retrieved deleted function!")
                    break

            if is_not_found_response(rank1_location):
                # Correct behavior - model reported "not found" for deleted function
                true_negative += 1
                deletion_hallucination = False
            else:
                # Model provided a location - check if it's the deleted function
                match_result = check_location_match(
                    rank1_location,
                    question['file_path'],
                    question.get('function_name', ''),
                    question.get('line_number', 0)
                )

                if match_result['exact_match'] or match_result['function_match']:
                    # Hallucination - model referenced the DELETED function from training memory!
                    deleted_function_hallucination += 1
                    deletion_hallucination = True
                else:
                    # Model provided wrong location (but not the deleted function)
                    false_positive_other += 1
                    deletion_hallucination = False

        # Check correctness for each rank (now checking top 5)
        # Skip ground truth matching for Category 2 and Category 3 questions
        match_results = []
        if not is_category_2 and not is_category_3:
            for rank_idx, predicted_location in enumerate(ranked_locations[:5], 1):
                match_result = check_location_match(
                    predicted_location,
                    question['file_path'],
                    question.get('function_name', ''),
                    question.get('line_number', 0)
                )
                match_result['rank'] = rank_idx
                match_result['location'] = predicted_location
                match_results.append(match_result)
        else:
            # For Category 2 and Category 3, create dummy match results (all false)
            for rank_idx, predicted_location in enumerate(ranked_locations[:5], 1):
                match_results.append({
                    'file_match': False,
                    'function_match': False,
                    'exact_match': False,
                    'rank': rank_idx,
                    'location': predicted_location
                })

        # Determine at which rank the correct answer was found
        exact_match_rank = None
        file_match_rank = None

        for match_result in match_results:
            if match_result['exact_match'] and exact_match_rank is None:
                exact_match_rank = match_result['rank']
            if match_result['file_match'] and file_match_rank is None:
                file_match_rank = match_result['rank']

        # Update position-based counters (exact match)
        if exact_match_rank == 1:
            found_at_rank1 += 1
        if exact_match_rank == 2:
            found_at_rank2 += 1
        if exact_match_rank == 3:
            found_at_rank3 += 1
        if exact_match_rank == 4:
            found_at_rank4 += 1
        if exact_match_rank == 5:
            found_at_rank5 += 1

        # Update top-N counters (exact match)
        if exact_match_rank is not None and exact_match_rank <= 2:
            found_in_top2 += 1
        if exact_match_rank is not None and exact_match_rank <= 3:
            found_in_top3 += 1
        if exact_match_rank is not None and exact_match_rank <= 4:
            found_in_top4 += 1
        if exact_match_rank is not None and exact_match_rank <= 5:
            found_in_top5 += 1

        # Update file match counters
        if file_match_rank == 1:
            file_match_at_rank1 += 1
        if file_match_rank == 2:
            file_match_at_rank2 += 1
        if file_match_rank == 3:
            file_match_at_rank3 += 1
        if file_match_rank == 4:
            file_match_at_rank4 += 1
        if file_match_rank == 5:
            file_match_at_rank5 += 1

        # Update top-N file match counters
        if file_match_rank is not None and file_match_rank <= 2:
            file_match_in_top2 += 1
        if file_match_rank is not None and file_match_rank <= 3:
            file_match_in_top3 += 1
        if file_match_rank is not None and file_match_rank <= 4:
            file_match_in_top4 += 1
        if file_match_rank is not None and file_match_rank <= 5:
            file_match_in_top5 += 1

        # Store result
        result = {
            'question_id': question['id'],
            'question': question['question'],
            'ground_truth_file': question.get('file_path', ''),
            'ground_truth_function': question.get('function_name', ''),
            'ground_truth_line': question.get('line_number', 0),
            'ranked_locations': ranked_locations,
            'match_results': match_results,
            'exact_match_rank': exact_match_rank,
            'file_match_rank': file_match_rank,
            'rerank_position': rerank_position,
            'rerank_file_position': rerank_file_position,
            'latency_seconds': latency,
            'found': response['found'],
            'llm_raw_response': response.get('raw_response', ''),
            'llm_prompt': response.get('llm_prompt', ''),  # Complete prompt sent to LLM
            'all_scores': response.get('all_scores', []),  # Include individual function scores
            # Category 2 specific fields
            'is_category_2': is_category_2,
            'exists': question.get('exists', True),
            'hallucinated': hallucinated if is_category_2 else None,
            'rationale': question.get('rationale', ''),
            # Category 3 (Deletion Experiment) specific fields
            'is_category_3': is_category_3,
            'deleted': question.get('deleted', False),
            'original_location': question.get('original_location', ''),
            'deletion_hallucination': deletion_hallucination if is_category_3 else None,
            'training_memory_interference': deleted_function_hallucination > 0 if is_category_3 else None
        }
        results.append(result)

        # Save result immediately to JSON file if output path is set
        if _CURRENT_OUTPUT_PATH:
            append_result_to_file(_CURRENT_OUTPUT_PATH, result)

        # Print result - different format for Category 2 and Category 3
        if is_category_2:
            # Category 2: Hallucination evaluation
            if hallucinated:
                status = "✗ HALLUCINATION"
                status_detail = "Model provided location for non-existent feature"
            else:
                status = "✓ CORRECT REFUSAL"
                status_detail = "Model correctly said 'not found'"

            print(f"  {status}: {status_detail}")
            print(f"  Rank 1: {shorten_path(ranked_locations[0])}")
            if question.get('rationale'):
                print(f"  Rationale: {question['rationale'][:80]}...")
        elif is_category_3:
            # Category 3: Deletion experiment evaluation
            rank1_location = ranked_locations[0] if ranked_locations else "NOT_FOUND"

            if true_negative > 0 or is_not_found_response(rank1_location):
                status = "✓ CORRECT REFUSAL"
                status_detail = "Model correctly said 'not found' for deleted function"
            elif deletion_hallucination:
                status = "✗ MEMORY INTERFERENCE"
                status_detail = "Model hallucinated DELETED function from training"
            else:
                status = "✗ FALSE POSITIVE"
                status_detail = "Model provided wrong location (not deleted function)"

            print(f"  {status}: {status_detail}")
            print(f"  Rank 1: {shorten_path(rank1_location)}")
            print(f"  Deleted Function: {question.get('function_name', '?')} from {shorten_path(question['file_path'])}")
        else:
            # Category 1: Standard evaluation
            status = "✓" if exact_match_rank else "✗"
            rank_info = f"@Rank{exact_match_rank}" if exact_match_rank else "Not in Top-5"

            # Re-Ranking status
            rerank_status = f"ReRank: @{rerank_position}" if rerank_position else "ReRank: Not in Top-40"
            rerank_file_status = f"(File: @{rerank_file_position})" if rerank_file_position else "(File: Not found)"

            print(f"  {status} LLM {rank_info} | {rerank_status} {rerank_file_status}")
            print(f"  Rank 1: {shorten_path(ranked_locations[0])}")
            print(f"  Rank 2: {shorten_path(ranked_locations[1])}")
            print(f"  Rank 3: {shorten_path(ranked_locations[2])}")
            print(f"  Rank 4: {shorten_path(ranked_locations[3])}")
            print(f"  Rank 5: {shorten_path(ranked_locations[4])}")

            # Ground truth with shortened path
            gt_path = f"{question['file_path']}:{question.get('function_name', question.get('line_number', '?'))}"
            print(f"  Ground Truth: {shorten_path(gt_path)}")

        # Show individual scores only for Individual Scoring mode (not Batch mode)
        if all_scores and not USE_BATCH_RANKING:
            print(f"  Top Scores: ", end="")
            for i, score_data in enumerate(all_scores[:3], 1):
                print(f"[{i}]={score_data['score']:.0f}% ", end="")
            print()

        print(f"  Latency: {latency:.2f}s\n")

    # Calculate metrics
    total_q = len(questions)
    avg_latency = total_latency / total_q

    # Category 1 questions count (for accuracy calculations)
    category_1_questions = total_q - category_2_questions
    # Use total_q if no Category 2 questions (backwards compatibility)
    accuracy_denominator = category_1_questions if category_1_questions > 0 else total_q

    metrics = {
        'model_name': model_name,
        'total_questions': total_q,
        'category_1_questions': category_1_questions,

        # LLM Selection metrics (correct file + function)
        # Note: Accuracy percentages based on Category 1 questions only (questions with ground truth)
        'found_at_rank1': found_at_rank1,
        'found_at_rank2': found_at_rank2,
        'found_at_rank3': found_at_rank3,
        'found_at_rank4': found_at_rank4,
        'found_at_rank5': found_at_rank5,
        'found_in_top2': found_in_top2,
        'found_in_top3': found_in_top3,
        'found_in_top4': found_in_top4,
        'found_in_top5': found_in_top5,
        'accuracy_rank1': found_at_rank1 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_rank2': found_at_rank2 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_rank3': found_at_rank3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_rank4': found_at_rank4 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_rank5': found_at_rank5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_top2': found_in_top2 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_top3': found_in_top3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_top4': found_in_top4 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'accuracy_top5': found_in_top5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,

        # LLM Selection file match metrics (correct file only)
        'file_match_at_rank1': file_match_at_rank1,
        'file_match_at_rank2': file_match_at_rank2,
        'file_match_at_rank3': file_match_at_rank3,
        'file_match_at_rank4': file_match_at_rank4,
        'file_match_at_rank5': file_match_at_rank5,
        'file_match_in_top2': file_match_in_top2,
        'file_match_in_top3': file_match_in_top3,
        'file_match_in_top4': file_match_in_top4,
        'file_match_in_top5': file_match_in_top5,
        'file_accuracy_rank1': file_match_at_rank1 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_rank2': file_match_at_rank2 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_rank3': file_match_at_rank3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_rank4': file_match_at_rank4 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_rank5': file_match_at_rank5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_top2': file_match_in_top2 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_top3': file_match_in_top3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_top4': file_match_in_top4 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'file_accuracy_top5': file_match_in_top5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,

        # Re-Ranking metrics (before LLM) - also based on Category 1 questions only
        'rerank_found_at_1': rerank_found_at_1,
        'rerank_found_at_3': rerank_found_at_3,
        'rerank_found_at_5': rerank_found_at_5,
        'rerank_found_at_10': rerank_found_at_10,
        'rerank_found_at_40': rerank_found_at_40,
        'rerank_accuracy_1': rerank_found_at_1 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_accuracy_3': rerank_found_at_3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_accuracy_5': rerank_found_at_5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_accuracy_10': rerank_found_at_10 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_accuracy_40': rerank_found_at_40 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,

        # Re-Ranking file match metrics
        'rerank_file_at_1': rerank_file_at_1,
        'rerank_file_at_3': rerank_file_at_3,
        'rerank_file_at_5': rerank_file_at_5,
        'rerank_file_at_10': rerank_file_at_10,
        'rerank_file_at_40': rerank_file_at_40,
        'rerank_file_accuracy_1': rerank_file_at_1 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_file_accuracy_3': rerank_file_at_3 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_file_accuracy_5': rerank_file_at_5 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_file_accuracy_10': rerank_file_at_10 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,
        'rerank_file_accuracy_40': rerank_file_at_40 / accuracy_denominator * 100 if accuracy_denominator > 0 else 0.0,

        # Latency
        'average_latency_seconds': avg_latency,
        'total_latency_seconds': total_latency,

        # Category 2 (Hallucination) metrics
        'category_2_questions': category_2_questions,
        'hallucination_count': hallucination_count,
        'correct_refusal_count': correct_refusal_count,
        'hallucination_rate': (hallucination_count / category_2_questions * 100) if category_2_questions > 0 else 0.0,
        'correct_refusal_rate': (correct_refusal_count / category_2_questions * 100) if category_2_questions > 0 else 0.0,

        # Category 3 (Deletion Experiment - RQ2) metrics
        'category_3_questions': category_3_questions,
        'deleted_function_hallucination': deleted_function_hallucination,  # Hallucinated EXACT deleted function
        'false_positive_other': false_positive_other,  # Provided wrong location (not deleted func)
        'true_negative': true_negative,  # Correctly said "not found"
        'rag_retrieval_contained_deleted': rag_retrieval_contained_deleted,  # RAG error
        'training_memory_interference_rate': (deleted_function_hallucination / category_3_questions * 100) if category_3_questions > 0 else 0.0,
        'false_positive_rate': (false_positive_other / category_3_questions * 100) if category_3_questions > 0 else 0.0,
        'true_negative_rate': (true_negative / category_3_questions * 100) if category_3_questions > 0 else 0.0
    }

    # Update metrics in the output file if output path is set
    if _CURRENT_OUTPUT_PATH:
        update_metrics_in_file(_CURRENT_OUTPUT_PATH, metrics)

    return {
        'metrics': metrics,
        'detailed_results': results
    }


def initialize_results_file(output_path: str, model_name: str):
    """Initialize results JSON file at the beginning of evaluation"""
    initial_data = {
        'evaluation_config': {
            'model_name': model_name,
            'use_llm': USE_LLM,
            'use_batch_ranking': USE_BATCH_RANKING if USE_LLM else None,
            'batch_ranking_size': BATCH_RANKING_SIZE if USE_LLM and USE_BATCH_RANKING else None,
            'test_questions_file': TEST_QUESTIONS_FILE,
            'max_questions': MAX_QUESTIONS,
            'timestamp': time.strftime("%Y%m%d_%H%M%S")
        },
        'detailed_results': [],
        'metrics': {}
    }
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(initial_data, f, indent=2, ensure_ascii=False)
    print(f"✓ Initialized results file: {output_path}\n")


def append_result_to_file(output_path: str, result: Dict):
    """Append a single result to the JSON file incrementally"""
    try:
        with open(output_path, 'r', encoding='utf-8') as f:
            data = json.load(f)

        data['detailed_results'].append(result)

        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
    except Exception as e:
        print(f"  Warning: Could not append result to file: {e}")


def update_metrics_in_file(output_path: str, metrics: Dict):
    """Update metrics in the JSON file at the end of evaluation"""
    try:
        with open(output_path, 'r', encoding='utf-8') as f:
            data = json.load(f)

        data['metrics'] = metrics

        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
        print(f"\n✓ Results saved to: {output_path}")
    except Exception as e:
        print(f"Warning: Could not update metrics in file: {e}")


def save_results(evaluation_data: Dict, output_path: str):
    """Save evaluation results to JSON file (legacy function, kept for compatibility)"""
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(evaluation_data, f, indent=2, ensure_ascii=False)
    print(f"\n✓ Results saved to: {output_path}")


def save_top10_questions(detailed_results: List[Dict], original_questions: List[Dict],
                         output_path: str, top_n: int = 10):
    """
    Save questions where the correct answer was found in top-N re-ranking results.
    Output format matches the test_questions.json format.

    Args:
        detailed_results: List of evaluation results for each question
        original_questions: Original test questions with ground truth
        output_path: Path to save the filtered questions
        top_n: Number of top results to consider (e.g., 1, 5, 10, 20)
    """
    topn_questions = []

    for result in detailed_results:
        # Check if correct answer was found in top-N of re-ranking
        if result.get('rerank_position') is not None and result['rerank_position'] <= top_n:
            # Find the original question by ID
            question_id = result['question_id']
            original_q = next((q for q in original_questions if q['id'] == question_id), None)

            if original_q:
                # Keep the original question structure
                topn_questions.append(original_q)

    # Save in the same format as test_questions.json
    output_data = {
        "questions": topn_questions
    }

    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(output_data, f, indent=2, ensure_ascii=False)

    print(f"\n✓ Top-{top_n} questions saved to: {output_path}")
    print(f"  {len(topn_questions)} questions found in top-{top_n} re-ranking results")


def print_summary(metrics: Dict):
    """Print summary of evaluation metrics with Top-5 ranking and Re-Ranking analysis"""
    print(f"\n{'='*80}")
    print(f"EVALUATION SUMMARY - {metrics['model_name'].upper()} MODEL")
    if USE_LLM:
        if USE_BATCH_RANKING:
            print(f"MODE: LLM Batch Selection ({BATCH_RANKING_SIZE} candidates, 1 call)")
        else:
            print(f"MODE: LLM Individual Scoring (10 candidates, 10 calls)")
    else:
        print(f"MODE: Re-Ranking Only (No LLM)")
    print(f"{'='*80}")
    print(f"Total Questions: {metrics['total_questions']}")
    print()

    print("=" * 80)
    print("RE-RANKING PERFORMANCE (Hybrid Search + Cross-Encoder, BEFORE LLM)")
    print("=" * 80)
    print("EXACT MATCH (Correct File + Function):")
    print(f"  Found at Position 1:  {metrics['rerank_found_at_1']:2d} ({metrics['rerank_accuracy_1']:5.1f}%)")
    print(f"  Found in Top-3:       {metrics['rerank_found_at_3']:2d} ({metrics['rerank_accuracy_3']:5.1f}%)")
    print(f"  Found in Top-5:       {metrics['rerank_found_at_5']:2d} ({metrics['rerank_accuracy_5']:5.1f}%)")
    print(f"  Found in Top-10:      {metrics['rerank_found_at_10']:2d} ({metrics['rerank_accuracy_10']:5.1f}%)")
    print(f"  Found in Top-40:      {metrics['rerank_found_at_40']:2d} ({metrics['rerank_accuracy_40']:5.1f}%)")
    print()
    print("FILE MATCH (Correct File Only):")
    print(f"  Found at Position 1:  {metrics['rerank_file_at_1']:2d} ({metrics['rerank_file_accuracy_1']:5.1f}%)")
    print(f"  Found in Top-3:       {metrics['rerank_file_at_3']:2d} ({metrics['rerank_file_accuracy_3']:5.1f}%)")
    print(f"  Found in Top-5:       {metrics['rerank_file_at_5']:2d} ({metrics['rerank_file_accuracy_5']:5.1f}%)")
    print(f"  Found in Top-10:      {metrics['rerank_file_at_10']:2d} ({metrics['rerank_file_accuracy_10']:5.1f}%)")
    print(f"  Found in Top-40:      {metrics['rerank_file_at_40']:2d} ({metrics['rerank_file_accuracy_40']:5.1f}%)")
    print()

    print("=" * 80)
    if USE_LLM:
        if USE_BATCH_RANKING:
            print(f"LLM SELECTION PERFORMANCE (Batch Ranking - {BATCH_RANKING_SIZE} candidates, 1 call)")
        else:
            print("LLM SELECTION PERFORMANCE (Individual Scoring - 10 calls)")
    else:
        print("RE-RANKING SELECTION PERFORMANCE (Top-5 Results)")
    print("=" * 80)
    print("EXACT MATCH (Correct File + Function):")
    print(f"  Found at Rank 1: {metrics['found_at_rank1']:2d} ({metrics['accuracy_rank1']:5.1f}%)")
    print(f"  Found at Rank 2: {metrics['found_at_rank2']:2d} ({metrics['accuracy_rank2']:5.1f}%)")
    print(f"  Found at Rank 3: {metrics['found_at_rank3']:2d} ({metrics['accuracy_rank3']:5.1f}%)")
    print(f"  Found at Rank 4: {metrics['found_at_rank4']:2d} ({metrics['accuracy_rank4']:5.1f}%)")
    print(f"  Found at Rank 5: {metrics['found_at_rank5']:2d} ({metrics['accuracy_rank5']:5.1f}%)")
    print()
    print(f"  Found in Top-2:  {metrics['found_in_top2']:2d} ({metrics['accuracy_top2']:5.1f}%)")
    print(f"  Found in Top-3:  {metrics['found_in_top3']:2d} ({metrics['accuracy_top3']:5.1f}%)")
    print(f"  Found in Top-4:  {metrics['found_in_top4']:2d} ({metrics['accuracy_top4']:5.1f}%)")
    print(f"  Found in Top-5:  {metrics['found_in_top5']:2d} ({metrics['accuracy_top5']:5.1f}%)")
    print()
    print("FILE MATCH (Correct File Only):")
    print(f"  Found at Rank 1: {metrics['file_match_at_rank1']:2d} ({metrics['file_accuracy_rank1']:5.1f}%)")
    print(f"  Found at Rank 2: {metrics['file_match_at_rank2']:2d} ({metrics['file_accuracy_rank2']:5.1f}%)")
    print(f"  Found at Rank 3: {metrics['file_match_at_rank3']:2d} ({metrics['file_accuracy_rank3']:5.1f}%)")
    print(f"  Found at Rank 4: {metrics['file_match_at_rank4']:2d} ({metrics['file_accuracy_rank4']:5.1f}%)")
    print(f"  Found at Rank 5: {metrics['file_match_at_rank5']:2d} ({metrics['file_accuracy_rank5']:5.1f}%)")
    print()
    print(f"  Found in Top-2:  {metrics['file_match_in_top2']:2d} ({metrics['file_accuracy_top2']:5.1f}%)")
    print(f"  Found in Top-3:  {metrics['file_match_in_top3']:2d} ({metrics['file_accuracy_top3']:5.1f}%)")
    print(f"  Found in Top-4:  {metrics['file_match_in_top4']:2d} ({metrics['file_accuracy_top4']:5.1f}%)")
    print(f"  Found in Top-5:  {metrics['file_match_in_top5']:2d} ({metrics['file_accuracy_top5']:5.1f}%)")
    print()

    # Category 2 (Hallucination) metrics - only show if there are Category 2 questions
    if metrics['category_2_questions'] > 0:
        print("=" * 80)
        print("HALLUCINATION RESISTANCE (Category 2: Non-Existent Functionality)")
        print("=" * 80)
        print(f"  Total Category 2 Questions: {metrics['category_2_questions']}")
        print(f"  Hallucinations (provided location): {metrics['hallucination_count']} ({metrics['hallucination_rate']:5.1f}%)")
        print(f"  Correct Refusals (said 'not found'): {metrics['correct_refusal_count']} ({metrics['correct_refusal_rate']:5.1f}%)")
        print()
        print(f"  📊 Hallucination Rate: {metrics['hallucination_rate']:.1f}%")
        print(f"  ✓  Correct Refusal Rate: {metrics['correct_refusal_rate']:.1f}%")
        print()

    # Category 3 (Deletion Experiment - RQ2) metrics - only show if there are Category 3 questions
    if metrics['category_3_questions'] > 0:
        print("=" * 80)
        print("DELETION EXPERIMENT (Category 3: RQ2 - Training Memory Interference)")
        print("=" * 80)
        print(f"  Total Category 3 Questions (Deleted Functions): {metrics['category_3_questions']}")
        print()
        print(f"  ✓  True Negatives (correctly said 'not found'): {metrics['true_negative']} ({metrics['true_negative_rate']:5.1f}%)")
        print(f"  ✗  Memory Interference (hallucinated DELETED function): {metrics['deleted_function_hallucination']} ({metrics['training_memory_interference_rate']:5.1f}%)")
        print(f"  ✗  False Positives (wrong location, not deleted func): {metrics['false_positive_other']} ({metrics['false_positive_rate']:5.1f}%)")
        print()
        print(f"  📊 Training Memory Interference Rate: {metrics['training_memory_interference_rate']:.1f}%")
        print(f"  📊 True Negative Rate (Adaptation): {metrics['true_negative_rate']:.1f}%")
        print()
        if metrics['rag_retrieval_contained_deleted'] > 0:
            print(f"  ⚠️  WARNING: RAG retrieved {metrics['rag_retrieval_contained_deleted']} deleted function(s)")
            print(f"     This suggests incomplete deletion or incorrect re-indexing!")
            print()

    print("=" * 80)
    print("PERFORMANCE:")
    print(f"  Average Response Latency: {metrics['average_latency_seconds']:.2f}s")
    print(f"  Total Time: {metrics['total_latency_seconds']:.2f}s")
    print(f"{'='*80}\n")


def main():
    global _CURRENT_OUTPUT_PATH
    import sys
    import os

    # Paths
    script_dir = os.path.dirname(os.path.abspath(__file__))

    # Handle both relative and absolute paths for test questions file
    if os.path.isabs(TEST_QUESTIONS_FILE):
        questions_path = TEST_QUESTIONS_FILE
    else:
        questions_path = os.path.join(script_dir, TEST_QUESTIONS_FILE)

    output_dir = os.path.join(script_dir, "evaluation_results")
    top10_dir = os.path.join(script_dir, "top10_questions")

    # Create output directories
    os.makedirs(output_dir, exist_ok=True)
    os.makedirs(top10_dir, exist_ok=True)

    # Use model choice from rag_chat.py
    print("=" * 70)
    print("DeepSeek-Coder RAG Evaluation")
    print("=" * 70)

    # Display evaluation mode
    print(f"\nEVALUATION MODE:")
    if USE_LLM:
        print(f"   Using LLM for function selection")
        print(f"\nMODEL CONFIGURATION:")
        print(f"   Model Size: {MODEL_CHOICE.upper()}")
        if MODEL_CHOICE == "1.3b":
            print("   Architecture: DeepSeek-Coder-1.3B (Float16, ~3 GB VRAM)")
            model_name = "deepseek-coder-1.3b-instruct"
        elif MODEL_CHOICE == "6.7b":
            print("   Architecture: DeepSeek-Coder-6.7B (4-bit quantized, ~5-6 GB VRAM)")
            model_name = "deepseek-coder-6.7b-instruct-gptq"
        else:
            print(f"   Unknown model: {MODEL_CHOICE}")
            model_name = f"deepseek-coder-{MODEL_CHOICE}"

        # Show model type
        if USE_FINETUNED:
            print(f"   Type: FINE-TUNED MODEL (with LoRA adapters)")
            print(f"   Adapters: {FINETUNED_MODEL_PATH}")
            model_name = model_name + "-finetuned"  # Add suffix to filename
        else:
            print("   Type: BASE MODEL (standard pre-trained)")

        # Show LLM ranking mode
        print(f"\n📊 LLM RANKING MODE:")
        if USE_BATCH_RANKING:
            print(f"   Mode: BATCH RANKING (1 LLM call for all candidates)")
            print(f"   Candidates: Top-{BATCH_RANKING_SIZE} from reranker")
            print(f"   Speed: ~10x faster than individual scoring")
            print(f"   Selection Strategy: {LLM_SELECTION_MODE}")
            model_name = model_name + "-batch-" + LLM_SELECTION_MODE  # Add suffix to filename with mode
        else:
            print(f"   Mode: INDIVIDUAL SCORING (10 separate LLM calls)")
            print(f"   Candidates: Top-10 from reranker")
            print(f"   Speed: Slower, legacy mode")
            model_name = model_name + "-individual"  # Add suffix to filename

        print("\n   💡 To switch modes: Change USE_BATCH_RANKING in rag_chat.py line 19")
        print("      USE_BATCH_RANKING = True   →  Batch Mode (FAST, recommended)")
        print("      USE_BATCH_RANKING = False  →  Individual Mode (SLOW, legacy)")
        print("\n   💡 To change candidate count: Change BATCH_RANKING_SIZE in rag_chat.py line 22")
        print("      Recommended: 5-10 candidates for best speed/accuracy balance")
    else:
        print(f"   ✓ Re-Ranking Only (No LLM) - Fast baseline evaluation")
        print(f"   ✓ Using Hybrid Search + Cross-Encoder Re-Ranking")
        model_name = "reranking-only"

    print(f"\n   💡 To switch modes: Change USE_LLM in evaluate.py line 13")
    print(f"      USE_LLM = True   →  LLM Individual Scoring (slower, more accurate)")
    print(f"      USE_LLM = False  →  Re-Ranking Only (faster, baseline)")

    # Display indexing mode
    print(f"\n📦 INDEXING MODE:")
    if USE_FULLCODE and USE_DOCSTRING_ONLY:
        print(f"   ✓ Comparing FULL CODE vs DOCSTRING-ONLY")
    elif USE_FULLCODE:
        print(f"   ✓ Using FULL CODE only (complete function code in embeddings)")
    elif USE_DOCSTRING_ONLY:
        print(f"   ✓ Using DOCSTRING-ONLY (signature + docstring in embeddings)")
    else:
        print(f"   ⚠ ERROR: Both USE_FULLCODE and USE_DOCSTRING_ONLY are False!")

    print(f"\n   💡 To change indexing mode: Change USE_FULLCODE / USE_DOCSTRING_ONLY in evaluate.py lines 18-21")
    print(f"      USE_FULLCODE=True,  USE_DOCSTRING_ONLY=False  →  Full Code Only")
    print(f"      USE_FULLCODE=False, USE_DOCSTRING_ONLY=True   →  Docstring Only")
    print(f"      USE_FULLCODE=True,  USE_DOCSTRING_ONLY=True   →  Compare Both")

    # Display database mode
    print(f"\n💾 DATABASE MODE:")
    if USE_FRESH_INDEX:
        print(f"   ✓ DELETE existing index and re-index from scratch")
        print(f"   ⚠ Warning: This will delete all cached embeddings!")
    else:
        print(f"   ✓ Use existing index if available (recommended)")

    print(f"\n   💡 To change database mode: Change USE_FRESH_INDEX in evaluate.py line 28")
    print(f"      USE_FRESH_INDEX=True   →  Force re-indexing (slower)")
    print(f"      USE_FRESH_INDEX=False  →  Use cached index (faster)")

    # Display top-N questions export configuration
    print(f"\n📤 TOP-N QUESTIONS EXPORT:")
    if SAVE_TOP_N_QUESTIONS is not None:
        print(f"   ✓ Save top-{SAVE_TOP_N_QUESTIONS} questions to separate file")
        print(f"   ✓ Questions where correct answer was found in top-{SAVE_TOP_N_QUESTIONS} re-ranking results")
    else:
        print(f"   ✓ Top-N export DISABLED")

    print(f"\n   💡 To change top-N export: Change SAVE_TOP_N_QUESTIONS in evaluate.py line 20")
    print(f"      SAVE_TOP_N_QUESTIONS=1   →  Export only rank 1 questions")
    print(f"      SAVE_TOP_N_QUESTIONS=5   →  Export top-5 questions")
    print(f"      SAVE_TOP_N_QUESTIONS=10  →  Export top-10 questions")
    print(f"      SAVE_TOP_N_QUESTIONS=20  →  Export top-20 questions")
    print(f"      SAVE_TOP_N_QUESTIONS=None  →  Disable export")
    print()

    # Validate SAVE_TOP_N_QUESTIONS configuration
    if SAVE_TOP_N_QUESTIONS is not None and SAVE_TOP_N_QUESTIONS not in [1, 5, 10, 20]:
        print(f"\n❌ ERROR: Invalid SAVE_TOP_N_QUESTIONS value: {SAVE_TOP_N_QUESTIONS}")
        print("   Valid options: 1, 5, 10, 20, or None to disable")
        print("   Please update the value in evaluate.py line 20")
        return

    # Load test questions
    print(f"\n📋 Test Questions:")
    print(f"   File: {TEST_QUESTIONS_FILE}")
    questions = load_test_questions(questions_path)

    # Limit questions if MAX_QUESTIONS is set
    if MAX_QUESTIONS is not None and MAX_QUESTIONS < len(questions):
        questions = questions[:MAX_QUESTIONS]
        print(f"✓ Loaded {len(questions)} test questions (limited from 100 by MAX_QUESTIONS={MAX_QUESTIONS})\n")
    else:
        print(f"✓ Loaded {len(questions)} test questions\n")

    # Validate configuration
    if not USE_FULLCODE and not USE_DOCSTRING_ONLY:
        print("\n❌ ERROR: Both USE_FULLCODE and USE_DOCSTRING_ONLY are False!")
        print("   Please set at least one to True in evaluate.py lines 18-21")
        return

    # Check which indexing mode to use
    if USE_FULLCODE and USE_DOCSTRING_ONLY:
        print("\n" + "="*70)
        print("COMPARISON MODE: Evaluating FULL CODE vs DOCSTRING-ONLY")
        print("="*70 + "\n")

        # Determine codebase path
        codebase_path = os.path.join(os.path.dirname(__file__), "codebase_enriched")
        if not os.path.exists(codebase_path):
            print(f"\nERROR: Codebase directory not found: {codebase_path}")
            print("Please ensure the codebase directory exists.")
            return

        fullcode_results = None
        docstring_results = None

        # Create timestamp for both evaluations
        timestamp = time.strftime("%Y%m%d_%H%M%S")

        # PHASE 1: Full Code Mode (if enabled)
        if USE_FULLCODE:
            print("="*70)
            print("PHASE 1: FULL CODE MODE EVALUATION")
            print("="*70 + "\n")

            print("Initializing Full Code RAG system...")
            rag_fullcode = ImprovedRAGSystem(use_docstring_only=False, reset_database=USE_FRESH_INDEX)

            if rag_fullcode.collection.count() == 0 or USE_FRESH_INDEX:
                print("\nIndexing codebase (Full Code)...")
                rag_fullcode.index_codebase(codebase_path)
                print(f"✓ Indexing complete: {rag_fullcode.collection.count()} chunks indexed\n")
            else:
                print(f"✓ Using existing Full Code index: {rag_fullcode.collection.count()} chunks\n")

            # Load model only if USE_LLM is True
            if USE_LLM:
                print("Loading LLM for Full Code evaluation...")
                model_fullcode, tokenizer_fullcode = load_model(MODEL_CHOICE, USE_FINETUNED, FINETUNED_MODEL_PATH)
            else:
                print("Skipping LLM loading (Re-Ranking only mode)...")
                model_fullcode, tokenizer_fullcode = None, None

            # Set up output path and initialize results file
            fullcode_output = os.path.join(output_dir, f"evaluation_{model_name}-fullcode_{timestamp}.json")
            _CURRENT_OUTPUT_PATH = fullcode_output
            initialize_results_file(fullcode_output, f"{model_name}-fullcode")

            # Evaluate Full Code
            fullcode_results = evaluate_model(model_fullcode, tokenizer_fullcode, rag_fullcode, questions,
                                              model_name=f"{model_name}-fullcode")

            # Free memory
            if USE_LLM:
                del model_fullcode
                torch.cuda.empty_cache()

        # PHASE 2: Docstring-Only Mode (if enabled)
        if USE_DOCSTRING_ONLY:
            print("\n" + "="*70)
            print("PHASE 2: DOCSTRING-ONLY MODE EVALUATION")
            print("="*70 + "\n")

            print("Initializing Docstring-Only RAG system...")
            rag_docstring = ImprovedRAGSystem(use_docstring_only=True, reset_database=USE_FRESH_INDEX)

            if rag_docstring.collection.count() == 0 or USE_FRESH_INDEX:
                print("\nIndexing codebase (Docstring-Only)...")
                rag_docstring.index_codebase(codebase_path)
                print(f"✓ Indexing complete: {rag_docstring.collection.count()} chunks indexed\n")
            else:
                print(f"✓ Using existing Docstring-Only index: {rag_docstring.collection.count()} chunks\n")

            # Load model only if USE_LLM is True
            if USE_LLM:
                print("Loading LLM for Docstring-Only evaluation...")
                model_docstring, tokenizer_docstring = load_model(MODEL_CHOICE, USE_FINETUNED, FINETUNED_MODEL_PATH)
            else:
                model_docstring, tokenizer_docstring = None, None

            # Set up output path and initialize results file
            docstring_output = os.path.join(output_dir, f"evaluation_{model_name}-docstring_{timestamp}.json")
            _CURRENT_OUTPUT_PATH = docstring_output
            initialize_results_file(docstring_output, f"{model_name}-docstring")

            # Evaluate Docstring-Only
            docstring_results = evaluate_model(model_docstring, tokenizer_docstring, rag_docstring, questions,
                                               model_name=f"{model_name}-docstring")

        # Results are already saved incrementally during evaluation

        if fullcode_results:
            # fullcode_output already defined above

            # Save top-N questions for Full Code mode (if enabled)
            if SAVE_TOP_N_QUESTIONS is not None:
                top10_fullcode_output = os.path.join(top10_dir, f"top{SAVE_TOP_N_QUESTIONS}_questions_fullcode_{timestamp}.json")
                save_top10_questions(fullcode_results['detailed_results'], questions, top10_fullcode_output, SAVE_TOP_N_QUESTIONS)

        if docstring_results:
            # docstring_output already defined above
            # Save top-N questions for Docstring mode (if enabled)
            if SAVE_TOP_N_QUESTIONS is not None:
                top10_docstring_output = os.path.join(top10_dir, f"top{SAVE_TOP_N_QUESTIONS}_questions_docstring_{timestamp}.json")
                save_top10_questions(docstring_results['detailed_results'], questions, top10_docstring_output, SAVE_TOP_N_QUESTIONS)

        # Print summaries
        print("\n" + "="*70)
        print("EVALUATION SUMMARY")
        print("="*70)

        if fullcode_results:
            print_summary(fullcode_results['metrics'])

        if docstring_results:
            if fullcode_results:
                print("\n")
            print_summary(docstring_results['metrics'])

        # Print comparison only if both modes were evaluated
        if fullcode_results and docstring_results:
            print("\n" + "="*70)
            print("QUICK COMPARISON: FULL CODE vs DOCSTRING-ONLY")
            print("="*70)
            print(f"{'Metric':<40} {'FULL CODE':<15} {'DOCSTRING':<15} {'Δ':<10}")
            print("-"*70)

            metrics_to_compare = [
                ('accuracy_rank1', 'Accuracy@Rank1 (%)'),
                ('accuracy_top3', 'Accuracy@Top3 (%)'),
                ('accuracy_top5', 'Accuracy@Top5 (%)'),
                ('rerank_accuracy_1', 'Re-Ranking@1 (%)'),
                ('rerank_accuracy_5', 'Re-Ranking@5 (%)'),
                ('rerank_accuracy_10', 'Re-Ranking@10 (%)'),
                ('average_latency_seconds', 'Avg Latency (s)'),
            ]

            for metric_key, metric_label in metrics_to_compare:
                fullcode_val = fullcode_results['metrics'][metric_key]
                docstring_val = docstring_results['metrics'][metric_key]
                delta = docstring_val - fullcode_val
                delta_str = f"+{delta:.2f}" if delta >= 0 else f"{delta:.2f}"

                print(f"{metric_label:<40} {fullcode_val:<15.2f} {docstring_val:<15.2f} {delta_str:<10}")

            print("="*70)

        print(f"\n✓ Evaluation complete!")
        if fullcode_results:
            print(f"  Full Code results: {fullcode_output}")
        if docstring_results:
            print(f"  Docstring results: {docstring_output}")

        return

    # Single mode evaluation (only one mode enabled)
    # Determine which mode to use
    use_docstring = USE_DOCSTRING_ONLY  # If only docstring is enabled, use it
    mode_label = "Docstring-Only" if use_docstring else "Full Code"

    print(f"Initializing RAG system ({mode_label} mode)...")
    rag = ImprovedRAGSystem(use_docstring_only=use_docstring, reset_database=USE_FRESH_INDEX)

    # Check if indexed, if not: index automatically
    if rag.collection.count() == 0 or USE_FRESH_INDEX:
        print("\nIndexing codebase...")
        codebase_path = os.path.join(os.path.dirname(__file__), "codebase")

        if not os.path.exists(codebase_path):
            print(f"\nERROR: Codebase directory not found: {codebase_path}")
            print("Please ensure the codebase directory exists.")
            return

        rag.index_codebase(codebase_path)
        print(f"✓ Indexing complete: {rag.collection.count()} chunks indexed\n")
    else:
        print(f"✓ Using existing index: {rag.collection.count()} chunks\n")

    # Load model only if USE_LLM is True
    if USE_LLM:
        print("Loading LLM...")
    else:
        print("Skipping LLM loading (Re-Ranking only mode)...")

    # Check if we should compare both models (for RQ1)
    compare_mode = os.getenv('COMPARE_MODELS', 'false').lower() == 'true'

    if compare_mode and USE_LLM:
        print("\n" + "="*70)
        print("COMPARISON MODE: Evaluating BASE vs FINE-TUNED models")
        print("="*70 + "\n")

        # Create timestamp for both evaluations
        timestamp = time.strftime("%Y%m%d_%H%M%S")
        base_output_path = os.path.join(output_dir, f"evaluation_{model_name}-base_{timestamp}.json")
        finetuned_output_path = os.path.join(output_dir, f"evaluation_{model_name}-finetuned_{timestamp}.json")

        # Evaluate BASE model
        print("="*70)
        print("PHASE 1: BASE MODEL EVALUATION")
        print("="*70 + "\n")

        _CURRENT_OUTPUT_PATH = base_output_path
        initialize_results_file(base_output_path, f"{model_name}-base")

        base_model, base_tokenizer = load_model(MODEL_CHOICE, use_finetuned=False)
        base_results = evaluate_model(base_model, base_tokenizer, rag, questions,
                                       model_name=f"{model_name}-base")

        # Free memory
        del base_model
        torch.cuda.empty_cache()

        # Evaluate FINE-TUNED model
        print("\n" + "="*70)
        print("PHASE 2: FINE-TUNED MODEL EVALUATION")
        print("="*70 + "\n")

        _CURRENT_OUTPUT_PATH = finetuned_output_path
        initialize_results_file(finetuned_output_path, f"{model_name}-finetuned")

        finetuned_path = os.getenv('FINETUNED_MODEL_PATH', './finetuned_model')
        finetuned_model, finetuned_tokenizer = load_model(MODEL_CHOICE, use_finetuned=True,
                                                           finetuned_path=finetuned_path)
        finetuned_results = evaluate_model(finetuned_model, finetuned_tokenizer, rag, questions,
                                           model_name=f"{model_name}-finetuned")

        # Results are already saved incrementally during evaluation

        # Save top-N questions for both models (if enabled)
        if SAVE_TOP_N_QUESTIONS is not None:
            top10_base_output = os.path.join(top10_dir, f"top{SAVE_TOP_N_QUESTIONS}_questions_base_{timestamp}.json")
            top10_finetuned_output = os.path.join(top10_dir, f"top{SAVE_TOP_N_QUESTIONS}_questions_finetuned_{timestamp}.json")
            save_top10_questions(base_results['detailed_results'], questions, top10_base_output, SAVE_TOP_N_QUESTIONS)
            save_top10_questions(finetuned_results['detailed_results'], questions, top10_finetuned_output, SAVE_TOP_N_QUESTIONS)

        # Print comparison
        print("\n" + "="*70)
        print("COMPARISON SUMMARY")
        print("="*70)
        print_summary(base_results['metrics'])
        print("\n")
        print_summary(finetuned_results['metrics'])

        print("\n" + "="*70)
        print("QUICK COMPARISON")
        print("="*70)
        print(f"{'Metric':<40} {'BASE':<15} {'FINE-TUNED':<15} {'Δ':<10}")
        print("-"*70)

        metrics_to_compare = [
            ('accuracy_rank1', 'Accuracy@Rank1 (%)'),
            ('accuracy_top3', 'Accuracy@Top3 (%)'),
            ('rerank_accuracy_5', 'Re-Ranking@5 (%)'),
            ('average_latency_seconds', 'Avg Latency (s)'),
        ]

        for metric_key, metric_label in metrics_to_compare:
            base_val = base_results['metrics'][metric_key]
            ft_val = finetuned_results['metrics'][metric_key]
            delta = ft_val - base_val
            delta_str = f"+{delta:.2f}" if delta >= 0 else f"{delta:.2f}"

            print(f"{metric_label:<40} {base_val:<15.2f} {ft_val:<15.2f} {delta_str:<10}")

        print("="*70)

    else:
        # Single model evaluation (original behavior)
        if USE_LLM:
            # Use USE_FINETUNED from rag_chat.py to decide which model to load
            model, tokenizer = load_model(MODEL_CHOICE, USE_FINETUNED, FINETUNED_MODEL_PATH)
        else:
            # No LLM needed - use dummy model/tokenizer
            model, tokenizer = None, None

        # Set up output path and initialize results file
        timestamp = time.strftime("%Y%m%d_%H%M%S")
        output_path = os.path.join(output_dir, f"evaluation_{model_name}_{timestamp}.json")

        _CURRENT_OUTPUT_PATH = output_path
        initialize_results_file(output_path, model_name)

        # Evaluate
        results = evaluate_model(model, tokenizer, rag, questions, model_name=model_name)

        # Print summary
        print_summary(results['metrics'])

        # Results are already saved incrementally during evaluation

        # Save top-N questions (if enabled)
        if SAVE_TOP_N_QUESTIONS is not None:
            top10_output = os.path.join(top10_dir, f"top{SAVE_TOP_N_QUESTIONS}_questions_{model_name}_{timestamp}.json")
            save_top10_questions(results['detailed_results'], questions, top10_output, SAVE_TOP_N_QUESTIONS)

        print(f"\n✓ Evaluation complete!")
        print(f"  Results saved to: {output_path}")
        if SAVE_TOP_N_QUESTIONS is not None:
            print(f"  Top-{SAVE_TOP_N_QUESTIONS} questions saved to: {top10_output}")

    return


if __name__ == "__main__":
    main()
