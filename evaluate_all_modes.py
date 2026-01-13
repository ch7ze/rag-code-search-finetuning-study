"""
Multi-Mode Evaluation Script
Tests all LLM selection modes on all datasets automatically
"""

import subprocess
import os
import sys
import json
import time

# ============================================================================
# CONFIGURATION
# ============================================================================

# Deletion Experiment Mode (RQ2)
DELETION_EXPERIMENT = True  # True = RQ2 deletion experiment, False = standard evaluation

# Modes to test
MODES_TO_TEST = ["conservative", "aggressive", "aggressive_no_fewshot"]

# Datasets to test
if DELETION_EXPERIMENT:
    # RQ2: Test only deletion experiment dataset
    DATASETS_TO_TEST = [
        "test_questions_rq2.json"
    ]
else:
    # Standard evaluation datasets
    DATASETS_TO_TEST = [
        "test_questions_category2.json",
        "top10_questions/top5_questions_reranking-only_20251124_145425.json"
    ]

# Questions per dataset
MAX_QUESTIONS = None

# ============================================================================
# HELPER FUNCTIONS
# ============================================================================

def set_mode_in_rag_chat(mode):
    """Modify rag_chat.py to set LLM_SELECTION_MODE"""
    rag_chat_path = os.path.join(os.path.dirname(__file__), "rag_chat.py")

    with open(rag_chat_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Find and replace the LLM_SELECTION_MODE line
    lines = content.split('\n')
    for i, line in enumerate(lines):
        if line.startswith('LLM_SELECTION_MODE ='):
            lines[i] = f'LLM_SELECTION_MODE = "{mode}"'
            break

    with open(rag_chat_path, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines))

    print(f"  [OK] Set LLM_SELECTION_MODE = \"{mode}\" in rag_chat.py")

def set_dataset_in_evaluate(dataset, max_questions):
    """Modify evaluate.py to set TEST_QUESTIONS_FILE and MAX_QUESTIONS"""
    evaluate_path = os.path.join(os.path.dirname(__file__), "evaluate.py")

    with open(evaluate_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Find and replace the TEST_QUESTIONS_FILE and MAX_QUESTIONS lines
    lines = content.split('\n')
    for i, line in enumerate(lines):
        if line.startswith('TEST_QUESTIONS_FILE =') and not line.startswith('#'):
            lines[i] = f'TEST_QUESTIONS_FILE = "{dataset}"'
        elif line.startswith('MAX_QUESTIONS =') and not line.startswith('#'):
            lines[i] = f'MAX_QUESTIONS = {max_questions}'

    with open(evaluate_path, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines))

    dataset_name = os.path.basename(dataset)
    print(f"  [OK] Set TEST_QUESTIONS_FILE = \"{dataset_name}\" in evaluate.py")
    print(f"  [OK] Set MAX_QUESTIONS = {max_questions} in evaluate.py")

def run_evaluation():
    """Run evaluate.py and capture output"""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    python_exe = os.path.join(script_dir, "venv", "Scripts", "python.exe")
    evaluate_script = os.path.join(script_dir, "evaluate.py")

    result = subprocess.run(
        [python_exe, evaluate_script],
        capture_output=True,
        text=True,
        cwd=script_dir,
        encoding='utf-8',
        errors='replace'  # Replace problematic characters instead of crashing
    )

    return result.stdout, result.stderr, result.returncode

def extract_metrics_from_output(output):
    """Extract key metrics from evaluation output"""
    metrics = {}

    # Handle None output
    if output is None:
        return metrics

    # Look for hallucination metrics (Category 2)
    if "HALLUCINATION RESISTANCE" in output:
        for line in output.split('\n'):
            if "Hallucination Rate:" in line:
                rate = line.split(':')[1].strip().replace('%', '')
                metrics['hallucination_rate'] = float(rate)
            elif "Correct Refusal Rate:" in line:
                rate = line.split(':')[1].strip().replace('%', '')
                metrics['correct_refusal_rate'] = float(rate)

    # Look for accuracy metrics (Top-5)
    for line in output.split('\n'):
        if "Found at Rank 1:" in line and "(" in line:
            # Extract percentage from "Found at Rank 1:  8 ( 80.0%)"
            percent_str = line.split('(')[1].split('%')[0].strip()
            metrics['rank1_accuracy'] = float(percent_str)
        elif "Found in Top-5:" in line and "(" in line:
            percent_str = line.split('(')[1].split('%')[0].strip()
            metrics['top5_accuracy'] = float(percent_str)
        elif "Average Response Latency:" in line:
            latency = line.split(':')[1].strip().replace('s', '')
            metrics['avg_latency'] = float(latency)

    return metrics

# ============================================================================
# MAIN EXECUTION
# ============================================================================

def main():
    print("=" * 80)
    if DELETION_EXPERIMENT:
        print("MULTI-MODE EVALUATION - RQ2 DELETION EXPERIMENT")
        print("Testing training memory interference on deleted functions")
    else:
        print("MULTI-MODE EVALUATION")
        print("Testing all LLM selection modes on all datasets")
    print("=" * 80)
    print(f"\nModes: {MODES_TO_TEST}")
    print(f"Datasets: {len(DATASETS_TO_TEST)}")
    if DELETION_EXPERIMENT:
        print(f"Dataset: test_questions_rq2.json (13 deleted functions)")
    print(f"Questions per dataset: {MAX_QUESTIONS if MAX_QUESTIONS else 'All'}")
    print(f"Total tests: {len(MODES_TO_TEST) * len(DATASETS_TO_TEST)}")
    print()

    # Store all results
    all_results = {}

    test_num = 0
    total_tests = len(MODES_TO_TEST) * len(DATASETS_TO_TEST)

    for dataset in DATASETS_TO_TEST:
        dataset_name = os.path.basename(dataset).replace('.json', '')
        all_results[dataset_name] = {}

        for mode in MODES_TO_TEST:
            test_num += 1

            print("\n" + "=" * 80)
            print(f"TEST {test_num}/{total_tests}: {mode.upper()} on {dataset_name}")
            print("=" * 80)

            # Configure files
            set_mode_in_rag_chat(mode)
            set_dataset_in_evaluate(dataset, MAX_QUESTIONS)

            print(f"\n  Running evaluation...")
            start_time = time.time()

            # Run evaluation
            stdout, stderr, returncode = run_evaluation()

            elapsed = time.time() - start_time

            if returncode != 0:
                print(f"  [ERROR] Evaluation failed!")
                print(f"  Error: {stderr}")
                all_results[dataset_name][mode] = {"error": stderr}
            else:
                print(f"  [OK] Evaluation completed in {elapsed:.1f}s")

                # Extract metrics
                metrics = extract_metrics_from_output(stdout)
                all_results[dataset_name][mode] = metrics

                # Print quick summary
                if 'hallucination_rate' in metrics:
                    print(f"  → Hallucination Rate: {metrics['hallucination_rate']:.1f}%")
                    print(f"  → Correct Refusals: {metrics['correct_refusal_rate']:.1f}%")
                if 'rank1_accuracy' in metrics:
                    print(f"  → Rank 1 Accuracy: {metrics['rank1_accuracy']:.1f}%")
                if 'avg_latency' in metrics:
                    print(f"  → Avg Latency: {metrics['avg_latency']:.2f}s")

    # ============================================================================
    # FINAL COMPARISON TABLE
    # ============================================================================

    print("\n\n" + "=" * 80)
    print("FINAL COMPARISON - ALL MODES ON ALL DATASETS")
    print("=" * 80)

    for dataset_name, modes_results in all_results.items():
        print(f"\n### {dataset_name}")
        print("-" * 80)

        # Determine which metrics to show based on dataset
        is_category2 = 'category2' in dataset_name.lower()

        if is_category2:
            # Show hallucination metrics
            print(f"{'Mode':<30} {'Hallucination Rate':<20} {'Correct Refusals':<20}")
            print("-" * 70)
            for mode, metrics in modes_results.items():
                if 'error' in metrics:
                    print(f"{mode:<30} ERROR")
                elif 'hallucination_rate' in metrics:
                    hall_rate = f"{metrics['hallucination_rate']:.1f}%"
                    refusal_rate = f"{metrics['correct_refusal_rate']:.1f}%"
                    print(f"{mode:<30} {hall_rate:<20} {refusal_rate:<20}")
        else:
            # Show accuracy metrics
            print(f"{'Mode':<30} {'Rank 1 Accuracy':<20} {'Top-5 Accuracy':<20} {'Avg Latency':<15}")
            print("-" * 85)
            for mode, metrics in modes_results.items():
                if 'error' in metrics:
                    print(f"{mode:<30} ERROR")
                elif 'rank1_accuracy' in metrics:
                    rank1 = f"{metrics['rank1_accuracy']:.1f}%"
                    top5 = f"{metrics.get('top5_accuracy', 0):.1f}%"
                    latency = f"{metrics.get('avg_latency', 0):.2f}s"
                    print(f"{mode:<30} {rank1:<20} {top5:<20} {latency:<15}")

    # ============================================================================
    # SAVE RESULTS TO JSON
    # ============================================================================

    # Add 'rq2_deletion' suffix if DELETION_EXPERIMENT is active
    if DELETION_EXPERIMENT:
        output_file = f"multi_mode_results_rq2_deletion_{time.strftime('%Y%m%d_%H%M%S')}.json"
    else:
        output_file = f"multi_mode_results_{time.strftime('%Y%m%d_%H%M%S')}.json"

    with open(output_file, 'w') as f:
        json.dump(all_results, f, indent=2)

    print(f"\n\n[OK] All tests completed!")
    print(f"  Results saved to: {output_file}")
    print()

if __name__ == "__main__":
    main()
