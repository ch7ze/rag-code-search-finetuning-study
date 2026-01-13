"""
Improved RAG-Enhanced Code Search with DeepSeek-Coder
Uses hybrid search + re-ranking for better results
"""

# ============================================================================
# MODEL CONFIGURATION - Change this to select which model to use
# ============================================================================
# Options: "1.3b" or "6.7b"
MODEL_CHOICE = "6.7b"  # Change to "6.7b" for better accuracy (uses ~5-6 GB VRAM)

# Use fine-tuned model (True) or base model (False)
USE_FINETUNED = False  # Set to True to use fine-tuned model with LoRA adapters

# Fine-tuned model configuration
# Use absolute path to avoid issues when running from different directories
import os as _os
_SCRIPT_DIR = _os.path.dirname(_os.path.abspath(__file__))
FINETUNED_MODEL_PATH = _os.path.join(_SCRIPT_DIR, "finetuned_model")  # Path to fine-tuned LoRA adapters

# LLM Ranking Mode Configuration
USE_BATCH_RANKING = True  # True = 1 LLM call for all candidates (FAST)
                          # False = Individual scoring (old method, SLOW)

# LLM Selection Strategy (only applies when USE_BATCH_RANKING = True)
# "aggressive" = Multiple choice with few-shot (high hallucination, high accuracy)
# "aggressive_no_fewshot" = Multiple choice WITHOUT few-shot (balanced)
LLM_SELECTION_MODE = "aggressive_no_fewshot"

BATCH_RANKING_SIZE = 5   # How many candidates to send to LLM (5, 10, 20)
                          # Recommended: 10 for good balance
# ============================================================================

import re
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer
from rag_system import (
    ImprovedRAGSystem,
    extract_function_signature,
    extract_parameters,
    extract_return_type,
    extract_semantic_tags
)


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


def extract_structured_response(response: str, candidates: list) -> dict:
    """
    Extract structured top-3 ranked candidate numbers from LLM response.
    Maps candidate numbers to actual locations.

    Args:
        response: LLM response text
        candidates: List of candidate chunks with 'location' field

    Returns:
        dict with keys: 'ranked_locations' (list of 3 strings), 'found' (bool), 'raw_response' (str)
    """
    ranked_locations = []

    # Extract RANK_1, RANK_2, RANK_3 as numbers
    for rank in [1, 2, 3]:
        rank_pattern = rf'RANK_{rank}:\s*\[?(\d+)\]?'
        rank_match = re.search(rank_pattern, response, re.IGNORECASE)

        if rank_match:
            candidate_num = int(rank_match.group(1))

            # Map candidate number to actual location (1-indexed)
            if 1 <= candidate_num <= len(candidates):
                location = candidates[candidate_num - 1]['location']
                ranked_locations.append(location)
            else:
                # Invalid candidate number
                ranked_locations.append("NOT_FOUND")
        else:
            # If rank not found, append NOT_FOUND
            ranked_locations.append("NOT_FOUND")

    # Ensure we always have 3 rankings
    while len(ranked_locations) < 3:
        ranked_locations.append("NOT_FOUND")

    # Keep only first 3
    ranked_locations = ranked_locations[:3]

    # Check if any location was found
    found = any(loc != "NOT_FOUND" for loc in ranked_locations)

    return {
        'ranked_locations': ranked_locations,
        'location': ranked_locations[0],  # Keep for backward compatibility
        'found': found,
        'raw_response': response
    }


def llm_batch_select_best(query: str, candidates: list, model, tokenizer,
                          language: str = 'rust', model_size: str = None) -> dict:
    """
    LLM selects best function from candidates in ONE batch call.
    Uses only signature + docstring for efficient token usage.

    Args:
        query: User's search query
        candidates: List of function chunks from reranker (5-20 functions)
        model: LLM model
        tokenizer: Tokenizer
        language: Programming language
        model_size: "1.3b" or "6.7b"

    Returns:
        dict with keys: 'location', 'found', 'raw_response', 'all_scores'
    """
    if model_size is None:
        model_size = MODEL_CHOICE

    # Optimize parameters for model - CRITICAL: Use short responses to prevent hallucinations
    if model_size == "6.7b":
        max_context_length = 4096
        temperature = 0.0  # Deterministic output (most likely token)
        do_sample = False  # Greedy decoding for single answer
        top_p = 1.0  # Not used when do_sample=False
        max_new_tokens = 10  # SHORT: expect "1", "2", or "NOT_FOUND" only
        repetition_penalty = 1.2  # Penalize repetition to avoid loops
    else:
        max_context_length = 3072
        temperature = 0.0
        do_sample = False
        top_p = 1.0
        max_new_tokens = 10
        repetition_penalty = 1.2

    # ============================================================================
    # LLM SELECTION STRATEGY - Two different approaches based on LLM_SELECTION_MODE
    # ============================================================================

    # Check which mode to use
    selection_mode = LLM_SELECTION_MODE if 'LLM_SELECTION_MODE' in globals() else "conservative"

    if selection_mode == "aggressive" or selection_mode == "aggressive_no_fewshot":
        # ============================================================================
        # AGGRESSIVE MODE: Multiple Choice with/without Few-Shot Examples
        # Best for: High accuracy on real questions (Top-5 dataset)
        # Trade-off: More hallucinations on Category 2 (non-existent features)
        # Expected: ~0-20% Category 2 (with few-shot), ~80% Category 2 (without few-shot)
        #           ~60% Top-5 (with few-shot), ~30% Top-5 (without few-shot)
        # ============================================================================

        use_few_shot = (selection_mode == "aggressive")
        mode_name = "Multiple Choice + Few-Shot" if use_few_shot else "Multiple Choice (no few-shot)"
        print(f"  LLM Selection Mode: AGGRESSIVE ({mode_name})", flush=True)

        # Build few-shot examples (2 FOUND, 1 NOT_FOUND for balance)
        if use_few_shot:
            few_shot_examples = """Examples:

Q: "How do I authenticate a user?"
Options:
A. NOT_FOUND
B. login_handler - Handles user login with credentials
C. logout_handler - Handles user logout
D. validate_token - Validates authentication token
Answer: B

Q: "Where is the blockchain integration?"
Options:
A. NOT_FOUND
B. DatabaseManager - Manages database connections
C. create_jwt - Creates JWT tokens
Answer: A

Q: "How do I validate a JWT token?"
Options:
A. NOT_FOUND
B. create_jwt - Creates new JWT token
C. validate_jwt - Validates JWT token and returns claims
D. extract_jwt_from_cookies - Extracts JWT from cookies
Answer: C

"""
        else:
            few_shot_examples = ""

        # Build multiple choice prompt
        if use_few_shot:
            prompt = few_shot_examples + f"""Now answer this question:

Query: {query}

Options:
A. NOT_FOUND
"""
        else:
            prompt = f"""Select the BEST function that matches the query, or choose A if none match.

Query: {query}

Options:
A. NOT_FOUND
"""

        # Add all candidates as options B, C, D, E, F
        option_letters = ['B', 'C', 'D', 'E', 'F']
        for i, chunk in enumerate(candidates):
            if i >= len(option_letters):
                break

            letter = option_letters[i]
            name = chunk.get('name', 'unknown')
            signature = chunk.get('signature', '')
            docstring = chunk.get('docstring', chunk.get('context', ''))

            # Use signature if available, otherwise docstring
            if signature:
                desc = signature[:200].replace('\n', ' ').strip()
            else:
                desc = docstring[:200].replace('\n', ' ').strip() if docstring else ""

            prompt += f"{letter}. {name}"
            if desc:
                prompt += f" - {desc}"
            prompt += "\n"

        prompt += f"\nAnswer with ONLY the letter (A-F):"

        # Generate response
        inputs = tokenizer(prompt, return_tensors="pt", truncation=True,
                          max_length=max_context_length).to(model.device)

        with torch.no_grad():
            outputs = model.generate(
                **inputs,
                max_new_tokens=max_new_tokens,
                temperature=temperature,
                top_p=top_p,
                do_sample=do_sample,
                repetition_penalty=repetition_penalty,
                pad_token_id=tokenizer.eos_token_id,
                eos_token_id=tokenizer.eos_token_id,
                use_cache=False
            )

        response = tokenizer.decode(outputs[0][inputs.input_ids.shape[1]:], skip_special_tokens=True).strip()
        print(f"  LLM response: {response}", flush=True)

        # Parse the letter from response
        selected_letter = None
        for letter in ['A', 'B', 'C', 'D', 'E', 'F']:
            if letter in response.upper()[:5]:  # Check first 5 chars for the letter
                selected_letter = letter
                break

        if selected_letter is None or selected_letter == 'A':
            # NOT_FOUND selected or unparseable
            print(f"  LLM selected: NOT_FOUND (letter={selected_letter})", flush=True)
            return {
                'location': 'NOT_FOUND',
                'found': False,
                'raw_response': response,
                'ranked_locations': ['NOT_FOUND'] * 5,
                'all_scores': [{'location': 'NOT_FOUND', 'function_name': 'NOT_FOUND',
                               'score': 0, 'rerank_score': 0}
                              for _ in range(5)],
                'llm_prompt': prompt
            }

        # Map letter to candidate index (B=0, C=1, D=2, E=3, F=4)
        letter_to_index = {'B': 0, 'C': 1, 'D': 2, 'E': 3, 'F': 4}
        selected_index = letter_to_index.get(selected_letter, 0)

        if selected_index >= len(candidates):
            # Invalid selection, default to first
            selected_index = 0

        selected_chunk = candidates[selected_index]
        selected_location = selected_chunk.get('location', 'unknown')
        print(f"  LLM selected: {selected_location} (letter={selected_letter}, index={selected_index})", flush=True)

        # Build ranked locations and scores
        ranked_locations = []
        all_scores = []
        for i, chunk in enumerate(candidates):
            location = chunk.get('location', 'unknown')
            rerank_score = chunk.get('rerank_score', 0.0)

            ranked_locations.append(location)
            all_scores.append({
                'location': location,
                'function_name': chunk.get('name', 'unknown'),
                'score': rerank_score,
                'rerank_score': rerank_score
            })

        return {
            'location': selected_location,
            'found': True,
            'raw_response': response,
            'ranked_locations': ranked_locations,
            'all_scores': all_scores,
            'llm_prompt': prompt
        }

    else:
        # ============================================================================
        # CONSERVATIVE MODE: Two-Phase Individual Filtering
        # Best for: Hallucination resistance (Category 2 dataset)
        # Trade-off: Poor accuracy on real questions (Top-5 dataset)
        # Expected: ~100% Category 2, ~0% Top-5
        # ============================================================================
        print(f"  LLM Selection Mode: CONSERVATIVE (Individual Filtering)", flush=True)

        # PHASE 1: Individual binary filtering for each function
        print(f"  Phase 1: Binary filtering of {len(candidates)} candidates...", flush=True)

        filtered_candidates = []
        phase1_responses = []

        for i, chunk in enumerate(candidates, 1):
            name = chunk.get('name', 'unknown')
            docstring = chunk.get('docstring', chunk.get('context', ''))
            signature = chunk.get('signature', '')

            # Use signature if available, otherwise docstring
            if signature:
                desc = signature[:200].replace('\n', ' ').strip()
            else:
                desc = docstring[:200].replace('\n', ' ').strip() if docstring else ""

            # Binary question for this specific function
            phase1_prompt = f"""Does this function match the query?

Query: {query}

Function: {name}
{desc}

Answer ONLY: YES or NO"""

            # Generate response for this function
            inputs = tokenizer(phase1_prompt, return_tensors="pt", truncation=True,
                              max_length=max_context_length).to(model.device)

            with torch.no_grad():
                outputs = model.generate(
                    **inputs,
                    max_new_tokens=3,  # Just "YES" or "NO"
                    temperature=temperature,
                    top_p=top_p,
                    do_sample=do_sample,
                    repetition_penalty=repetition_penalty,
                    pad_token_id=tokenizer.eos_token_id,
                    eos_token_id=tokenizer.eos_token_id,
                    use_cache=False
                )

            phase1_response = tokenizer.decode(outputs[0][inputs.input_ids.shape[1]:], skip_special_tokens=True).strip()
            phase1_responses.append(phase1_response)

            # Check if YES
            if "YES" in phase1_response.upper():
                filtered_candidates.append(chunk)
                print(f"    [{i}] {name}: YES", flush=True)
            else:
                print(f"    [{i}] {name}: NO", flush=True)

        print(f"  Phase 1 result: {len(filtered_candidates)}/{len(candidates)} candidates passed", flush=True)

        # If NO candidates passed Phase 1 → NOT_FOUND
        if len(filtered_candidates) == 0:
            print(f"  All candidates rejected in Phase 1 → NOT_FOUND", flush=True)
            return {
                'location': 'NOT_FOUND',
                'found': False,
                'raw_response': f'Phase 1: All NO - {phase1_responses}',
                'ranked_locations': ['NOT_FOUND'] * 5,
                'all_scores': [{'location': 'NOT_FOUND', 'function_name': 'NOT_FOUND',
                               'score': 0, 'rerank_score': 0}
                              for _ in range(5)],
                'llm_prompt': 'Phase 1: Binary filtering (see raw_response for details)'
            }

        # If exactly 1 candidate passed → return it directly
        if len(filtered_candidates) == 1:
            selected_chunk = filtered_candidates[0]
            selected_location = selected_chunk['location']
            selected_name = selected_chunk.get('name', 'unknown')
            print(f"  Only 1 candidate passed Phase 1: {selected_name}", flush=True)

            # Build ranked list
            ranked_chunks = [selected_chunk]
            ranked_locations = [selected_location]

            # Add remaining original candidates
            for chunk in candidates:
                if chunk['location'] != selected_location and len(ranked_locations) < 5:
                    ranked_chunks.append(chunk)
                    ranked_locations.append(chunk['location'])

            # Pad to 5
            while len(ranked_locations) < 5:
                ranked_locations.append('NOT_FOUND')

            # Build scores
            all_scores = []
            for chunk in ranked_chunks:
                rerank_score = chunk.get('rerank_score', 0)
                rerank_score = float(rerank_score) if rerank_score else 0.0
                all_scores.append({
                    'location': chunk['location'],
                    'function_name': chunk.get('name', 'unknown'),
                    'score': rerank_score,
                    'rerank_score': rerank_score
                })

            return {
                'location': selected_location,
                'found': True,
                'raw_response': f'Phase 1: Only {selected_name} passed',
                'ranked_locations': ranked_locations,
                'all_scores': all_scores,
                'llm_prompt': 'Phase 1: Binary filtering (only 1 passed)'
            }

        # PHASE 2: Multiple candidates passed → ask which is BEST
        print(f"  Phase 2: Selecting best from {len(filtered_candidates)} filtered candidates...", flush=True)

        # Build multiple choice with filtered candidates
        phase2_prompt = f"""Which function is the BEST match?

Query: {query}

Options:
"""

        for i, chunk in enumerate(filtered_candidates, 1):
            name = chunk.get('name', 'unknown')
            docstring = chunk.get('docstring', chunk.get('context', ''))
            desc = docstring[:100].replace('\n', ' ').strip() if docstring else ""
            phase2_prompt += f"{i}. {name} - {desc}\n"

        phase2_prompt += f"\nAnswer with ONLY the number (1-{len(filtered_candidates)}):"

        # Generate Phase 2 response
        inputs = tokenizer(phase2_prompt, return_tensors="pt", truncation=True,
                          max_length=max_context_length).to(model.device)

        with torch.no_grad():
            outputs = model.generate(
                **inputs,
                max_new_tokens=3,
                temperature=temperature,
                top_p=top_p,
                do_sample=do_sample,
                repetition_penalty=repetition_penalty,
                pad_token_id=tokenizer.eos_token_id,
                eos_token_id=tokenizer.eos_token_id,
                use_cache=False
            )

        phase2_response = tokenizer.decode(outputs[0][inputs.input_ids.shape[1]:], skip_special_tokens=True).strip()
        print(f"  Phase 2 response: {phase2_response}", flush=True)

        # Parse number from Phase 2
        selected_idx = None
        numbers = re.findall(r'\b(\d+)\b', phase2_response)
        if numbers:
            try:
                selected_idx = int(numbers[0])
                if selected_idx < 1 or selected_idx > len(filtered_candidates):
                    selected_idx = None
            except ValueError:
                selected_idx = None

        if selected_idx is None:
            # Default to first filtered candidate if parsing fails
            print(f"  Phase 2 parsing failed, using first filtered candidate", flush=True)
            selected_idx = 1

        selected_chunk = filtered_candidates[selected_idx - 1]
        selected_location = selected_chunk['location']
        selected_name = selected_chunk.get('name', 'unknown')
        print(f"  Phase 2 selected: [{selected_idx}] {selected_name}", flush=True)

        # Build ranked list
        ranked_chunks = [selected_chunk]
        ranked_locations = [selected_location]

        # Add remaining candidates
        for chunk in candidates:
            if chunk['location'] != selected_location and len(ranked_locations) < 5:
                ranked_chunks.append(chunk)
                ranked_locations.append(chunk['location'])

        # Pad to 5
        while len(ranked_locations) < 5:
            ranked_locations.append('NOT_FOUND')

        # Build scores in SAME ORDER as ranked_locations (critical for evaluation!)
        all_scores = []
        for chunk in ranked_chunks:
            rerank_score = chunk.get('rerank_score', 0)
            # Convert numpy/torch float32 to Python float for JSON serialization
            rerank_score = float(rerank_score) if rerank_score else 0.0

            # Use raw reranking score for all candidates (including LLM selection)
            all_scores.append({
                'location': chunk['location'],
                'function_name': chunk.get('name', 'unknown'),
                'score': rerank_score,  # Raw cross-encoder score (not percentage)
                'rerank_score': rerank_score
            })

        return {
            'location': selected_location,
            'found': True,
            'raw_response': phase2_response,
            'ranked_locations': ranked_locations,
            'all_scores': all_scores,
            'llm_prompt': phase2_prompt
        }


def evaluate_single_function(query: str, function_chunk: dict, model, tokenizer,
                            language: str = 'rust', model_size: str = None) -> dict:
    """
    Evaluate a single function and return a confidence score (0-100%)
    that it answers the given question.

    Args:
        query: The user's question
        function_chunk: Single function chunk with 'code', 'location', 'name', etc.
        model: The LLM model
        tokenizer: The tokenizer
        language: Programming language (for metadata extraction)
        model_size: "1.3b" or "6.7b" - optimizes parameters

    Returns:
        dict with keys: 'score' (0-100), 'location', 'raw_response'
    """
    # Auto-detect model size if not provided
    if model_size is None:
        model_size = MODEL_CHOICE

    # Optimize parameters for model
    if model_size == "6.7b":
        max_code_chars = 600
        max_doc_chars = 250
        max_context_length = 4096
        temperature = 0.1
        do_sample = False
        top_p = 0.9
        max_new_tokens = 20  # Just need a number: "85"
        repetition_penalty = 1.15
    else:
        max_code_chars = 400
        max_doc_chars = 150
        max_context_length = 3072
        temperature = 0.15
        do_sample = True
        top_p = 0.9
        max_new_tokens = 20
        repetition_penalty = 1.1

    # Extract metadata
    code = function_chunk['code']
    location = function_chunk['location']
    signature = extract_function_signature(code, language)
    params = extract_parameters(signature, language)
    return_type = extract_return_type(signature, language)

    # Build compact context for this single function
    context = f"""FUNCTION:
Location: {location}
Name: {function_chunk.get('name', 'unknown')}
Signature: {signature}
"""

    if params:
        context += f"Parameters: {', '.join(params[:3])}\n"
    if return_type:
        context += f"Returns: {return_type}\n"
    if function_chunk.get('context'):
        doc = function_chunk['context'][:max_doc_chars].replace('\n', ' ')
        context += f"Documentation: {doc}\n"

    context += f"\nCode:\n{code[:max_code_chars]}\n"

    # Focused scoring prompt
    prompt = f"""Rate how well this function answers the question (0-100%).

QUESTION: {query}

{context}

TASK: Provide a confidence score (0-100) that this function correctly answers the question.

IMPORTANT:
- 0% = Completely unrelated
- 50% = Somewhat related but not the answer
- 100% = Perfect match, this is the answer

OUTPUT FORMAT (just the number):
SCORE: """

    # Generate response with optimized parameters
    inputs = tokenizer(prompt, return_tensors="pt", truncation=True,
                      max_length=max_context_length).to(model.device)

    with torch.no_grad():
        outputs = model.generate(
            **inputs,
            max_new_tokens=max_new_tokens,
            temperature=temperature,
            top_p=top_p,
            do_sample=do_sample,
            repetition_penalty=repetition_penalty,
            pad_token_id=tokenizer.eos_token_id,
            eos_token_id=tokenizer.eos_token_id,
            use_cache=False  # Don't use cache to ensure clean context
        )

    # Decode response
    response = tokenizer.decode(outputs[0][inputs.input_ids.shape[1]:], skip_special_tokens=True)

    # Extract score from response
    score = 0
    # Try to find a number in the response
    import re
    numbers = re.findall(r'\b(\d+(?:\.\d+)?)\b', response)
    if numbers:
        try:
            score = float(numbers[0])
            # Clamp to 0-100 range
            score = max(0, min(100, score))
        except ValueError:
            score = 0

    return {
        'score': score,
        'location': location,
        'raw_response': response,
        'function_name': function_chunk.get('name', 'unknown'),
        'llm_prompt': prompt
    }


def load_model(model_choice: str = "1.3b", use_finetuned: bool = False, finetuned_path: str = None):
    """
    Load DeepSeek-Coder model based on choice.

    Args:
        model_choice: "1.3b" for original model, "6.7b" for GPTQ quantized model
        use_finetuned: If True, load fine-tuned model with LoRA adapters
        finetuned_path: Path to fine-tuned LoRA adapters (required if use_finetuned=True)

    Returns:
        (model, tokenizer) tuple
    """
    # If fine-tuned model requested, use the loader
    if use_finetuned:
        if finetuned_path is None:
            finetuned_path = FINETUNED_MODEL_PATH

        from load_finetuned_model import load_finetuned_model
        print("="*70)
        print("LOADING FINE-TUNED MODEL")
        print("="*70)
        model, tokenizer = load_finetuned_model(finetuned_path, model_choice)
        return model, tokenizer

    # Otherwise load base model
    print("="*70)
    print("LOADING BASE MODEL")
    print("="*70)

    if model_choice == "6.7b":
        print("Loading DeepSeek-Coder-6.7B-Instruct (4-bit quantized)...")
        model_name = "deepseek-ai/deepseek-coder-6.7b-instruct"

        # Use bitsandbytes for on-the-fly 4-bit quantization (simple, no extra packages)
        from transformers import BitsAndBytesConfig

        quantization_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_compute_dtype=torch.float16,
            bnb_4bit_use_double_quant=True,
            bnb_4bit_quant_type="nf4"
        )

        model = AutoModelForCausalLM.from_pretrained(
            model_name,
            device_map="auto",
            quantization_config=quantization_config,
            trust_remote_code=True,
            torch_dtype=torch.float16
        )

        tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=True)
        print("✓ BASE Model loaded (4-bit quantized, ~5-6 GB VRAM)\n")

    else:  # Default: 1.3b
        print("Loading DeepSeek-Coder-1.3B-Instruct...")
        model_name = "deepseek-ai/deepseek-coder-1.3b-instruct"

        model = AutoModelForCausalLM.from_pretrained(
            model_name,
            device_map="cuda:0",
            trust_remote_code=True,
            torch_dtype=torch.float16
        )

        tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=True)
        print("✓ BASE Model loaded (Float16, ~3 GB VRAM)\n")

    return model, tokenizer


def rag_query(query: str, rag: ImprovedRAGSystem, model, tokenizer, top_k: int = 10, model_size: str = None) -> dict:
    """
    RAG-based code search with LLM ranking.

    MODE 1 (USE_BATCH_RANKING=True):
        - Gets top-N functions from RAG reranker
        - LLM selects best in ONE call (fast, efficient)

    MODE 2 (USE_BATCH_RANKING=False):
        - Gets top-10 functions from RAG system
        - Evaluates each individually (old method, slow)

    Returns structured response with extracted location.

    Args:
        top_k: Number of top functions to retrieve (default: 10)
        model_size: "1.3b" or "6.7b" - optimizes parameters for model capacity
    """
    # Auto-detect model size if not provided
    if model_size is None:
        model_size = MODEL_CHOICE

    # Get top-N functions from RAG system (reranker output)
    # Use configurable batch size for new mode
    retrieve_count = BATCH_RANKING_SIZE if USE_BATCH_RANKING else 10
    retrieved_chunks = rag.retrieve(query, top_k=retrieve_count, hybrid=True)

    if not retrieved_chunks:
        return {
            'location': 'NOT_FOUND',
            'explanation': 'No relevant code found in the codebase.',
            'found': False,
            'raw_response': 'No relevant code found in the codebase.'
        }

    # Detect language from first chunk
    first_location = retrieved_chunks[0]['location']
    if '.rs' in first_location:
        language = 'rust'
    elif '.js' in first_location:
        language = 'javascript'
    else:
        language = 'unknown'

    # MODE 1: Batch Ranking (NEW - FAST)
    if USE_BATCH_RANKING:
        print(f"  LLM Batch Selection: Evaluating top-{len(retrieved_chunks)} functions in 1 call...", flush=True)

        # Single LLM call to select best
        result = llm_batch_select_best(
            query=query,
            candidates=retrieved_chunks,
            model=model,
            tokenizer=tokenizer,
            language=language,
            model_size=model_size
        )

        print(f"  LLM selected: {shorten_path(result['location'])}", flush=True)
        print(f"  Raw response: {result['raw_response']}", flush=True)

        return result

    # MODE 2: Individual Scoring (OLD - SLOW)
    print(f"  Evaluating top-10 functions individually...", flush=True)

    function_scores = []
    all_prompts = []  # Collect all prompts for storage

    for i, chunk in enumerate(retrieved_chunks, 1):
        # Clear GPU cache before each evaluation to reset context
        torch.cuda.empty_cache()

        print(f"  [{i}/10] Evaluating {chunk.get('name', 'unknown')}...", flush=True)

        # Evaluate this single function
        evaluation = evaluate_single_function(
            query=query,
            function_chunk=chunk,
            model=model,
            tokenizer=tokenizer,
            language=language,
            model_size=model_size
        )

        function_scores.append({
            'location': evaluation['location'],
            'function_name': evaluation['function_name'],
            'score': evaluation['score'],
            'raw_response': evaluation['raw_response'],
            'rerank_score': chunk.get('rerank_score', 0)
        })

        # Store prompt for this function
        all_prompts.append({
            'function_name': evaluation['function_name'],
            'prompt': evaluation.get('llm_prompt', ''),
            'response': evaluation.get('llm_full_response', '')
        })

        # Shorten path for display
        display_location = shorten_path(evaluation['location'])
        print(f"      Score: {evaluation['score']:.1f}% | {display_location}", flush=True)

    # Sort by LLM score (descending)
    function_scores.sort(key=lambda x: x['score'], reverse=True)

    # Get top-5 results for ranking
    ranked_locations = []
    for i in range(5):
        if i < len(function_scores):
            ranked_locations.append(function_scores[i]['location'])
        else:
            ranked_locations.append("NOT_FOUND")

    # Build response in expected format
    found = function_scores[0]['score'] > 0 if function_scores else False

    # Create detailed raw response showing all scores
    raw_response = "EVALUATION RESULTS:\n"
    for i, result in enumerate(function_scores[:5], 1):  # Show top-5
        raw_response += f"RANK_{i}: {result['function_name']} - {result['score']:.1f}%\n"

    structured_response = {
        'ranked_locations': ranked_locations,
        'location': ranked_locations[0],
        'found': found,
        'raw_response': raw_response,
        'all_scores': function_scores,  # Include all 10 scores for analysis
        'llm_prompts': all_prompts,  # Include all individual prompts and responses
        'llm_prompt': f"Individual Scoring Mode: {len(all_prompts)} functions evaluated"
    }

    print("  Evaluation complete!", flush=True)

    return structured_response


def interactive_rag_search(rag: ImprovedRAGSystem, model, tokenizer):
    """Interactive RAG-based code search with improved retrieval"""
    print("=" * 70)
    print("Improved RAG-Enhanced Code Search (type 'exit' to quit)")
    print("Features: Hybrid Search (Vector + BM25) + Cross-Encoder Re-ranking")
    print("=" * 70)

    while True:
        query = input("\nQuery: ").strip()

        if query.lower() in ['exit', 'quit', 'q']:
            print("\nGoodbye!")
            break

        if not query:
            continue

        print("\nSearching codebase (hybrid search + re-ranking)...", end="", flush=True)

        # Retrieve chunks
        chunks = rag.retrieve(query, top_k=5, hybrid=True)

        print(f"\r✓ Found {len(chunks)} relevant chunks\n")

        # Display retrieved locations with scores
        print("📍 Retrieved Locations (Re-ranked):")
        for i, chunk in enumerate(chunks, 1):
            score_info = f"rerank: {chunk.get('rerank_score', 0):.3f}"
            if 'vector_score' in chunk:
                score_info += f", vector: {chunk['vector_score']:.3f}"
            if 'bm25_score' in chunk:
                score_info += f", bm25: {chunk['bm25_score']:.2f}"

            display_loc = shorten_path(chunk['location'])
            print(f"  {i}. {display_loc} ({score_info})")
            if chunk.get('name'):
                print(f"      Function: {chunk['name']}")

        # Generate LLM response
        print("\n🤖 DeepSeek Analysis:")
        response = rag_query(query, rag, model, tokenizer)

        # Display structured response
        print(f"\nTop Result: {shorten_path(response['location'])}")

        # Show top 3 scored functions if available
        if 'all_scores' in response and response['all_scores']:
            print("\nTop 3 Functions by Score:")
            for i, score_data in enumerate(response['all_scores'][:3], 1):
                loc = shorten_path(score_data['location'])
                print(f"  {i}. {score_data['function_name']} - {score_data['score']:.1f}%")
                print(f"     {loc}")

        if response['found']:
            print("\n✓ Found in codebase")
        else:
            print("\n✗ Not found in codebase")
        print("-" * 70)


def main():
    import sys
    import os

    print("=" * 70)
    print("DeepSeek-Coder RAG Code Search")
    print("=" * 70)

    # Display model configuration
    print(f"\n📊 MODEL CONFIGURATION:")
    print(f"   Model Size: {MODEL_CHOICE.upper()}")
    if MODEL_CHOICE == "1.3b":
        print("   Architecture: DeepSeek-Coder-1.3B (Float16, ~3 GB VRAM)")
    elif MODEL_CHOICE == "6.7b":
        print("   Architecture: DeepSeek-Coder-6.7B (4-bit quantized, ~5-6 GB VRAM)")

    # Show model type
    if USE_FINETUNED:
        print(f"   Type: FINE-TUNED MODEL (with LoRA adapters)")
        print(f"   Adapters: {FINETUNED_MODEL_PATH}")
    else:
        print("   Type: BASE MODEL (standard pre-trained)")

    # Show ranking mode
    print(f"\n📊 LLM RANKING MODE:")
    if USE_BATCH_RANKING:
        print(f"   Mode: BATCH RANKING (1 LLM call for all candidates)")
        print(f"   Candidates: Top-{BATCH_RANKING_SIZE} from reranker")
        print(f"   Speed: ~10x faster than individual scoring")
    else:
        print(f"   Mode: INDIVIDUAL SCORING (10 separate LLM calls)")
        print(f"   Candidates: Top-10 from reranker")
        print(f"   Speed: Slower, legacy mode")

    print("\n   💡 To switch modes: Change USE_BATCH_RANKING in rag_chat.py line 19")
    print("      USE_BATCH_RANKING = True   →  Batch Mode (FAST, recommended)")
    print("      USE_BATCH_RANKING = False  →  Individual Mode (SLOW, legacy)")
    print("\n   💡 To change candidate count: Change BATCH_RANKING_SIZE in rag_chat.py line 22")
    print("      Recommended: 5-10 candidates for best speed/accuracy balance")
    print()

    # Default codebase path
    default_codebase = os.path.join(os.path.dirname(__file__), "codebase")

    if len(sys.argv) < 2:
        codebase_path = default_codebase
        print(f"Using default codebase directory: {codebase_path}")
        print("Copy your code into ./codebase/ or specify a path:")
        print("  python rag_chat.py <path_to_codebase>\n")
    else:
        codebase_path = sys.argv[1]

    # Initialize improved RAG system
    rag = ImprovedRAGSystem()

    # Check if already indexed
    if rag.collection.count() == 0:
        print(f"Indexing codebase with improved chunking...")
        rag.index_codebase(codebase_path)
    else:
        print(f"Using existing index ({rag.collection.count()} chunks)")
        print("Note: Delete ./chroma_db/ folder to re-index with improvements\n")

    # Load LLM with selected model
    model, tokenizer = load_model(MODEL_CHOICE, USE_FINETUNED, FINETUNED_MODEL_PATH)

    # Interactive search
    interactive_rag_search(rag, model, tokenizer)


if __name__ == "__main__":
    main()
