# RAG Code Search System - Usage Guide

## Quick Start

### 1. Initial Setup
```bash
# Install dependencies
pip install -r requirements.txt

# Index the codebase
python reindex.py
```

### 2. Interactive Search
```bash
python rag_chat.py
```

### 3. Run Evaluation
```bash
python evaluate.py
```

## Main Components

### rag_system.py
Core RAG implementation with hybrid search (vector + BM25) and cross-encoder re-ranking.

Configuration variables (lines 20-64):
- `USE_FUNCTION_NAME_BOOSTING`: Boost scores for function name matches
- `USE_SIGNATURE_ONLY`: Re-rank using signature only vs. full code
- `USE_QUERY_EXPANSION`: Expand queries with variations
- `CANDIDATE_POOL_SIZE`: Number of candidates before re-ranking (default: 40)
- `USE_FILE_SUMMARY_CHUNKS`: Index file-level summaries

### reindex.py
Re-index codebase after changes.

```bash
# Standard indexing
python reindex.py

# Index different codebase
python reindex.py --codebase ./codebase_enriched

# Docstring-only mode
python reindex.py --docstring-only
```

### evaluate.py
Automated evaluation using test questions.

Configuration (lines 13-83):
- `TEST_QUESTIONS_FILE`: Which question set to use
- `USE_LLM`: Enable/disable LLM scoring (True = full pipeline, False = re-ranking only)
- `USE_FINETUNED`: Use fine-tuned model vs. base model
- `USE_FRESH_INDEX`: Delete and rebuild index vs. use existing

Test question files:
- `test_questions_v2.json`: Main evaluation set
- `test_questions_rq2.json`: Deletion experiment (RQ2)
- `test_questions_category2.json`: Alternative test set

### rag_chat.py
Interactive search interface with LLM ranking.

Configuration (lines 10-31):
- `MODEL_CHOICE`: "1.3b" or "6.7b" (DeepSeek-Coder)
- `USE_FINETUNED`: Base vs. fine-tuned model
- `USE_BATCH_RANKING`: Batch ranking (fast) vs. individual scoring (slow)
- `LLM_SELECTION_MODE`: "aggressive" vs. "aggressive_no_fewshot"
- `BATCH_RANKING_SIZE`: Candidates sent to LLM (default: 5)

## Advanced Features

### Fine-tuning
```bash
# Generate training data and fine-tune
python finetune.py
```

Configuration in finetune.py (lines 42-56):
- `MODEL_NAME`: Base model to fine-tune
- `LORA_RANK`: 16
- `LEARNING_RATE`: 2e-4
- `NUM_EPOCHS`: 3

### LLM Summaries
```bash
# Generate function summaries
python generate_function_summaries.py

# Load summaries into RAG
python load_summaries_into_rag.py
```

### RQ2 Deletion Experiment
```bash
# Select functions to delete
python select_deletion_candidates.py

# Execute deletion
python delete_functions.py

# Evaluate impact
python evaluate.py  # with DELETION_EXPERIMENT_MODE = True
```

## Workflow Examples

### Standard Evaluation
1. `python reindex.py` - Index codebase
2. Edit `evaluate.py`: Set `TEST_QUESTIONS_FILE`, `USE_LLM = True`
3. `python evaluate.py` - Run evaluation
4. Results saved to `evaluation_results_TIMESTAMP.json`

### Compare Base vs. Fine-tuned
1. Edit `rag_chat.py`: `USE_FINETUNED = False`
2. `python evaluate.py` - Evaluate base model
3. Edit `rag_chat.py`: `USE_FINETUNED = True`
4. `python evaluate.py` - Evaluate fine-tuned model

### Test with Different Codebases
```bash
# Index enriched codebase
python reindex.py --codebase ./codebase_enriched --db-path ./chroma_db_enriched

# Update evaluate.py to use enriched database
# Then run evaluation
python evaluate.py
```

## All Python Scripts

### Core System
- `rag_system.py`: Hybrid search (vector + BM25), cross-encoder re-ranking, AST-based chunking
- `rag_chat.py`: Interactive CLI + LLM integration (DeepSeek-Coder)
- `evaluate.py`: Automated evaluation with metrics (Rank@1, Rank@5, hallucination detection)
- `reindex.py`: ChromaDB indexing from codebase

### Fine-tuning Pipeline
- `finetune.py`: QLoRA fine-tuning (4-bit quantization, LoRA adapters)
- `load_finetuned_model.py`: Load base or fine-tuned model with PEFT adapters

### LLM Summaries
- `generate_function_summaries.py`: Generate LLM summaries for all functions
- `load_summaries_into_rag.py`: Load summaries into RAG system
- `smart_summary_selector.py`: Auto-select functions needing LLM summaries (quality score)

### RQ2 Deletion Experiment
- `select_deletion_candidates.py`: Select functions both models found correctly
- `delete_functions.py`: AST-based function deletion with backup
- `analyze_deletion_results.py`: Statistical analysis (McNemar test, Cohen's h)

### Utilities
- `evaluate_all_modes.py`: Test all LLM modes on all datasets automatically
- `gpu_monitor.py`: GPU utilization and memory tracking

### Test Data
- `test_questions_v2.json`: Main evaluation set
- `test_questions_rq2.json`: Deletion experiment (RQ2)
- `test_questions_category2.json`: Hallucination resistance tests
- `training_data.json`: Fine-tuning data
