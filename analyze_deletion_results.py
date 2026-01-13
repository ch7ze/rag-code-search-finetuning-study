"""
Analysis script for deletion experiment (RQ2) results.

Compares base model vs fine-tuned model behavior on deleted functions
to test the hypothesis about training memory interference.

Statistical tests:
- McNemar's test for paired binary outcomes
- Cohen's h for effect size
"""

import json
import argparse
from pathlib import Path
from typing import Dict, List, Tuple
import sys


def load_evaluation_results(file_path: str) -> Dict:
    """Load evaluation results from JSON file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        return json.load(f)


def extract_deletion_metrics(results: Dict) -> Dict:
    """Extract Category 3 (deletion experiment) metrics from results."""
    metrics = results.get('evaluation_metrics', results)

    return {
        'model_name': metrics.get('model_name', 'unknown'),
        'category_3_questions': metrics.get('category_3_questions', 0),
        'true_negative': metrics.get('true_negative', 0),
        'deleted_function_hallucination': metrics.get('deleted_function_hallucination', 0),
        'false_positive_other': metrics.get('false_positive_other', 0),
        'true_negative_rate': metrics.get('true_negative_rate', 0.0),
        'training_memory_interference_rate': metrics.get('training_memory_interference_rate', 0.0),
        'false_positive_rate': metrics.get('false_positive_rate', 0.0),
        'rag_retrieval_contained_deleted': metrics.get('rag_retrieval_contained_deleted', 0)
    }


def mcnemar_test(n01: int, n10: int) -> Tuple[float, float]:
    """
    McNemar's test for paired binary data.

    n01: Model1 incorrect, Model2 correct
    n10: Model1 correct, Model2 incorrect

    Returns:
        (chi_square, p_value)
    """
    # Continuity correction for small samples
    chi_square = ((abs(n01 - n10) - 1) ** 2) / (n01 + n10) if (n01 + n10) > 0 else 0.0

    # Approximate p-value from chi-square distribution (df=1)
    # For quick implementation, we'll just return chi-square
    # Proper p-value would require scipy or similar
    p_value = None  # Would need scipy.stats.chi2.sf(chi_square, df=1)

    return chi_square, p_value


def cohens_h(p1: float, p2: float) -> float:
    """
    Cohen's h effect size for difference between two proportions.

    h = 2 * (arcsin(sqrt(p1)) - arcsin(sqrt(p2)))

    Interpretation:
        Small:  |h| = 0.20
        Medium: |h| = 0.50
        Large:  |h| = 0.80
    """
    import math

    phi1 = 2 * math.asin(math.sqrt(p1))
    phi2 = 2 * math.asin(math.sqrt(p2))

    return phi1 - phi2


def analyze_detailed_results(base_results: Dict, finetuned_results: Dict) -> Dict:
    """
    Analyze detailed question-by-question results.

    Creates contingency table:
    - Both correct (true negative)
    - Base correct, Fine-tuned incorrect
    - Base incorrect, Fine-tuned correct
    - Both incorrect
    """
    base_detailed = base_results.get('detailed_results', [])
    finetuned_detailed = finetuned_results.get('detailed_results', [])

    # Match questions by ID
    base_by_id = {r['question_id']: r for r in base_detailed if r.get('is_category_3')}
    finetuned_by_id = {r['question_id']: r for r in finetuned_detailed if r.get('is_category_3')}

    common_ids = set(base_by_id.keys()) & set(finetuned_by_id.keys())

    both_correct = 0  # Both said "not found" (true negative)
    base_correct_ft_incorrect = 0  # Base: TN, Fine-tuned: hallucination
    base_incorrect_ft_correct = 0  # Base: hallucination, Fine-tuned: TN
    both_incorrect = 0  # Both hallucinated

    case_details = []

    for qid in common_ids:
        base_result = base_by_id[qid]
        ft_result = finetuned_by_id[qid]

        # Check if model correctly refused (true negative)
        base_correct = not base_result.get('deletion_hallucination', False)
        ft_correct = not ft_result.get('deletion_hallucination', False)

        if base_correct and ft_correct:
            both_correct += 1
        elif base_correct and not ft_correct:
            base_correct_ft_incorrect += 1
            case_details.append({
                'question_id': qid,
                'question': base_result.get('question', ''),
                'deleted_function': ft_result.get('function_name', ''),
                'base_response': base_result.get('ranked_locations', [''])[0],
                'finetuned_response': ft_result.get('ranked_locations', [''])[0],
                'analysis': 'Fine-tuned hallucinated, base adapted'
            })
        elif not base_correct and ft_correct:
            base_incorrect_ft_correct += 1
            case_details.append({
                'question_id': qid,
                'question': base_result.get('question', ''),
                'deleted_function': base_result.get('function_name', ''),
                'base_response': base_result.get('ranked_locations', [''])[0],
                'finetuned_response': ft_result.get('ranked_locations', [''])[0],
                'analysis': 'Base hallucinated, fine-tuned adapted (unexpected)'
            })
        else:
            both_incorrect += 1

    return {
        'total_questions': len(common_ids),
        'both_correct': both_correct,
        'base_correct_ft_incorrect': base_correct_ft_incorrect,
        'base_incorrect_ft_correct': base_incorrect_ft_correct,
        'both_incorrect': both_incorrect,
        'case_details': case_details
    }


def generate_report(
    base_metrics: Dict,
    finetuned_metrics: Dict,
    detailed_analysis: Dict,
    output_path: str = None
) -> Dict:
    """Generate comprehensive deletion experiment report."""

    report = {
        'experiment': 'RQ2 - Deletion Experiment (Training Memory Interference)',
        'hypothesis': (
            'Fine-tuned models exhibit training memory interference, '
            'hallucinating deleted functions from training data, '
            'while base models correctly adapt to updated RAG retrieval.'
        ),
        'base_model': base_metrics,
        'finetuned_model': finetuned_metrics,
        'comparison': {},
        'statistical_tests': {},
        'detailed_analysis': detailed_analysis,
        'interpretation': {}
    }

    # Comparison
    total_q = finetuned_metrics['category_3_questions']

    report['comparison'] = {
        'total_deletion_questions': total_q,
        'true_negative_rate_base': base_metrics['true_negative_rate'],
        'true_negative_rate_finetuned': finetuned_metrics['true_negative_rate'],
        'interference_rate_base': base_metrics['training_memory_interference_rate'],
        'interference_rate_finetuned': finetuned_metrics['training_memory_interference_rate'],
        'difference_tn_rate': base_metrics['true_negative_rate'] - finetuned_metrics['true_negative_rate'],
        'difference_interference': finetuned_metrics['training_memory_interference_rate'] - base_metrics['training_memory_interference_rate']
    }

    # Statistical tests
    n01 = detailed_analysis['base_correct_ft_incorrect']  # Base correct, FT incorrect
    n10 = detailed_analysis['base_incorrect_ft_correct']  # Base incorrect, FT correct

    chi_square, p_value = mcnemar_test(n01, n10)

    report['statistical_tests']['mcnemar'] = {
        'n01_base_correct_ft_incorrect': n01,
        'n10_base_incorrect_ft_correct': n10,
        'chi_square': chi_square,
        'p_value': p_value,
        'note': 'McNemar test for paired binary outcomes (with continuity correction)'
    }

    # Effect size (Cohen's h)
    tn_rate_base = base_metrics['true_negative_rate'] / 100.0
    tn_rate_ft = finetuned_metrics['true_negative_rate'] / 100.0

    cohens_h_value = cohens_h(tn_rate_base, tn_rate_ft)

    report['statistical_tests']['effect_size'] = {
        'cohens_h': cohens_h_value,
        'interpretation': (
            'Large' if abs(cohens_h_value) >= 0.8 else
            'Medium' if abs(cohens_h_value) >= 0.5 else
            'Small' if abs(cohens_h_value) >= 0.2 else
            'Negligible'
        ),
        'note': 'Cohen\'s h for difference between proportions'
    }

    # Interpretation
    hypothesis_supported = (
        finetuned_metrics['training_memory_interference_rate'] >
        base_metrics['training_memory_interference_rate']
    )

    report['interpretation'] = {
        'hypothesis_supported': hypothesis_supported,
        'key_finding': (
            f"Fine-tuned model showed {finetuned_metrics['training_memory_interference_rate']:.1f}% "
            f"training memory interference vs {base_metrics['training_memory_interference_rate']:.1f}% "
            f"for base model (difference: {report['comparison']['difference_interference']:.1f}%)"
        ),
        'statistical_significance': (
            f"Chi-square = {chi_square:.2f}, p-value = {p_value if p_value else 'N/A (requires scipy)'}"
        ),
        'effect_size_interpretation': report['statistical_tests']['effect_size']['interpretation'],
        'practical_significance': (
            'Strong evidence' if abs(report['comparison']['difference_interference']) > 20 else
            'Moderate evidence' if abs(report['comparison']['difference_interference']) > 10 else
            'Weak evidence' if abs(report['comparison']['difference_interference']) > 5 else
            'Minimal evidence'
        )
    }

    # Save report if output path provided
    if output_path:
        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(report, f, indent=2)
        print(f"✓ Report saved to: {output_path}")

    return report


def print_report_summary(report: Dict):
    """Print human-readable summary of the deletion experiment report."""

    print("\n" + "="*80)
    print("DELETION EXPERIMENT ANALYSIS (RQ2)")
    print("="*80)

    print(f"\nHypothesis: {report['hypothesis']}")

    print("\n" + "-"*80)
    print("RESULTS")
    print("-"*80)

    comp = report['comparison']
    print(f"\nTotal Deletion Questions: {comp['total_deletion_questions']}")

    print("\nTrue Negative Rate (Correctly said 'not found'):")
    print(f"  Base Model:       {comp['true_negative_rate_base']:5.1f}%")
    print(f"  Fine-tuned Model: {comp['true_negative_rate_finetuned']:5.1f}%")
    print(f"  Difference:       {comp['difference_tn_rate']:+5.1f}%")

    print("\nTraining Memory Interference Rate (Hallucinated deleted function):")
    print(f"  Base Model:       {comp['interference_rate_base']:5.1f}%")
    print(f"  Fine-tuned Model: {comp['interference_rate_finetuned']:5.1f}%")
    print(f"  Difference:       {comp['difference_interference']:+5.1f}%")

    print("\n" + "-"*80)
    print("STATISTICAL ANALYSIS")
    print("-"*80)

    mcnemar = report['statistical_tests']['mcnemar']
    print(f"\nMcNemar's Test (paired binary outcomes):")
    print(f"  Base correct, FT incorrect: {mcnemar['n01_base_correct_ft_incorrect']}")
    print(f"  Base incorrect, FT correct: {mcnemar['n10_base_incorrect_ft_correct']}")
    print(f"  Chi-square statistic: {mcnemar['chi_square']:.2f}")
    if mcnemar['p_value']:
        print(f"  P-value: {mcnemar['p_value']:.4f}")
    else:
        print(f"  P-value: Not computed (requires scipy)")

    effect = report['statistical_tests']['effect_size']
    print(f"\nEffect Size (Cohen's h):")
    print(f"  h = {effect['cohens_h']:.3f}")
    print(f"  Interpretation: {effect['interpretation']}")

    print("\n" + "-"*80)
    print("INTERPRETATION")
    print("-"*80)

    interp = report['interpretation']
    print(f"\nHypothesis Supported: {'YES' if interp['hypothesis_supported'] else 'NO'}")
    print(f"\nKey Finding:")
    print(f"  {interp['key_finding']}")
    print(f"\nPractical Significance: {interp['practical_significance']}")
    print(f"Effect Size: {interp['effect_size_interpretation']}")

    # Detailed cases
    detailed = report['detailed_analysis']
    if detailed['base_correct_ft_incorrect'] > 0:
        print("\n" + "-"*80)
        print("NOTABLE CASES: Fine-tuned Hallucinated, Base Adapted")
        print("-"*80)
        for case in detailed['case_details'][:5]:  # Show first 5
            if 'Fine-tuned hallucinated' in case['analysis']:
                print(f"\nQ{case['question_id']}: {case['question'][:60]}...")
                print(f"  Deleted Function: {case['deleted_function']}")
                print(f"  Base Response:       {case['base_response']}")
                print(f"  Fine-tuned Response: {case['finetuned_response']}")

    print("\n" + "="*80)


def main():
    parser = argparse.ArgumentParser(
        description="Analyze deletion experiment (RQ2) results"
    )
    parser.add_argument(
        '--base-results',
        required=True,
        help='Path to base model deletion evaluation results JSON'
    )
    parser.add_argument(
        '--finetuned-results',
        required=True,
        help='Path to fine-tuned model deletion evaluation results JSON'
    )
    parser.add_argument(
        '--output',
        default='deletion_experiment_report.json',
        help='Output path for analysis report (default: deletion_experiment_report.json)'
    )
    parser.add_argument(
        '--verbose',
        action='store_true',
        help='Print detailed analysis'
    )

    args = parser.parse_args()

    print("Loading evaluation results...")
    base_results = load_evaluation_results(args.base_results)
    finetuned_results = load_evaluation_results(args.finetuned_results)

    print("Extracting deletion experiment metrics...")
    base_metrics = extract_deletion_metrics(base_results)
    finetuned_metrics = extract_deletion_metrics(finetuned_results)

    # Validate
    if base_metrics['category_3_questions'] == 0:
        print("ERROR: No Category 3 (deletion) questions found in base model results!")
        sys.exit(1)

    if finetuned_metrics['category_3_questions'] == 0:
        print("ERROR: No Category 3 (deletion) questions found in fine-tuned model results!")
        sys.exit(1)

    print("Analyzing detailed question-by-question results...")
    detailed_analysis = analyze_detailed_results(base_results, finetuned_results)

    print("Generating report...")
    report = generate_report(
        base_metrics,
        finetuned_metrics,
        detailed_analysis,
        output_path=args.output
    )

    # Print summary
    print_report_summary(report)

    print(f"\n✓ Analysis complete!")
    print(f"Full report saved to: {args.output}")


if __name__ == '__main__':
    main()
