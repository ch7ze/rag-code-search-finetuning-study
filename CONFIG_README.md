# RAG System Configuration Guide

## Configuration: `INDEX_STRUCTS_AND_ENUMS`

### Location
The configuration variable is defined at the top of `rag_system.py`:

```python
# ============================================================================
# CONFIGURATION
# ============================================================================

# Set to False to index ONLY functions (exclude structs, enums, classes)
# Set to True to index functions AND data structures (structs, enums)
INDEX_STRUCTS_AND_ENUMS = True
```

### What It Does

**When `INDEX_STRUCTS_AND_ENUMS = True` (default):**
- Indexes functions **AND** structs/enums
- For Rust: Indexes `function_item`, `struct_item`, `enum_item`
- For JavaScript: Indexes all function types (no change, JS doesn't have separate struct/enum in AST)

**When `INDEX_STRUCTS_AND_ENUMS = False`:**
- Indexes **ONLY** functions
- For Rust: Indexes only `function_item`
- For JavaScript: Indexes all function types (no change)

### How to Use

#### Method 1: Change the Global Configuration (Recommended)

Edit `rag_system.py` line 29:

```python
# To index only functions:
INDEX_STRUCTS_AND_ENUMS = False

# To include structs and enums:
INDEX_STRUCTS_AND_ENUMS = True
```

Then re-index your codebase:

```bash
python reindex.py
```

#### Method 2: Override at Runtime

You can override the configuration when creating the RAG system:

```python
from rag_system import ImprovedRAGSystem

# Functions only
rag = ImprovedRAGSystem(index_structs_and_enums=False)

# Functions + structs/enums
rag = ImprovedRAGSystem(index_structs_and_enums=True)
```

### Testing the Configuration

Run the test script to see the difference:

```bash
python test_config.py
```

This will show you:
- How many chunks are created with each configuration
- What types of code elements are indexed
- Which structs/enums would be filtered out

### Expected Results

For a typical Rust codebase like the ESP32 project:

| Configuration | Chunks | Contains |
|--------------|--------|----------|
| `True` | ~150-200 | Functions + Structs + Enums |
| `False` | ~100-120 | Functions only |

Example filtered items when set to `False`:
- `ESP32Device` (struct)
- `User` (struct)
- `Claims` (struct)
- `DeviceStatus` (enum)
- `DeviceConnectionType` (enum)
- `Esp32Command` (enum)
- etc.

### When to Use Each Setting

**Use `INDEX_STRUCTS_AND_ENUMS = True` when:**
- You want comprehensive code search covering all public APIs
- Developers need to find data structures, not just implementations
- Use cases include: "What fields does User have?", "What are the device states?"

**Use `INDEX_STRUCTS_AND_ENUMS = False` when:**
- You want to focus purely on function-level search
- Your test questions only ask about function implementations
- You want to reduce index size and improve function-only retrieval

### Impact on Evaluation

If your test questions (`test_questions_v2.json`) only contain function queries, setting `INDEX_STRUCTS_AND_ENUMS = False` may improve:
- Precision (less noise from struct/enum results)
- Speed (smaller index)
- Evaluation scores (if questions don't target structs)

However, it makes the system less useful for real-world code search scenarios.

### Recommendation

For **research/evaluation**: Set to `False` if your benchmark only tests function search.

For **production/real-world use**: Set to `True` to provide comprehensive code search.
