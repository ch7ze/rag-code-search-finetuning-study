"""
Generate LLM-based summaries for all functions in the codebase.
Uses the existing RAG system's chunking to extract functions,
then generates optimized summaries for code search.

Output: function_summaries.json - can be used for:
1. RAG system enhancement (better retrieval)
2. Fine-tuning data generation
3. Documentation generation
"""

import json
import os
from pathlib import Path
from typing import List, Dict
from tqdm import tqdm

# Use existing RAG system chunker
from rag_system import ImprovedCodeChunker

# LLM for summary generation
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer


class FunctionSummaryGenerator:
    """Generates code-search optimized summaries for all functions"""

    def __init__(self, model_name: str = "deepseek-ai/deepseek-coder-6.7b-instruct"):
        """
        Initialize the summary generator with a code LLM.

        Args:
            model_name: HuggingFace model ID (default: DeepSeek-Coder 6.7B)
        """
        print(f"Loading LLM for summary generation: {model_name}")
        print("(This may take a minute...)")

        self.tokenizer = AutoTokenizer.from_pretrained(
            model_name,
            trust_remote_code=True
        )

        self.model = AutoModelForCausalLM.from_pretrained(
            model_name,
            torch_dtype=torch.float16,
            device_map="auto",
            trust_remote_code=True
        )

        self.chunker = ImprovedCodeChunker()
        print("✓ LLM and chunker loaded\n")

    def create_summary_prompt(self, chunk: Dict) -> str:
        """
        Create a prompt optimized for generating code-search summaries.

        The summary should include:
        - What the function does (high-level purpose)
        - Key parameters and their purpose
        - Return value description
        - Important implementation details (e.g., "uses bcrypt", "validates JWT")
        - Keywords relevant for search
        """
        code = chunk['code']
        name = chunk.get('name', 'unknown')
        func_type = chunk.get('type', 'function')

        prompt = f"""You are a technical documentation expert. Generate a concise summary for this {func_type} that will help developers find it using code search.

Function name: {name}

Code:
```
{code[:1000]}
```

Generate a 2-3 sentence summary that includes:
1. What the function does (purpose)
2. Key parameters and return value
3. Important technical details (e.g., algorithms used, external services called, validation performed)
4. Keywords that developers might search for

Summary:"""

        return prompt

    def generate_summary(self, chunk: Dict) -> str:
        """
        Generate LLM summary for a single function.

        Returns:
            Summary string optimized for code search
        """
        prompt = self.create_summary_prompt(chunk)

        # Tokenize
        inputs = self.tokenizer(prompt, return_tensors="pt").to(self.model.device)

        # Generate
        with torch.no_grad():
            outputs = self.model.generate(
                **inputs,
                max_new_tokens=150,
                temperature=0.3,
                do_sample=True,
                top_p=0.9,
                pad_token_id=self.tokenizer.eos_token_id
            )

        # Decode
        full_response = self.tokenizer.decode(outputs[0], skip_special_tokens=True)

        # Extract only the summary (after "Summary:")
        if "Summary:" in full_response:
            summary = full_response.split("Summary:")[-1].strip()
        else:
            summary = full_response[len(prompt):].strip()

        # Clean up
        summary = summary.split('\n')[0]  # Take first line only
        summary = summary.strip()

        return summary

    def extract_all_functions(self, codebase_path: str) -> List[Dict]:
        """
        Extract all functions from codebase using RAG system's chunker.

        Args:
            codebase_path: Path to codebase directory

        Returns:
            List of function chunks
        """
        print(f"Extracting functions from: {codebase_path}\n")

        codebase_path = Path(codebase_path)
        extensions = ['.rs', '.js']

        all_chunks = []

        for ext in extensions:
            files = list(codebase_path.rglob(f'*{ext}'))
            print(f"Found {len(files)} {ext} files")

            for file_path in files:
                try:
                    content = file_path.read_text(encoding='utf-8')
                    chunks = self.chunker.chunk_file(str(file_path), content)
                    all_chunks.extend(chunks)
                except Exception as e:
                    print(f"Warning: Could not parse {file_path}: {e}")

        print(f"\n✓ Total functions extracted: {len(all_chunks)}\n")
        return all_chunks

    def generate_all_summaries(self, codebase_path: str, output_file: str = "function_summaries.json"):
        """
        Generate summaries for all functions and save to JSON file.

        Args:
            codebase_path: Path to codebase
            output_file: Output JSON file path
        """
        # Extract all functions
        chunks = self.extract_all_functions(codebase_path)

        # Generate summaries
        print("Generating LLM summaries for all functions...")
        print("(This will take several minutes)\n")

        summaries = []

        for i, chunk in enumerate(tqdm(chunks, desc="Generating summaries")):
            try:
                summary = self.generate_summary(chunk)

                summaries.append({
                    "location": chunk['location'],
                    "name": chunk['name'],
                    "type": chunk.get('type', 'unknown'),
                    "start_line": chunk.get('start_line', 0),
                    "original_docstring": chunk.get('docstring', ''),
                    "llm_summary": summary,
                    "code_preview": chunk['code'][:200] + "..." if len(chunk['code']) > 200 else chunk['code']
                })

                # Clear GPU cache periodically
                if i % 10 == 0:
                    torch.cuda.empty_cache()

            except Exception as e:
                print(f"\nError generating summary for {chunk['location']}: {e}")
                summaries.append({
                    "location": chunk['location'],
                    "name": chunk['name'],
                    "type": chunk.get('type', 'unknown'),
                    "start_line": chunk.get('start_line', 0),
                    "original_docstring": chunk.get('docstring', ''),
                    "llm_summary": "",
                    "error": str(e)
                })

        # Save to JSON
        output_data = {
            "metadata": {
                "total_functions": len(summaries),
                "codebase_path": str(codebase_path),
                "model_used": "deepseek-coder-6.7b-instruct",
                "summary_count": len([s for s in summaries if s.get('llm_summary')])
            },
            "summaries": summaries
        }

        with open(output_file, 'w', encoding='utf-8') as f:
            json.dump(output_data, f, indent=2, ensure_ascii=False)

        print(f"\n✓ Summaries saved to: {output_file}")
        print(f"  Total functions: {len(summaries)}")
        print(f"  Successful summaries: {len([s for s in summaries if s.get('llm_summary')])}")

        return summaries


def main():
    """Main entry point"""
    import sys

    # Configuration
    CODEBASE_PATH = "./codebase"
    OUTPUT_FILE = "function_summaries.json"

    print("="*80)
    print("FUNCTION SUMMARY GENERATOR")
    print("="*80)
    print()
    print("This script will:")
    print("1. Extract all functions from your codebase (using RAG system chunker)")
    print("2. Generate LLM-based summaries optimized for code search")
    print("3. Save to JSON file for flexible usage")
    print()
    print(f"Codebase: {CODEBASE_PATH}")
    print(f"Output: {OUTPUT_FILE}")
    print()

    # Check if codebase exists
    if not os.path.exists(CODEBASE_PATH):
        print(f"ERROR: Codebase not found at {CODEBASE_PATH}")
        sys.exit(1)

    # Initialize generator
    generator = FunctionSummaryGenerator()

    # Generate summaries
    summaries = generator.generate_all_summaries(CODEBASE_PATH, OUTPUT_FILE)

    # Show some examples
    print("\n" + "="*80)
    print("EXAMPLE SUMMARIES (first 3 functions)")
    print("="*80)

    for i, summary in enumerate(summaries[:3], 1):
        print(f"\n{i}. {summary['location']}")
        print(f"   Name: {summary['name']}")
        print(f"   Original docstring: {summary.get('original_docstring', 'None')[:60]}...")
        print(f"   LLM summary: {summary.get('llm_summary', 'ERROR')}")

    print("\n" + "="*80)
    print("✓ COMPLETE!")
    print("="*80)
    print()
    print("You can now use function_summaries.json for:")
    print("  1. RAG system enhancement (load summaries during indexing)")
    print("  2. Fine-tuning data generation")
    print("  3. Documentation generation")
    print()


if __name__ == "__main__":
    main()
