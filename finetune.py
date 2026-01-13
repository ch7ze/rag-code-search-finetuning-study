"""
Fine-Tuning Script for DeepSeek-Coder on Project Codebase
Uses QLoRA (4-bit quantization) for memory-efficient fine-tuning on RTX 4070

Based on Exposé Section 4.2:
- Self-supervised learning from AST-based chunks
- Docstrings/function names → locations as training pairs
- QLoRA with 4-bit NormalFloat quantization
- Hyperparameters: lr=2e-4, rank=16, alpha=32, 3-5 epochs
"""

import os
import json
import torch
from pathlib import Path
from typing import List, Dict, Tuple
from dataclasses import dataclass
from datetime import datetime

from transformers import (
    AutoModelForCausalLM,
    AutoTokenizer,
    BitsAndBytesConfig,
    TrainingArguments,
    Trainer,
    default_data_collator
)
from peft import (
    LoraConfig,
    get_peft_model,
    prepare_model_for_kbit_training,
    TaskType
)
from datasets import Dataset

from rag_system import ImprovedCodeChunker


# ============================================================================
# CONFIGURATION
# ============================================================================
MODEL_NAME = "deepseek-ai/deepseek-coder-6.7b-instruct"
OUTPUT_DIR = "./finetuned_model"
TRAINING_DATA_PATH = "./training_data.json"
CODEBASE_PATH = "./codebase"

# QLoRA Configuration (from Exposé)
LORA_RANK = 16
LORA_ALPHA = 32
LORA_DROPOUT = 0.05
LEARNING_RATE = 2e-4
NUM_EPOCHS = 3
BATCH_SIZE = 1  # Reduced for RTX 4070 (8GB VRAM)
GRADIENT_ACCUMULATION_STEPS = 16  # Effective batch size = 1*16=16 (same as before)
MAX_SEQ_LENGTH = 1024  # Reduced from 2048 to save memory
VALIDATION_SPLIT = 0.1  # 10% validation

# GPU Thermal Management
SLEEP_BETWEEN_STEPS = 0  # Pause 0.5 seconds between steps to cool GPU

# 4-bit Quantization Config
QUANTIZATION_CONFIG = BitsAndBytesConfig(
    load_in_4bit=True,
    bnb_4bit_compute_dtype=torch.float16,
    bnb_4bit_use_double_quant=True,
    bnb_4bit_quant_type="nf4"  # NormalFloat as specified in Exposé
)


@dataclass
class TrainingExample:
    """Single training example: query → location"""
    query: str
    location: str
    code_snippet: str
    function_name: str


class CodeSearchDatasetGenerator:
    """
    Generate self-supervised training data from codebase chunks.
    Each function/class becomes a training example:
    - Query: docstring or synthetic description
    - Target: file_path:function_name
    """

    def __init__(self):
        self.chunker = ImprovedCodeChunker()

    def generate_training_data(self, codebase_path: str) -> List[TrainingExample]:
        """
        Generate training examples from all code chunks in codebase.

        Returns:
            List of TrainingExample objects
        """
        print(f"Generating training data from: {codebase_path}")

        codebase_path = Path(codebase_path)
        extensions = ['.rs', '.js']  # Only Rust and JavaScript (not HTML/CSS)

        all_examples = []

        for ext in extensions:
            files = list(codebase_path.rglob(f'*{ext}'))
            print(f"Processing {len(files)} {ext} files...")

            for file_path in files:
                try:
                    content = file_path.read_text(encoding='utf-8')
                    chunks = self.chunker.chunk_file(str(file_path), content)

                    for chunk in chunks:
                        examples = self._chunk_to_examples(chunk)
                        all_examples.extend(examples)

                except Exception as e:
                    print(f"Warning: Could not process {file_path}: {e}")

        print(f"✓ Generated {len(all_examples)} training examples")
        return all_examples

    def _chunk_to_examples(self, chunk: Dict) -> List[TrainingExample]:
        """
        Convert a code chunk into multiple training examples.

        Strategy:
        1. If docstring exists: use it as query
        2. If no docstring: generate synthetic query from function name
        3. Create variations for data augmentation
        """
        examples = []

        function_name = chunk.get('name', 'unknown')
        location = chunk['location']
        code = chunk['code']
        docstring = chunk.get('docstring', '')

        # Skip if not a proper function/class
        if chunk.get('type') not in ['function_item', 'function_declaration',
                                       'method_definition', 'struct_item', 'enum_item']:
            return examples

        # Example 1: Use docstring as query (if available)
        if docstring and len(docstring) > 10:
            examples.append(TrainingExample(
                query=docstring.strip(),
                location=location,
                code_snippet=code[:500],  # First 500 chars
                function_name=function_name
            ))

        # Example 2: Generate synthetic query from function name
        synthetic_query = self._generate_synthetic_query(function_name, chunk)
        if synthetic_query:
            examples.append(TrainingExample(
                query=synthetic_query,
                location=location,
                code_snippet=code[:500],
                function_name=function_name
            ))

        return examples

    def _generate_synthetic_query(self, function_name: str, chunk: Dict) -> str:
        """
        Generate a synthetic query from function name and context.

        Examples:
        - create_jwt → "How do I create a JWT token?"
        - validate_user → "Where is user validation implemented?"
        - handle_websocket_message → "Where is WebSocket message handling?"
        """
        # Remove common prefixes
        name = function_name.replace('handle_', '').replace('create_', '').replace('get_', '')
        name = name.replace('validate_', '').replace('send_', '').replace('process_', '')

        # Convert snake_case/camelCase to words
        words = []
        for word in name.replace('_', ' ').split():
            # Split camelCase
            import re
            camel_words = re.sub('([A-Z][a-z]+)', r' \1', word).split()
            words.extend(camel_words)

        # Clean up
        words = [w.lower().strip() for w in words if w.strip()]
        if not words:
            return ""

        # Generate query based on function name pattern
        if 'create' in function_name.lower():
            return f"How do I create {' '.join(words)}?"
        elif 'validate' in function_name.lower() or 'check' in function_name.lower():
            return f"Where is {' '.join(words)} validation?"
        elif 'handle' in function_name.lower():
            return f"Where is {' '.join(words)} handler?"
        elif 'get' in function_name.lower() or 'fetch' in function_name.lower():
            return f"How do I get {' '.join(words)}?"
        elif 'send' in function_name.lower():
            return f"How do I send {' '.join(words)}?"
        else:
            return f"Where is the {' '.join(words)} function?"


def prepare_dataset(examples: List[TrainingExample], tokenizer) -> Tuple[Dataset, Dataset]:
    """
    Convert training examples to HuggingFace Dataset format.

    Format for instruction-tuning:
    ### Instruction: Find the code that answers this question.
    QUESTION: {query}
    ### Response: The code is located at: {location}
    """
    print("Preparing dataset for instruction-tuning...")

    formatted_examples = []

    for ex in examples:
        # Instruction-tuning format for DeepSeek-Coder
        prompt = f"""### Instruction: Find the code that answers this question.

QUESTION: {ex.query}

### Response: The code is located at: {ex.location}"""

        formatted_examples.append({
            'text': prompt,
            'query': ex.query,
            'location': ex.location
        })

    # Create HuggingFace Dataset
    dataset = Dataset.from_list(formatted_examples)

    # Tokenize
    def tokenize_function(examples):
        # Tokenize with truncation only (no padding - done by collator)
        tokenized = tokenizer(
            examples['text'],
            truncation=True,
            max_length=MAX_SEQ_LENGTH,
            padding=False,  # No padding here - will be done by data collator
            return_tensors=None  # Return as lists, not tensors
        )

        # For causal LM, labels are the same as input_ids
        tokenized['labels'] = tokenized['input_ids'].copy()
        return tokenized

    print("Tokenizing dataset...")
    tokenized_dataset = dataset.map(
        tokenize_function,
        batched=True,
        remove_columns=['text', 'query', 'location'],
        desc="Tokenizing"
    )

    # Split into train/validation
    split_dataset = tokenized_dataset.train_test_split(
        test_size=VALIDATION_SPLIT,
        seed=42
    )

    train_dataset = split_dataset['train']
    val_dataset = split_dataset['test']

    print(f"✓ Training examples: {len(train_dataset)}")
    print(f"✓ Validation examples: {len(val_dataset)}")

    return train_dataset, val_dataset


def setup_model_and_tokenizer():
    """
    Load base model with 4-bit quantization and prepare for QLoRA training.
    """
    print(f"Loading base model: {MODEL_NAME}")
    print("Using 4-bit quantization for memory efficiency...")

    # Load tokenizer
    tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME, trust_remote_code=True)

    # Set padding token (required for batch training)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
        tokenizer.pad_token_id = tokenizer.eos_token_id

    # Load model config first to modify it
    from transformers import AutoConfig
    config = AutoConfig.from_pretrained(MODEL_NAME, trust_remote_code=True)

    # CRITICAL: Disable RoPE scaling for training (causes attention mask issues)
    config.rope_scaling = None

    # Load model with 4-bit quantization and modified config
    model = AutoModelForCausalLM.from_pretrained(
        MODEL_NAME,
        config=config,  # Use modified config without RoPE scaling
        quantization_config=QUANTIZATION_CONFIG,
        device_map="auto",
        trust_remote_code=True,
        torch_dtype=torch.float16
    )

    print("✓ Model loaded with 4-bit quantization (RoPE scaling disabled for training)")

    # Prepare model for k-bit training (explicitly disable gradient checkpointing)
    model = prepare_model_for_kbit_training(model, use_gradient_checkpointing=False)

    # Ensure gradient checkpointing is OFF (causes attention mask issues with RoPE scaling)
    if hasattr(model, 'gradient_checkpointing_disable'):
        model.gradient_checkpointing_disable()

    # Configure LoRA (reduced target modules to save memory)
    lora_config = LoraConfig(
        r=LORA_RANK,                    # Rank (from Exposé)
        lora_alpha=LORA_ALPHA,
        target_modules=[
            "q_proj", "k_proj", "v_proj", "o_proj"
        ],
        lora_dropout=LORA_DROPOUT,
        bias="none",
        task_type=TaskType.CAUSAL_LM
    )

    # Apply LoRA to model
    model = get_peft_model(model, lora_config)

    # Print trainable parameters
    trainable_params = sum(p.numel() for p in model.parameters() if p.requires_grad)
    total_params = sum(p.numel() for p in model.parameters())
    print(f"✓ LoRA adapters added:")
    print(f"  Trainable params: {trainable_params:,} ({100 * trainable_params / total_params:.2f}%)")
    print(f"  Total params: {total_params:,}")

    return model, tokenizer


def train_model(model, tokenizer, train_dataset, val_dataset):
    """
    Fine-tune model using QLoRA with specified hyperparameters.
    """
    print("\n" + "="*70)
    print("STARTING FINE-TUNING")
    print("="*70)

    # Training arguments (from Exposé)
    training_args = TrainingArguments(
        output_dir=OUTPUT_DIR,

        # Training hyperparameters (from Exposé)
        num_train_epochs=NUM_EPOCHS,
        learning_rate=LEARNING_RATE,

        # Batch size and gradient accumulation
        per_device_train_batch_size=BATCH_SIZE,
        per_device_eval_batch_size=BATCH_SIZE,
        gradient_accumulation_steps=GRADIENT_ACCUMULATION_STEPS,

        # Optimization
        optim="paged_adamw_8bit",  # Memory-efficient optimizer for 4-bit training
        warmup_steps=10,  # Reduced from 100 (10% of ~100 total steps)
        weight_decay=0.01,

        # Logging and evaluation
        logging_steps=5,
        evaluation_strategy="epoch",  # Evaluate at end of each epoch instead of every N steps
        save_strategy="epoch",  # Save at end of each epoch
        save_total_limit=2,  # Keep only 2 best checkpoints to save disk space

        # Performance
        fp16=True,
        gradient_checkpointing=False,  # Already enabled in model setup
        max_grad_norm=0.3,  # Gradient clipping for stability

        # Other
        load_best_model_at_end=False,  # Disabled to save memory (we save best manually)
        metric_for_best_model="eval_loss",
        greater_is_better=False,
        report_to="none",  # Disable wandb/tensorboard
        remove_unused_columns=True,
    )

    # Data collator for language modeling with dynamic padding
    from transformers import DataCollatorForLanguageModeling
    data_collator = DataCollatorForLanguageModeling(
        tokenizer=tokenizer,
        mlm=False,  # Causal LM (not masked LM)
        pad_to_multiple_of=8  # Pad to multiple of 8 for efficiency
    )

    # Custom Trainer with cooling pauses to prevent overheating
    class CoolingTrainer(Trainer):
        def training_step(self, model, inputs):
            # Normal training step
            loss = super().training_step(model, inputs)

            # Add cooling pause every step to prevent GPU overheating
            if SLEEP_BETWEEN_STEPS > 0:
                import time
                time.sleep(SLEEP_BETWEEN_STEPS)

            return loss

    # Initialize trainer
    trainer = CoolingTrainer(
        model=model,
        args=training_args,
        train_dataset=train_dataset,
        eval_dataset=val_dataset,
        data_collator=data_collator,
    )

    # Train
    print(f"\nStarting training for {NUM_EPOCHS} epochs...")
    print(f"Effective batch size: {BATCH_SIZE * GRADIENT_ACCUMULATION_STEPS}")
    print(f"Training examples: {len(train_dataset)}")
    print(f"Validation examples: {len(val_dataset)}")
    print(f"Estimated steps per epoch: {len(train_dataset) // (BATCH_SIZE * GRADIENT_ACCUMULATION_STEPS)}")
    print(f"\n⚠ GPU Cooling: {SLEEP_BETWEEN_STEPS}s pause between steps to prevent overheating")
    print("This will take 3-5 hours on RTX 4070 (longer due to cooling pauses)...\n")

    train_result = trainer.train()

    # Save final model
    print("\n" + "="*70)
    print("TRAINING COMPLETE")
    print("="*70)
    print(f"Final training loss: {train_result.training_loss:.4f}")

    # Save LoRA adapters
    model.save_pretrained(OUTPUT_DIR)
    tokenizer.save_pretrained(OUTPUT_DIR)

    print(f"\n✓ Fine-tuned model saved to: {OUTPUT_DIR}")
    print(f"  - LoRA adapters: {OUTPUT_DIR}/adapter_model.bin")
    print(f"  - Config: {OUTPUT_DIR}/adapter_config.json")

    # Save training metadata
    metadata = {
        'base_model': MODEL_NAME,
        'lora_rank': LORA_RANK,
        'lora_alpha': LORA_ALPHA,
        'learning_rate': LEARNING_RATE,
        'num_epochs': NUM_EPOCHS,
        'training_examples': len(train_dataset),
        'validation_examples': len(val_dataset),
        'final_train_loss': float(train_result.training_loss),
        'timestamp': datetime.now().isoformat(),
        'codebase_path': CODEBASE_PATH
    }

    with open(f"{OUTPUT_DIR}/training_metadata.json", 'w') as f:
        json.dump(metadata, f, indent=2)

    print(f"✓ Training metadata saved to: {OUTPUT_DIR}/training_metadata.json")

    return trainer


def main():
    print("="*70)
    print("DeepSeek-Coder Fine-Tuning for Code Search")
    print("QLoRA (4-bit) Self-Supervised Learning")
    print("="*70)
    print(f"\nBase Model: {MODEL_NAME}")
    print(f"Output Directory: {OUTPUT_DIR}")
    print(f"Codebase: {CODEBASE_PATH}")
    print(f"\nHyperparameters (from Exposé):")
    print(f"  LoRA Rank: {LORA_RANK}")
    print(f"  LoRA Alpha: {LORA_ALPHA}")
    print(f"  Learning Rate: {LEARNING_RATE}")
    print(f"  Epochs: {NUM_EPOCHS}")
    print(f"  Batch Size: {BATCH_SIZE} (effective: {BATCH_SIZE * GRADIENT_ACCUMULATION_STEPS})")
    print(f"  Max Sequence Length: {MAX_SEQ_LENGTH}")
    print(f"  Validation Split: {VALIDATION_SPLIT * 100}%")
    print()

    # Check if codebase exists
    if not os.path.exists(CODEBASE_PATH):
        print(f"ERROR: Codebase not found at {CODEBASE_PATH}")
        print("Please ensure the codebase directory exists.")
        return

    # Step 1: Generate training data
    print("STEP 1/4: Generating Training Data")
    print("-" * 70)
    generator = CodeSearchDatasetGenerator()
    training_examples = generator.generate_training_data(CODEBASE_PATH)

    if len(training_examples) == 0:
        print("ERROR: No training examples generated!")
        return

    # Save training data for inspection
    print(f"\nSaving training data to: {TRAINING_DATA_PATH}")
    with open(TRAINING_DATA_PATH, 'w', encoding='utf-8') as f:
        json.dump([{
            'query': ex.query,
            'location': ex.location,
            'function_name': ex.function_name
        } for ex in training_examples], f, indent=2, ensure_ascii=False)
    print(f"✓ Training data saved")

    # Step 2: Load model and tokenizer
    print("\n" + "="*70)
    print("STEP 2/4: Loading Model and Tokenizer")
    print("-" * 70)
    model, tokenizer = setup_model_and_tokenizer()

    # Step 3: Prepare datasets
    print("\n" + "="*70)
    print("STEP 3/4: Preparing Datasets")
    print("-" * 70)
    train_dataset, val_dataset = prepare_dataset(training_examples, tokenizer)

    # Step 4: Train
    print("\n" + "="*70)
    print("STEP 4/4: Fine-Tuning")
    print("-" * 70)
    trainer = train_model(model, tokenizer, train_dataset, val_dataset)

    print("\n" + "="*70)
    print("FINE-TUNING COMPLETE!")
    print("="*70)
    print(f"\nTo use the fine-tuned model:")
    print(f"  1. Set FINETUNED_MODEL_PATH = '{OUTPUT_DIR}' in rag_chat.py")
    print(f"  2. Run evaluation: python evaluate.py")
    print()


if __name__ == "__main__":
    main()
