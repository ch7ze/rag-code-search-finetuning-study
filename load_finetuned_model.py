"""
Load Fine-Tuned DeepSeek-Coder Model with LoRA Adapters

This module provides functions to load both:
1. Base Model (for baseline comparison)
2. Fine-Tuned Model (with LoRA adapters)

Usage:
    from load_finetuned_model import load_finetuned_model, load_base_model

    # Load fine-tuned model
    model, tokenizer = load_finetuned_model("./finetuned_model")

    # Load base model (for comparison)
    model, tokenizer = load_base_model()
"""

import os
import json
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig
from peft import PeftModel


def load_base_model(model_choice: str = "6.7b"):
    """
    Load base DeepSeek-Coder model WITHOUT fine-tuning.

    Args:
        model_choice: "1.3b" or "6.7b"

    Returns:
        (model, tokenizer) tuple
    """
    if model_choice == "6.7b":
        print("Loading BASE DeepSeek-Coder-6.7B-Instruct (4-bit quantized)...")
        model_name = "deepseek-ai/deepseek-coder-6.7b-instruct"

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
        print("✓ BASE model loaded (4-bit quantized, ~5-6 GB VRAM)\n")

    else:  # 1.3b
        print("Loading BASE DeepSeek-Coder-1.3B-Instruct...")
        model_name = "deepseek-ai/deepseek-coder-1.3b-instruct"

        model = AutoModelForCausalLM.from_pretrained(
            model_name,
            device_map="cuda:0",
            trust_remote_code=True,
            torch_dtype=torch.float16
        )

        tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=True)
        print("✓ BASE model loaded (Float16, ~3 GB VRAM)\n")

    return model, tokenizer


def load_finetuned_model(adapter_path: str, model_choice: str = "6.7b"):
    """
    Load fine-tuned DeepSeek-Coder model with LoRA adapters.

    Args:
        adapter_path: Path to directory containing LoRA adapters
                      (e.g., "./finetuned_model")
        model_choice: "1.3b" or "6.7b" (should match the base model used for training)

    Returns:
        (model, tokenizer) tuple with LoRA adapters merged

    Raises:
        FileNotFoundError: If adapter path doesn't exist
        ValueError: If adapter config is invalid
    """
    # Validate adapter path
    if not os.path.exists(adapter_path):
        raise FileNotFoundError(f"Adapter path not found: {adapter_path}")

    adapter_config_path = os.path.join(adapter_path, "adapter_config.json")
    if not os.path.exists(adapter_config_path):
        raise FileNotFoundError(f"Adapter config not found: {adapter_config_path}")

    print(f"Loading FINE-TUNED DeepSeek-Coder with LoRA adapters from: {adapter_path}")

    # Load training metadata if available
    metadata_path = os.path.join(adapter_path, "training_metadata.json")
    if os.path.exists(metadata_path):
        with open(metadata_path, 'r') as f:
            metadata = json.load(f)
        print(f"\nTraining Metadata:")
        print(f"  Base Model: {metadata.get('base_model', 'unknown')}")
        print(f"  LoRA Rank: {metadata.get('lora_rank', 'unknown')}")
        print(f"  Training Examples: {metadata.get('training_examples', 'unknown')}")
        print(f"  Final Train Loss: {metadata.get('final_train_loss', 'unknown')}")
        print(f"  Trained on: {metadata.get('timestamp', 'unknown')}")
        print()

    # Determine base model name
    if model_choice == "6.7b":
        base_model_name = "deepseek-ai/deepseek-coder-6.7b-instruct"
        use_4bit = True
    else:
        base_model_name = "deepseek-ai/deepseek-coder-1.3b-instruct"
        use_4bit = False

    print(f"Loading base model: {base_model_name}")

    # Load tokenizer
    tokenizer = AutoTokenizer.from_pretrained(adapter_path, trust_remote_code=True)

    # Load base model with same quantization as training
    if use_4bit:
        quantization_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_compute_dtype=torch.float16,
            bnb_4bit_use_double_quant=True,
            bnb_4bit_quant_type="nf4"
        )

        base_model = AutoModelForCausalLM.from_pretrained(
            base_model_name,
            device_map="auto",
            quantization_config=quantization_config,
            trust_remote_code=True,
            torch_dtype=torch.float16
        )
    else:
        base_model = AutoModelForCausalLM.from_pretrained(
            base_model_name,
            device_map="cuda:0",
            trust_remote_code=True,
            torch_dtype=torch.float16
        )

    print("✓ Base model loaded")

    # Load LoRA adapters
    print(f"Loading LoRA adapters from: {adapter_path}")
    model = PeftModel.from_pretrained(base_model, adapter_path)

    print("✓ LoRA adapters loaded and merged")

    # Print model info
    print("\nModel Info:")
    print(f"  Type: Fine-Tuned with LoRA")
    print(f"  Adapter Path: {adapter_path}")
    print(f"  Memory: ~5-6 GB VRAM (4-bit)" if use_4bit else f"  Memory: ~3 GB VRAM")
    print()

    return model, tokenizer


def compare_models_info(base_model, finetuned_model):
    """
    Compare base and fine-tuned models to verify LoRA adapters are loaded.

    Args:
        base_model: Base model without adapters
        finetuned_model: Model with LoRA adapters
    """
    print("="*70)
    print("MODEL COMPARISON")
    print("="*70)

    # Count parameters
    base_params = sum(p.numel() for p in base_model.parameters())
    finetuned_params = sum(p.numel() for p in finetuned_model.parameters())
    trainable_params = sum(p.numel() for p in finetuned_model.parameters() if p.requires_grad)

    print(f"\nBase Model Parameters: {base_params:,}")
    print(f"Fine-Tuned Model Parameters: {finetuned_params:,}")
    print(f"LoRA Trainable Parameters: {trainable_params:,} ({100 * trainable_params / finetuned_params:.2f}%)")

    # Check if model is a PEFT model
    from peft import PeftModel
    is_peft = isinstance(finetuned_model, PeftModel)
    print(f"\nIs PEFT Model: {is_peft}")

    if is_peft:
        print("✓ LoRA adapters successfully loaded!")
    else:
        print("⚠ Warning: Model doesn't appear to have LoRA adapters")

    print("="*70)


# Example usage and testing
if __name__ == "__main__":
    import sys

    print("="*70)
    print("Fine-Tuned Model Loader - Test")
    print("="*70)

    # Default paths
    adapter_path = "./finetuned_model"

    if len(sys.argv) > 1:
        adapter_path = sys.argv[1]

    print(f"\nTesting fine-tuned model loading...")
    print(f"Adapter path: {adapter_path}\n")

    try:
        # Test loading fine-tuned model
        model, tokenizer = load_finetuned_model(adapter_path)

        print("\n✓ Successfully loaded fine-tuned model!")

        # Test inference
        print("\nTesting inference...")
        test_prompt = "### Instruction: Find the code that answers this question.\n\nQUESTION: How do I create a JWT token?\n\n### Response:"

        inputs = tokenizer(test_prompt, return_tensors="pt").to(model.device)

        with torch.no_grad():
            outputs = model.generate(
                **inputs,
                max_new_tokens=50,
                temperature=0.1,
                do_sample=False
            )

        response = tokenizer.decode(outputs[0][inputs.input_ids.shape[1]:], skip_special_tokens=True)
        print(f"\nTest Response: {response}")

        print("\n✓ Inference test successful!")

    except Exception as e:
        print(f"\n✗ Error loading model: {e}")
        import traceback
        traceback.print_exc()
