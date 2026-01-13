"""
Improved RAG System for Code Search
Enhancements:
1. Docstring extraction
2. Context enrichment (function name + class + comments)
3. Hybrid search (Vector + BM25)
4. Re-ranking with cross-encoder
"""

# ============================================================================
# RAG RETRIEVAL CONFIGURATION
# ============================================================================
# To restore old c378578 behavior (better file match @ position 1):
#   SET: USE_FUNCTION_NAME_BOOSTING = True
#   SET: USE_SIGNATURE_ONLY = True
#   SET: USE_QUERY_EXPANSION = False
#   SET: CANDIDATE_POOL_SIZE = 20 (or 50 if using evaluate.py with top_k=40)
# ============================================================================

USE_FUNCTION_NAME_BOOSTING = True   # True  = Apply function name matching boost (old c378578 behavior)
                                     # False = No boosting, pure cross-encoder scores

USE_SIGNATURE_ONLY = True            # True  = Use only function signature (first line) for re-ranking
                                     # False = Use first 20 lines of code for re-ranking

USE_QUERY_EXPANSION = True          # True  = Expand query with variations ("implement X", "function that X")
                                     # False = Use original query only

CANDIDATE_POOL_SIZE = 40             # Number of candidates from vector/BM25
                                     # Old c378578: 20 (with top_k=10)
                                     # Current:     50 (compatible with evaluate.py top_k=40)
                                     # Max recall:  100
                                     # NOTE: Must be ≥ top_k in retrieve() calls

# ============================================================================
# FILE-LEVEL RETRIEVAL CONFIGURATION (NEW)
# ============================================================================
# Options to prioritize finding the correct FILE over the correct FUNCTION

USE_FILE_SCORE_AGGREGATION = False  # True  = Aggregate function scores by file, prioritize best files
                                     # False = Rank individual functions independently
                                     # Recommendation: True for better file-level accuracy

FILE_AGGREGATION_STRATEGY = "max"   # Strategy for aggregating function scores per file:
                                     # "max"     = Best function represents the file
                                     # "mean"    = Average of all function scores
                                     # "weighted"= Mean + Max boost (balanced approach)
                                     # "count"   = Sum weighted by number of matches

FILE_VS_FUNCTION_WEIGHT = 0.7       # Weight for file score vs function score (0.0-1.0)
                                     # 0.7 = 70% file score, 30% function score
                                     # 1.0 = 100% file score (pure file-level retrieval)
                                     # 0.0 = 100% function score (original behavior)

USE_FILE_SUMMARY_CHUNKS = True     # True  = Create and index file-level summary chunks
                                     # False = Only index individual functions
                                     # Note: Requires re-indexing when changed

USE_TWO_STAGE_FILE_RETRIEVAL = True # True  = Two-stage: 1) Find best files, 2) Search within files
                                      # False = Single-stage retrieval (original behavior)
                                      # Note: Only works if USE_FILE_SUMMARY_CHUNKS = True

FILE_RETRIEVAL_TOP_K = 3             # Number of top files to retrieve in two-stage mode
                                     # Recommendation: 2-5 files

# ============================================================================

import os
import re
import math
from pathlib import Path
from typing import List, Dict, Tuple, Optional
from tree_sitter_language_pack import get_parser
from sentence_transformers import SentenceTransformer, CrossEncoder
import chromadb
from rank_bm25 import BM25Okapi
import numpy as np
import torch
from transformers import AutoTokenizer, AutoModel


def sanitize_score(score) -> float:
    """
    Convert score to Python float and validate (no NaN/Infinity).
    Returns 0.0 for invalid scores to ensure JSON serialization works.
    """
    try:
        score_float = float(score)
        if math.isnan(score_float) or math.isinf(score_float):
            print(f"WARNING: Invalid score detected (NaN or Infinity), using 0.0 instead")
            return 0.0
        return score_float
    except (TypeError, ValueError):
        print(f"WARNING: Could not convert score to float: {score}, using 0.0")
        return 0.0


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


def extract_function_signature(code: str, language: str = 'rust') -> str:
    """
    Extract the full function signature from code.
    Returns the first line(s) containing the function definition.
    """
    lines = code.split('\n')

    if language == 'rust':
        # Look for function definition (pub fn, async fn, fn)
        for i, line in enumerate(lines):
            if 'fn ' in line and '{' not in line:
                # Multi-line signature - collect until opening brace
                sig_lines = [line]
                for j in range(i+1, min(i+10, len(lines))):
                    sig_lines.append(lines[j])
                    if '{' in lines[j]:
                        break
                return '\n'.join(sig_lines).strip()
            elif 'fn ' in line:
                return line.strip()

    elif language == 'javascript':
        # Look for function/async function/arrow function
        for i, line in enumerate(lines):
            if ('function ' in line or 'async ' in line or '=>' in line) and '{' not in line:
                sig_lines = [line]
                for j in range(i+1, min(i+5, len(lines))):
                    sig_lines.append(lines[j])
                    if '{' in lines[j]:
                        break
                return '\n'.join(sig_lines).strip()
            elif 'function ' in line or '=>' in line:
                return line.strip()

    # Fallback: return first non-empty line
    for line in lines:
        if line.strip():
            return line.strip()

    return code[:150]


def extract_parameters(signature: str, language: str = 'rust') -> List[str]:
    """
    Extract parameter names and types from function signature.
    """
    params = []

    if language == 'rust':
        # Extract content between first ( and )
        match = re.search(r'fn\s+\w+\s*\((.*?)\)', signature, re.DOTALL)
        if match:
            params_str = match.group(1)
            # Split by comma and clean up
            for param in params_str.split(','):
                param = param.strip()
                if param and param != 'self' and param != '&self' and param != '&mut self':
                    params.append(param)

    elif language == 'javascript':
        # Extract content between first ( and )
        match = re.search(r'\((.*?)\)', signature)
        if match:
            params_str = match.group(1)
            for param in params_str.split(','):
                param = param.strip()
                if param:
                    params.append(param)

    return params


def extract_return_type(signature: str, language: str = 'rust') -> Optional[str]:
    """
    Extract return type from function signature.
    """
    if language == 'rust':
        # Look for -> Type pattern
        match = re.search(r'->\s*([^{]+)', signature)
        if match:
            return match.group(1).strip()

    elif language == 'javascript':
        # Check for TypeScript return type annotation
        match = re.search(r':\s*([^{=>]+)', signature)
        if match:
            return match.group(1).strip()

    return None


def extract_semantic_tags(chunk: Dict) -> List[str]:
    """
    Extract semantic tags from chunk metadata (function name, type, keywords).
    """
    tags = []

    # Add function name parts (split by underscore/camelCase)
    if chunk.get('name'):
        name = chunk['name']
        # Split by underscore
        tags.extend(name.lower().split('_'))
        # Split camelCase
        tags.extend(re.findall(r'[a-z]+|[A-Z][a-z]*', name))

    # Add type
    if chunk.get('type'):
        tags.append(chunk['type'])

    # Extract keywords from docstring
    if chunk.get('docstring'):
        doc = chunk['docstring'].lower()
        keywords = ['create', 'validate', 'handle', 'process', 'send', 'receive',
                   'connect', 'register', 'authenticate', 'hash', 'token', 'websocket',
                   'jwt', 'password', 'user', 'device', 'message', 'tcp', 'mdns']
        for kw in keywords:
            if kw in doc:
                tags.append(kw)

    # Remove duplicates and empty strings
    tags = list(set([t.lower() for t in tags if t]))

    return tags


class ImprovedCodeChunker:
    """Enhanced AST-based chunking with docstrings and context"""

    def __init__(self):
        self.parsers = {
            'rust': get_parser('rust'),
            'javascript': get_parser('javascript'),
        }

    def chunk_file(self, file_path: str, content: str) -> List[Dict]:
        """Extract function/class chunks with docstrings and context"""
        ext = Path(file_path).suffix
        lang_map = {'.rs': 'rust', '.js': 'javascript'}
        lang = lang_map.get(ext)

        if not lang or lang not in self.parsers:
            return [{"code": content, "location": file_path, "type": "file",
                     "context": "", "name": Path(file_path).name}]

        parser = self.parsers[lang]
        # Convert to bytes for tree-sitter parsing
        content_bytes = bytes(content, "utf8")
        tree = parser.parse(content_bytes)
        chunks = []

        if lang == 'rust':
            chunks = self._extract_rust_chunks(tree, content_bytes, file_path)
        elif lang == 'javascript':
            chunks = self._extract_js_chunks(tree, content_bytes, file_path)

        return chunks if chunks else [{"code": content, "location": file_path,
                                       "type": "file", "context": "", "name": Path(file_path).name}]

    def _extract_rust_chunks(self, tree, content_bytes: bytes, file_path: str) -> List[Dict]:
        """Extract Rust functions with docstrings and context"""
        chunks = []

        def traverse(node, parent_context="", parent_type_name=""):
            # Handle impl blocks specially
            if node.type == 'impl_item':
                # Get the type being implemented (e.g., "Esp32Connection")
                type_node = node.child_by_field_name('type')
                if type_node:
                    type_name = content_bytes[type_node.start_byte:type_node.end_byte].decode('utf-8', errors='replace')
                else:
                    type_name = "UnknownType"

                # Don't create a chunk for the impl block itself, just traverse children
                # Pass the type name as context for nested functions
                for child in node.children:
                    traverse(child, parent_context, type_name)
                return

            # Only extract functions, NOT structs or enums
            if node.type == 'function_item':
                start_byte = node.start_byte
                end_byte = node.end_byte
                code = content_bytes[start_byte:end_byte].decode('utf-8', errors='replace')

                # Get name - decode from bytes
                name_node = node.child_by_field_name('name')
                if name_node:
                    func_name = content_bytes[name_node.start_byte:name_node.end_byte].decode('utf-8', errors='replace')
                else:
                    func_name = "anonymous"

                # If this function is inside an impl block, combine: Type::function
                if parent_type_name:
                    name = f"{parent_type_name}::{func_name}"
                else:
                    name = func_name

                # Extract docstring (preceding line comments)
                docstring = self._extract_rust_docstring(content_bytes, node.start_point[0])

                # Build context: docstring + function signature
                signature = code.split('\n')[0] if '\n' in code else code[:100]
                context = f"{docstring}\n{signature}" if docstring else signature

                # Limit chunk size
                if len(code) > 5000:
                    code = code[:5000] + "\n... (truncated)"

                chunks.append({
                    "code": code,
                    "location": f"{file_path}:{name}",
                    "type": node.type,
                    "start_line": node.start_point[0] + 1,
                    "name": name,
                    "context": context,
                    "docstring": docstring,
                    "parent": parent_context
                })

                # Update parent context for nested items (don't recurse for functions)
                # Functions don't contain other functions in Rust
            elif node.type in ['struct_item', 'enum_item']:
                # Don't create chunks for structs/enums, but traverse children
                # to find methods within impl blocks
                for child in node.children:
                    traverse(child, parent_context, parent_type_name)
            else:
                for child in node.children:
                    traverse(child, parent_context, parent_type_name)

        traverse(tree.root_node)
        return chunks

    def _extract_js_chunks(self, tree, content_bytes: bytes, file_path: str) -> List[Dict]:
        """Extract JavaScript functions with docstrings"""
        chunks = []

        def has_nested_functions(node) -> bool:
            """
            Check if node contains nested NAMED function declarations.
            Ignores anonymous callbacks like: websocket.onopen = function() {...}
            Only counts: function foo() {...}
            """
            def check_node(n):
                # Only count 'function_declaration' (named functions with 'function' keyword)
                # Ignore 'function' type (anonymous callbacks), 'arrow_function', etc.
                if n.type == 'function_declaration':
                    # Check if it has a name (to exclude anonymous)
                    name_node = n.child_by_field_name('name')
                    if name_node:
                        return True

                # Recursively check all children
                for child in n.children:
                    if check_node(child):
                        return True
                return False

            # Check children of this node (not the node itself)
            for child in node.children:
                if check_node(child):
                    return True
            return False

        def traverse(node, parent_context="", parent_node=None):
            if node.type in ['function_declaration', 'method_definition', 'arrow_function', 'function']:
                start_byte = node.start_byte
                end_byte = node.end_byte
                code = content_bytes[start_byte:end_byte].decode('utf-8', errors='replace')

                # Try to get name from the node itself
                name_node = node.child_by_field_name('name')
                if name_node:
                    name = content_bytes[name_node.start_byte:name_node.end_byte].decode('utf-8', errors='replace')
                else:
                    # Arrow functions and anonymous functions don't have a name field
                    # Check if parent is variable_declarator: const foo = () => {}
                    name = "anonymous"
                    if parent_node and parent_node.type == 'variable_declarator':
                        # Get the identifier from variable_declarator
                        identifier_node = None
                        for child in parent_node.children:
                            if child.type == 'identifier':
                                identifier_node = child
                                break
                        if identifier_node:
                            name = content_bytes[identifier_node.start_byte:identifier_node.end_byte].decode('utf-8', errors='replace')
                    # Check if parent is pair (object property): { foo: function() {} }
                    elif parent_node and parent_node.type == 'pair':
                        key_node = parent_node.child_by_field_name('key')
                        if key_node:
                            if key_node.type == 'property_identifier':
                                name = content_bytes[key_node.start_byte:key_node.end_byte].decode('utf-8', errors='replace')
                            elif key_node.type == 'string':
                                # Handle { "foo": function() {} }
                                name = content_bytes[key_node.start_byte:key_node.end_byte].decode('utf-8', errors='replace').strip('"\'')

                # Extract JSDoc
                docstring = self._extract_jsdoc(content_bytes, node.start_point[0])

                signature = code.split('\n')[0] if '\n' in code else code[:100]
                context = f"{docstring}\n{signature}" if docstring else signature

                if len(code) > 5000:
                    code = code[:5000] + "\n... (truncated)"

                # Only index this function if it has NO nested functions
                # This prevents large wrapper functions (like IIFEs) from polluting the index
                # Also skip small anonymous callbacks (< 50 lines)
                is_small_anonymous = (name == "anonymous" and len(code.split('\n')) < 50)

                if not has_nested_functions(node) and not is_small_anonymous:
                    chunks.append({
                        "code": code,
                        "location": f"{file_path}:{name}",
                        "type": node.type,
                        "start_line": node.start_point[0] + 1,
                        "name": name,
                        "context": context,
                        "docstring": docstring,
                        "parent": parent_context
                    })

            for child in node.children:
                traverse(child, parent_context, node)

        traverse(tree.root_node)
        return chunks

    def _extract_rust_docstring(self, content_bytes: bytes, start_line: int) -> str:
        """Extract Rust doc comments (///) before function"""
        content = content_bytes.decode('utf-8', errors='replace')
        lines = content.split('\n')
        docstring_lines = []

        for i in range(start_line - 1, -1, -1):
            line = lines[i].strip()
            if line.startswith('///'):
                docstring_lines.insert(0, line[3:].strip())
            elif line.startswith('//!'):
                docstring_lines.insert(0, line[3:].strip())
            elif line and not line.startswith('//'):
                break

        return ' '.join(docstring_lines)

    def _extract_jsdoc(self, content_bytes: bytes, start_line: int) -> str:
        """Extract JSDoc comments before function"""
        content = content_bytes.decode('utf-8', errors='replace')
        lines = content.split('\n')
        docstring_lines = []

        for i in range(start_line - 1, -1, -1):
            line = lines[i].strip()
            if line.startswith('/**') or line.startswith('*'):
                cleaned = line.replace('/**', '').replace('*/', '').replace('*', '').strip()
                if cleaned:
                    docstring_lines.insert(0, cleaned)
            elif not line:
                continue
            else:
                break

        return ' '.join(docstring_lines)


class CodeT5Wrapper:
    """
    Optimized wrapper for CodeT5-large (770M parameters).
    Uses encoder-only mode with proper pooling for better embeddings.
    """

    def __init__(self, model_name='Salesforce/codet5-large'):
        print(f"Loading {model_name} with native transformers (optimized for code)...")
        from transformers import T5EncoderModel, RobertaTokenizer

        # CodeT5 uses RobertaTokenizer
        self.tokenizer = RobertaTokenizer.from_pretrained(model_name, local_files_only=True)
        # Use T5EncoderModel (encoder-only) for embeddings
        self.model = T5EncoderModel.from_pretrained(model_name, local_files_only=True)
        self.device = torch.device('cuda' if torch.cuda.is_available() else 'cpu')
        self.model.to(self.device)
        self.model.eval()

        # Count parameters
        param_count = sum(p.numel() for p in self.model.parameters()) / 1e6
        print(f"CodeT5-large loaded on {self.device} ({param_count:.0f}M parameters)")

    def encode(self, texts, show_progress_bar=False, batch_size=8, normalize=True):
        """
        Encode texts to embeddings using CodeT5's encoder.
        Uses mean pooling over encoder outputs.

        Note: batch_size=8 for better quality with large model
        """
        all_embeddings = []

        # Process in batches
        for i in range(0, len(texts), batch_size):
            batch_texts = texts[i:i+batch_size]

            # CodeT5 tokenization (supports up to 512 tokens)
            inputs = self.tokenizer(
                batch_texts,
                padding=True,
                truncation=True,
                max_length=256,  # CodeT5 max length
                return_tensors='pt',
                add_special_tokens=True
            )
            inputs = {k: v.to(self.device) for k, v in inputs.items()}

            # Get embeddings
            with torch.no_grad():
                outputs = self.model(**inputs)
                # Use mean pooling over encoder hidden states
                hidden_states = outputs.last_hidden_state  # (batch, seq_len, hidden_dim)
                attention_mask = inputs['attention_mask'].unsqueeze(-1)  # (batch, seq_len, 1)

                # Mean pooling (ignoring padding tokens)
                sum_embeddings = torch.sum(hidden_states * attention_mask, dim=1)
                sum_mask = torch.clamp(attention_mask.sum(dim=1), min=1e-9)
                embeddings = sum_embeddings / sum_mask

                # L2 normalization for better cosine similarity
                if normalize:
                    embeddings = torch.nn.functional.normalize(embeddings, p=2, dim=1)

                embeddings = embeddings.cpu().numpy()
                all_embeddings.append(embeddings)

        return np.vstack(all_embeddings)


class UniXcoderWrapper:
    """
    Optimized wrapper for UniXcoder to match SentenceTransformer interface.
    Uses [CLS] token + L2 normalization for better retrieval.
    """

    def __init__(self, model_name='microsoft/unixcoder-base'):
        print(f"Loading {model_name} with native transformers (optimized for code)...")
        self.tokenizer = AutoTokenizer.from_pretrained(model_name, local_files_only=True)
        self.model = AutoModel.from_pretrained(model_name, local_files_only=True)
        self.device = torch.device('cuda' if torch.cuda.is_available() else 'cpu')
        self.model.to(self.device)
        self.model.eval()
        print(f"UniXcoder loaded on {self.device}")

    def encode(self, texts, show_progress_bar=False, batch_size=8, normalize=True):
        """
        Encode texts to embeddings using UniXcoder's [CLS] token.
        Optionally normalizes embeddings for better cosine similarity.

        Note: batch_size=8 (reduced from 32) for better quality:
        - Less padding per batch = more accurate embeddings
        - With max_length=512, GPU utilization remains high
        """
        all_embeddings = []

        # Process in batches
        for i in range(0, len(texts), batch_size):
            batch_texts = texts[i:i+batch_size]

            # UniXcoder-optimized tokenization
            # UniXcoder supports up to 1024 tokens (longer than CodeBERT's 512)
            inputs = self.tokenizer(
                batch_texts,
                padding=True,
                truncation=True,
                max_length=512,  # Increased from 256 for better quality: captures more code context
                return_tensors='pt',
                add_special_tokens=True
            )
            inputs = {k: v.to(self.device) for k, v in inputs.items()}

            # Get embeddings
            with torch.no_grad():
                outputs = self.model(**inputs)
                # Use [CLS] token (first token) - trained for this purpose
                embeddings = outputs.last_hidden_state[:, 0, :]

                # L2 normalization for better cosine similarity
                if normalize:
                    embeddings = torch.nn.functional.normalize(embeddings, p=2, dim=1)

                embeddings = embeddings.cpu().numpy()
                all_embeddings.append(embeddings)

        return np.vstack(all_embeddings)


class ImprovedRAGSystem:
    """Enhanced RAG with hybrid search and re-ranking"""

    def __init__(self, db_path: str = "./chroma_db", use_docstring_only: bool = False, reset_database: bool = False):
        print("Initializing Improved RAG System...")

        self.use_docstring_only = use_docstring_only
        self.chunker = ImprovedCodeChunker()

        # Embedder for semantic search
        # Uncomment ONE model to test:

        self.embedder = SentenceTransformer('microsoft/codebert-base')          # CodeBERT (Baseline) - 125M params, ~500MB VRAM
        #self.embedder = UniXcoderWrapper('microsoft/unixcoder-base')           # UniXcoder (Native - Better for cross-language) - 125M params, ~500MB VRAM
        #self.embedder = SentenceTransformer('microsoft/graphcodebert-base')    # GraphCodeBERT (Graph-based) - 125M params, ~500MB VRAM
        #self.embedder = CodeT5Wrapper('Salesforce/codet5-large')               # CodeT5-large (LARGEST) - 770M params, ~3GB VRAM, best quality

        # Cross-encoder for re-ranking
        # Options:
        # - 'cross-encoder/ms-marco-MiniLM-L-6-v2'  # 22M params, ~100MB VRAM (fast)
        # - 'cross-encoder/ms-marco-MiniLM-L-12-v2' # 110M params, ~400MB VRAM (better)
        # - 'BAAI/bge-reranker-large'               # 560M params, ~2GB VRAM (best quality)
        print("Loading cross-encoder for re-ranking...")
        self.reranker = CrossEncoder('cross-encoder/ms-marco-MiniLM-L-12-v2')  # Medium-sized reranker

        # ChromaDB - separate collections for full code and docstring-only
        self.client = chromadb.PersistentClient(path=db_path)

        # Determine collection name based on mode
        collection_name = "code_chunks_docstring_only" if use_docstring_only else "code_chunks"

        # Delete collection if reset_database is True
        if reset_database:
            try:
                self.client.delete_collection(collection_name)
                print(f"🗑️  Deleted existing {collection_name} collection (reset_database=True)")
            except:
                pass  # Collection doesn't exist, that's fine

        try:
            self.collection = self.client.get_collection(collection_name)
            print(f"Loaded existing {collection_name} collection ({self.collection.count()} chunks)")

            # BM25 index (needs to be rebuilt from existing collection)
            self.bm25 = None
            self.all_chunks = []

            # Rebuild BM25 index from existing ChromaDB data
            if self.collection.count() > 0:
                print("Rebuilding BM25 index from existing data...")
                self._rebuild_bm25_index()
        except:
            self.collection = self.client.create_collection(
                name=collection_name,
                metadata={"description": f"Code chunks {'(docstring only)' if use_docstring_only else 'with improved chunking'}"}
            )
            print(f"Created new {collection_name} collection")

            # BM25 index (will be created during indexing)
            self.bm25 = None
            self.all_chunks = []

        print(f"✓ Improved RAG System initialized ({'Docstring-Only Mode' if use_docstring_only else 'Full Code Mode'})\n")

    def _create_file_summary_chunks(self, all_chunks: List[Dict]) -> List[Dict]:
        """
        OPTION B: Create file-level summary chunks that aggregate all functions in a file.

        Args:
            all_chunks: List of function-level chunks

        Returns:
            List of file-level summary chunks
        """
        from collections import defaultdict

        # Group chunks by file
        file_contents = defaultdict(list)

        for chunk in all_chunks:
            file_path = self._extract_file_path(chunk['location'])
            file_contents[file_path].append({
                'name': chunk.get('name', ''),
                'docstring': chunk.get('docstring', ''),
                'signature': extract_function_signature(chunk.get('code', '')),
                'type': chunk.get('type', '')
            })

        # Create summary chunks
        file_summary_chunks = []

        for file_path, functions in file_contents.items():
            # Build comprehensive file summary
            summary_lines = [f"File: {file_path}"]
            summary_lines.append(f"Total Functions: {len(functions)}")
            summary_lines.append("\nFunctions in this file:")

            # List all functions with signatures and docstrings
            for i, func in enumerate(functions[:30], 1):  # Limit to 30 functions to avoid token overflow
                summary_lines.append(f"\n{i}. {func['name']}")
                if func['signature']:
                    summary_lines.append(f"   Signature: {func['signature'][:150]}")
                if func['docstring']:
                    summary_lines.append(f"   Doc: {func['docstring'][:200]}")

            summary_text = '\n'.join(summary_lines)

            # Create file summary chunk
            file_summary_chunks.append({
                'code': summary_text,
                'location': file_path,  # No :function suffix for file-level chunks
                'type': 'file_summary',
                'name': file_path.split('/')[-1].split('\\')[-1],  # Just filename
                'context': f"File summary with {len(functions)} functions",
                'docstring': f"Summary of {file_path}",
                'start_line': 1
            })

        return file_summary_chunks

    def index_codebase(self, codebase_path: str):
        """Index codebase with enriched context"""
        print(f"Indexing codebase: {codebase_path}")

        codebase_path = Path(codebase_path)
        extensions = ['.rs', '.js']

        # Exclude directories with generated/external code
        exclude_dirs = ['target', 'node_modules', 'build', 'dist', '.git']

        all_chunks = []

        for ext in extensions:
            all_files = list(codebase_path.rglob(f'*{ext}'))

            # Filter out files from excluded directories
            files = [f for f in all_files if not any(excluded in f.parts for excluded in exclude_dirs)]

            print(f"Found {len(files)} {ext} files (excluded {len(all_files) - len(files)} from {exclude_dirs})")

            for file_path in files:
                try:
                    content = file_path.read_text(encoding='utf-8')
                    chunks = self.chunker.chunk_file(str(file_path), content)
                    all_chunks.extend(chunks)
                except Exception as e:
                    print(f"Warning: Could not parse {file_path}: {e}")

        if not all_chunks:
            print("No code chunks found!")
            return

        print(f"\nTotal chunks extracted: {len(all_chunks)}")

        # OPTION B: Create file-level summary chunks (if enabled)
        if USE_FILE_SUMMARY_CHUNKS:
            print("Creating file-level summary chunks...")
            file_summary_chunks = self._create_file_summary_chunks(all_chunks)
            print(f"Created {len(file_summary_chunks)} file summary chunks")
            # Add file summaries to the chunks list
            all_chunks.extend(file_summary_chunks)
            print(f"Total chunks (including file summaries): {len(all_chunks)}")

        print("Generating embeddings with context enrichment...")

        # Embed based on mode
        enriched_texts = []
        documents_to_store = []

        for chunk in all_chunks:
            if self.use_docstring_only:
                # Docstring-only mode: embed and store only docstring + signature
                signature = extract_function_signature(chunk['code'])
                docstring = chunk.get('docstring', '')

                # For embedding: combine name, docstring, and signature
                enriched = f"{chunk['name']}\n{docstring}\n{signature}"
                enriched_texts.append(enriched)

                # For storage: store docstring + signature (not full code)
                doc_text = f"{chunk['name']}\n{docstring}\n{signature}"
                documents_to_store.append(doc_text)
            else:
                # Full code mode: embed with enriched context (original behavior)
                # Combine multiple signals for better embeddings
                # Use full code (CodeBERT will truncate at 512 tokens automatically)
                enriched = f"{chunk['name']}\n{chunk.get('docstring', '')}\n{chunk['code'][:256]}"
                enriched_texts.append(enriched)

                # Store full code
                documents_to_store.append(chunk['code'])

        embeddings = self.embedder.encode(enriched_texts, show_progress_bar=True)

        # Store in ChromaDB with shortened paths
        print("Storing in vector database...")
        self.collection.add(
            embeddings=embeddings.tolist(),
            documents=documents_to_store,
            metadatas=[{
                "location": shorten_path(chunk['location']),  # Store relative path
                "type": chunk.get('type', 'unknown'),
                "start_line": str(chunk.get('start_line', 0)),
                "name": chunk.get('name', ''),
                "context": chunk.get('context', ''),
                "docstring": chunk.get('docstring', ''),
                "full_code": chunk['code'] if self.use_docstring_only else ''  # Store full code in metadata for docstring mode
            } for chunk in all_chunks],
            ids=[f"chunk_{i}" for i in range(len(all_chunks))]
        )

        # Build BM25 index for keyword search
        print("Building BM25 keyword index...")
        # Build BM25 based on mode
        if self.use_docstring_only:
            # Docstring-only: index name + docstring + signature
            tokenized_corpus = [
                self._tokenize(f"{chunk['name']} {chunk.get('docstring', '')} {extract_function_signature(chunk['code'])}")
                for chunk in all_chunks
            ]
        else:
            # Full code: index name + docstring + code
            tokenized_corpus = [
                self._tokenize(f"{chunk['name']} {chunk.get('docstring', '')} {chunk['code']}")
                for chunk in all_chunks
            ]

        self.bm25 = BM25Okapi(tokenized_corpus)
        self.all_chunks = all_chunks

        mode_label = "docstring-only" if self.use_docstring_only else "full code"
        print(f"✓ Indexed {len(all_chunks)} code chunks with hybrid search ({mode_label})\n")

    def _rebuild_bm25_index(self):
        """Rebuild BM25 index from existing ChromaDB collection"""
        # Get all documents from ChromaDB
        all_data = self.collection.get(include=['documents', 'metadatas'])

        # Reconstruct all_chunks from ChromaDB data
        self.all_chunks = []
        for i, (doc, meta) in enumerate(zip(all_data['documents'], all_data['metadatas'])):
            # In docstring mode, doc contains only docstring+signature, full code is in metadata
            # In full code mode, doc contains the full code
            if self.use_docstring_only:
                code = meta.get('full_code', doc)  # Get full code from metadata
            else:
                code = doc

            self.all_chunks.append({
                'code': code,
                'location': meta.get('location', ''),
                'name': meta.get('name', ''),
                'context': meta.get('context', ''),
                'docstring': meta.get('docstring', ''),
                'type': meta.get('type', ''),
                'start_line': int(meta.get('start_line', 0))
            })

        # Build BM25 index based on mode
        if self.use_docstring_only:
            # Index only docstring + name + signature (from documents, not full code)
            tokenized_corpus = [
                self._tokenize(all_data['documents'][i])
                for i in range(len(all_data['documents']))
            ]
        else:
            # Index full code + name + docstring
            tokenized_corpus = [
                self._tokenize(f"{chunk['name']} {chunk.get('docstring', '')} {chunk['code']}")
                for chunk in self.all_chunks
            ]

        self.bm25 = BM25Okapi(tokenized_corpus)

        mode_label = "docstring-only" if self.use_docstring_only else "full code"
        print(f"✓ Rebuilt BM25 index with {len(self.all_chunks)} chunks ({mode_label})\n")

    def _tokenize(self, text: str) -> List[str]:
        """Simple tokenization for BM25"""
        return re.findall(r'\w+', text.lower())

    def _extract_file_path(self, location: str) -> str:
        """
        Extract file path from location string (removes :function_name).
        Handles Windows paths correctly (C:\path\file.rs:function).

        Returns: file_path without function name
        """
        if ':' in location:
            # Split from right to handle Windows drive letters (C:\...)
            parts = location.rsplit(':', 1)
            # Check if the part after ':' looks like a function name (no path separators)
            if len(parts) == 2 and ('\\' not in parts[1] and '/' not in parts[1]):
                return parts[0]
        return location

    def aggregate_file_scores(self, candidates: List[Dict]) -> Dict[str, float]:
        """
        OPTION A: Aggregate function scores by file to rank files instead of individual functions.

        Args:
            candidates: List of candidate chunks with 'rerank_score' field

        Returns:
            Dictionary mapping {file_path: aggregated_score}
        """
        from collections import defaultdict

        file_scores = defaultdict(list)

        # Group candidates by file
        for candidate in candidates:
            file_path = self._extract_file_path(candidate['location'])
            file_scores[file_path].append(candidate.get('rerank_score', 0))

        # Aggregate scores based on strategy
        aggregated = {}
        for file_path, scores in file_scores.items():
            if FILE_AGGREGATION_STRATEGY == "max":
                # Best function represents the file
                aggregated[file_path] = max(scores)

            elif FILE_AGGREGATION_STRATEGY == "mean":
                # Average of all function scores
                aggregated[file_path] = np.mean(scores)

            elif FILE_AGGREGATION_STRATEGY == "weighted":
                # Balanced: mean + max boost
                aggregated[file_path] = np.mean(scores) + 0.5 * max(scores)

            elif FILE_AGGREGATION_STRATEGY == "count":
                # Files with more relevant functions rank higher
                # Sum weighted by sqrt(count) to avoid over-weighting large files
                aggregated[file_path] = sum(scores) / (len(scores) ** 0.5)

            else:
                # Default to max
                aggregated[file_path] = max(scores)

        return aggregated

    def retrieve(self, query: str, top_k: int = 5, hybrid: bool = True) -> List[Dict]:
        """
        Hybrid retrieval with re-ranking
        1. Vector search (semantic)
        2. BM25 search (keyword)
        3. Combine and re-rank
        """
        if not hybrid or self.bm25 is None:
            return self._vector_search_only(query, top_k)

        # 1. Vector search - configurable candidate pool size
        if USE_QUERY_EXPANSION:
            # Query expansion: Create multiple query variations and average embeddings
            query_variations = [
                query,
                f"implement {query}",
                f"function that {query}",
            ]
            query_embeddings = self.embedder.encode(query_variations)
            query_embedding = np.mean(query_embeddings, axis=0)
        else:
            # Original query only (old c378578 behavior)
            query_embedding = self.embedder.encode([query])[0]

        vector_results = self.collection.query(
            query_embeddings=[query_embedding.tolist()],
            n_results=min(CANDIDATE_POOL_SIZE, self.collection.count())
        )

        # 2. BM25 search - configurable candidate pool size
        tokenized_query = self._tokenize(query)
        bm25_scores = self.bm25.get_scores(tokenized_query)
        top_bm25_indices = np.argsort(bm25_scores)[-CANDIDATE_POOL_SIZE:][::-1]

        # 3. Combine candidates (union of both)
        vector_ids = set(vector_results['ids'][0])
        bm25_chunks = [self.all_chunks[i] for i in top_bm25_indices]

        # Get all candidate chunks
        candidates = []
        for i, chunk_id in enumerate(vector_results['ids'][0]):
            candidates.append({
                "code": vector_results['documents'][0][i],
                "location": vector_results['metadatas'][0][i]['location'],
                "name": vector_results['metadatas'][0][i]['name'],
                "context": vector_results['metadatas'][0][i]['context'],
                "vector_score": 1 - vector_results['distances'][0][i]
            })

        for idx in top_bm25_indices:
            chunk = self.all_chunks[idx]
            # Add if not already in candidates
            if f"chunk_{idx}" not in vector_ids:
                candidates.append({
                    "code": chunk['code'],
                    "location": chunk['location'],
                    "name": chunk.get('name', ''),
                    "context": chunk.get('context', ''),
                    "bm25_score": bm25_scores[idx]
                })

        # 4. Re-rank with cross-encoder
        print(f"Re-ranking {len(candidates)} candidates...")
        pairs = []
        for c in candidates:
            if USE_SIGNATURE_ONLY:
                # Old c378578 behavior: Use only function signature (first line)
                code_preview = c['code'].split('\n')[0] if '\n' in c['code'] else c['code'][:200]
                text = f"Function: {c['name']}\n{c.get('context', '')}\n{code_preview}"
            else:
                # New behavior: Use first 20 lines of code
                code_lines = c['code'].split('\n')
                code_preview = '\n'.join(code_lines[:20]) if len(code_lines) > 20 else c['code'][:1500]
                docstring = c.get('docstring', '') if 'docstring' in c else ''
                text = f"Function: {c['name']}\n{docstring}\n{c.get('context', '')}\n{code_preview}"
            pairs.append([query, text])

        rerank_scores = self.reranker.predict(pairs)

        # 5. Apply function name boosting (configurable)
        if USE_FUNCTION_NAME_BOOSTING:
            # Old c378578 behavior: Apply function name matching boost
            query_lower = query.lower()
            query_tokens = set(re.findall(r'\w+', query_lower))

            # Extract action words from query
            action_words = {'create', 'validate', 'hash', 'load', 'send', 'check', 'handle',
                           'register', 'connect', 'start', 'stop', 'get', 'set', 'new',
                           'init', 'update', 'delete', 'find', 'search', 'discover'}

            # Extract important keywords (nouns)
            important_keywords = {'jwt', 'token', 'password', 'websocket', 'template',
                                'device', 'esp32', 'tcp', 'mdns', 'auth', 'user', 'message',
                                'connection', 'client', 'server', 'command', 'discovery'}

            for i, candidate in enumerate(candidates):
                base_score = sanitize_score(rerank_scores[i])
                candidate['rerank_score'] = base_score

                func_name_lower = candidate['name'].lower()
                func_tokens = set(re.findall(r'\w+', func_name_lower))

                # Skip "anonymous" functions - they get no boost
                if 'anonymous' in func_name_lower:
                    candidate['rerank_score'] = base_score - 1.0  # Penalize anonymous
                    continue

                boost = 0.0

                # Strong boost: Action word + keyword match in function name
                action_matches = action_words & query_tokens & func_tokens
                keyword_matches = important_keywords & query_tokens & func_tokens

                if action_matches and keyword_matches:
                    # Perfect match: action + keyword
                    boost += 3.0
                elif action_matches:
                    # Action word match
                    boost += 2.0
                elif keyword_matches:
                    # Keyword match
                    boost += 1.5

                # Medium boost: Any query word in function name
                common_tokens = query_tokens & func_tokens
                if common_tokens:
                    boost += len(common_tokens) * 0.5

                # Exact substring match (e.g., "loadTemplate" contains "load" and "template")
                for query_word in query_tokens:
                    if len(query_word) > 3 and query_word in func_name_lower:
                        boost += 1.0

                candidate['rerank_score'] = base_score + boost
        else:
            # New behavior: No boosting, pure cross-encoder scores
            for i, candidate in enumerate(candidates):
                candidate['rerank_score'] = sanitize_score(rerank_scores[i])

        # 6. Apply file-level score aggregation (if enabled)
        if USE_FILE_SCORE_AGGREGATION:
            # Aggregate scores by file
            file_scores = self.aggregate_file_scores(candidates)

            # Add file scores and compute combined scores
            for candidate in candidates:
                file_path = self._extract_file_path(candidate['location'])
                candidate['file_score'] = file_scores.get(file_path, 0)

                # Combine file score and function score
                # Higher FILE_VS_FUNCTION_WEIGHT = prioritize files
                # Lower FILE_VS_FUNCTION_WEIGHT = prioritize functions
                candidate['combined_score'] = (
                    FILE_VS_FUNCTION_WEIGHT * candidate['file_score'] +
                    (1 - FILE_VS_FUNCTION_WEIGHT) * candidate['rerank_score']
                )

            # Sort by combined score (file-aware ranking)
            candidates.sort(key=lambda x: x.get('combined_score', x['rerank_score']), reverse=True)
        else:
            # Original behavior: Sort by function-level re-rank scores only
            candidates.sort(key=lambda x: x['rerank_score'], reverse=True)

        return candidates[:top_k]

    def retrieve_files(self, query: str, top_k: int = 3) -> List[Tuple[str, float]]:
        """
        OPTION C: Two-stage file-first retrieval - retrieve top files only.

        Stage 1: Find the most relevant FILES (not individual functions).
        Use file summary chunks if available, otherwise aggregate function scores.

        Args:
            query: User query
            top_k: Number of top files to return

        Returns:
            List of (file_path, score) tuples, sorted by relevance
        """
        if not USE_TWO_STAGE_FILE_RETRIEVAL:
            # Fallback: Use regular retrieval and extract unique files
            candidates = self.retrieve(query, top_k=top_k * 5, hybrid=True)
            file_scores = self.aggregate_file_scores(candidates)
            # Sort files by score
            sorted_files = sorted(file_scores.items(), key=lambda x: x[1], reverse=True)
            return sorted_files[:top_k]

        # Two-stage retrieval requires file summary chunks
        if not USE_FILE_SUMMARY_CHUNKS:
            print("Warning: USE_TWO_STAGE_FILE_RETRIEVAL requires USE_FILE_SUMMARY_CHUNKS=True")
            print("Falling back to aggregation-based file retrieval...")
            candidates = self.retrieve(query, top_k=top_k * 5, hybrid=True)
            file_scores = self.aggregate_file_scores(candidates)
            sorted_files = sorted(file_scores.items(), key=lambda x: x[1], reverse=True)
            return sorted_files[:top_k]

        # Retrieve file summary chunks only
        # Filter for file_summary type chunks
        print(f"  Stage 1: Finding top {top_k} files...")

        # Use hybrid search on all chunks (including file summaries)
        all_candidates = self.retrieve(query, top_k=50, hybrid=True)

        # Separate file summaries from function chunks
        file_summary_candidates = [c for c in all_candidates if c.get('location', '').find(':') == -1]

        # If no file summaries found (shouldn't happen), fall back
        if not file_summary_candidates:
            print("  Warning: No file summary chunks found, using function aggregation...")
            file_scores = self.aggregate_file_scores(all_candidates)
            sorted_files = sorted(file_scores.items(), key=lambda x: x[1], reverse=True)
            return sorted_files[:top_k]

        # Sort file summaries by their scores
        file_summary_candidates.sort(key=lambda x: x.get('rerank_score', 0), reverse=True)

        # Return top files with their scores
        top_files = [
            (candidate['location'], candidate.get('rerank_score', 0))
            for candidate in file_summary_candidates[:top_k]
        ]

        return top_files

    def retrieve_two_stage(self, query: str, top_k: int = 5) -> List[Dict]:
        """
        OPTION C: Complete two-stage retrieval implementation.

        Stage 1: Find best files using retrieve_files()
        Stage 2: Retrieve all functions from those files and re-rank

        Args:
            query: User query
            top_k: Number of final results to return

        Returns:
            List of top-k function chunks from the most relevant files
        """
        # Stage 1: Get top files
        top_files = self.retrieve_files(query, top_k=FILE_RETRIEVAL_TOP_K)

        if not top_files:
            print("  No relevant files found")
            return []

        print(f"  Stage 2: Retrieving functions from {len(top_files)} top files...")

        # Stage 2: Get all functions from top files
        all_functions = []
        for file_path, file_score in top_files:
            functions = self.get_all_functions_from_file(file_path)
            # Add file score to each function
            for func in functions:
                func['file_score'] = file_score
            all_functions.extend(functions)

        if not all_functions:
            print("  No functions found in top files")
            return []

        print(f"  Found {len(all_functions)} functions in top files")

        # Stage 3: Re-rank all functions from top files
        print(f"  Re-ranking {len(all_functions)} functions...")

        pairs = []
        for func in all_functions:
            if USE_SIGNATURE_ONLY:
                code_preview = func['code'].split('\n')[0] if '\n' in func['code'] else func['code'][:200]
                text = f"Function: {func['name']}\n{func.get('context', '')}\n{code_preview}"
            else:
                code_lines = func['code'].split('\n')
                code_preview = '\n'.join(code_lines[:20]) if len(code_lines) > 20 else func['code'][:1500]
                text = f"Function: {func['name']}\n{func.get('context', '')}\n{code_preview}"
            pairs.append([query, text])

        # Re-rank with cross-encoder
        rerank_scores = self.reranker.predict(pairs)

        # Add scores to functions
        for i, func in enumerate(all_functions):
            func['rerank_score'] = sanitize_score(rerank_scores[i])
            # Combined score: file score + function score
            func['combined_score'] = (
                0.5 * func.get('file_score', 0) +
                0.5 * func['rerank_score']
            )

        # Sort by combined score
        all_functions.sort(key=lambda x: x['combined_score'], reverse=True)

        return all_functions[:top_k]

    def get_all_functions_from_file(self, file_path: str) -> List[Dict]:
        """
        Retrieve ALL functions from a specific file.

        Args:
            file_path: The file path to search for (e.g., "codebase/src/backend/auth.rs")

        Returns:
            List of all function chunks from that file
        """
        all_functions = []

        # Normalize the file path for comparison
        normalized_target = file_path.replace('\\', '/').lower()

        for chunk in self.all_chunks:
            chunk_location = chunk['location']

            # Extract file path from location (handle Windows paths correctly)
            if ':' in chunk_location:
                # Split from right to handle Windows drive letters
                parts = chunk_location.rsplit(':', 1)
                # Check if the part after ':' looks like a function name (no path separators)
                if len(parts) == 2 and ('\\' not in parts[1] and '/' not in parts[1]):
                    chunk_file = parts[0]
                else:
                    chunk_file = chunk_location
            else:
                chunk_file = chunk_location

            # Normalize for comparison
            normalized_chunk_file = chunk_file.replace('\\', '/').lower()

            # Check if this chunk is from the target file (exact match or ends with target)
            # Use endswith to avoid matching "c" to every file with "c" in the path
            if (normalized_target == normalized_chunk_file or
                normalized_chunk_file.endswith(normalized_target) or
                normalized_target.endswith(normalized_chunk_file)):
                all_functions.append({
                    "code": chunk['code'],
                    "location": chunk['location'],
                    "name": chunk.get('name', ''),
                    "context": chunk.get('context', ''),
                    "type": chunk.get('type', ''),
                    "rerank_score": 0.0  # Default score for display consistency
                })

        return all_functions

    def _vector_search_only(self, query: str, top_k: int) -> List[Dict]:
        """Fallback: vector search only"""
        query_embedding = self.embedder.encode([query])[0]
        results = self.collection.query(
            query_embeddings=[query_embedding.tolist()],
            n_results=top_k
        )

        chunks = []
        for i in range(len(results['ids'][0])):
            chunks.append({
                "code": results['documents'][0][i],
                "location": results['metadatas'][0][i]['location'],
                "name": results['metadatas'][0][i]['name'],
                "context": results['metadatas'][0][i]['context'],
                "similarity": 1 - results['distances'][0][i]
            })

        return chunks


if __name__ == "__main__":
    print("Improved RAG System ready!")
    print("\nUsage:")
    print("  rag = ImprovedRAGSystem()")
    print("  rag.index_codebase('path/to/codebase')")
    print("  results = rag.retrieve('your query', top_k=5, hybrid=True)")
