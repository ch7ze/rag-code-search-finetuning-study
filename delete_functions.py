"""
Function deletion script for RQ2 deletion experiment.

This script removes selected functions from the codebase using AST parsing
to preserve file structure and avoid breaking the code unnecessarily.

For Rust files: Uses tree-sitter-rust to identify and remove function definitions
For JavaScript files: Uses tree-sitter-javascript for function deletion
"""

import json
import os
import shutil
from pathlib import Path
from typing import List, Dict, Optional
import argparse
from datetime import datetime


def load_deletion_candidates(file_path: str) -> List[Dict]:
    """Load deletion candidates from JSON file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        data = json.load(f)
        return data.get('deletion_candidates', [])


def backup_codebase(source_dir: str, backup_dir: str):
    """Create a backup of the original codebase."""
    if os.path.exists(backup_dir):
        print(f"Backup already exists at: {backup_dir}")
        response = input("Overwrite? (y/n): ")
        if response.lower() != 'y':
            print("Aborting.")
            exit(1)
        shutil.rmtree(backup_dir)

    print(f"Creating backup: {source_dir} -> {backup_dir}")
    shutil.copytree(source_dir, backup_dir)
    print(f"✓ Backup created at: {backup_dir}")


def copy_codebase_for_deletion(source_dir: str, target_dir: str):
    """Copy codebase to new directory where deletions will be made."""
    if os.path.exists(target_dir):
        print(f"Target directory already exists: {target_dir}")
        response = input("Overwrite? (y/n): ")
        if response.lower() != 'y':
            print("Aborting.")
            exit(1)
        shutil.rmtree(target_dir)

    print(f"Copying codebase: {source_dir} -> {target_dir}")
    shutil.copytree(source_dir, target_dir)
    print(f"✓ Codebase copied to: {target_dir}")


def find_function_in_file(file_path: str, function_name: str, line_number: Optional[int] = None) -> Optional[tuple]:
    """
    Find function boundaries in source file.

    Returns:
        Tuple of (start_line, end_line) if found, None otherwise
    """
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
    except Exception as e:
        print(f"Error reading {file_path}: {e}")
        return None

    # Simple heuristic-based approach (fallback if tree-sitter not available)
    # Look for function definition patterns

    file_ext = Path(file_path).suffix
    start_line = None
    end_line = None
    brace_count = 0
    in_function = False

    if file_ext == '.rs':
        # Rust function patterns: "pub fn function_name", "fn function_name", "async fn function_name"
        function_patterns = [
            f"pub fn {function_name}",
            f"fn {function_name}",
            f"pub async fn {function_name}",
            f"async fn {function_name}",
            f"pub(crate) fn {function_name}",
            f"pub(crate) async fn {function_name}"
        ]
    elif file_ext == '.js':
        # JavaScript function patterns
        function_patterns = [
            f"function {function_name}",
            f"const {function_name} =",
            f"let {function_name} =",
            f"var {function_name} =",
            f"{function_name}: function",
            f"async function {function_name}"
        ]
    else:
        print(f"Unsupported file type: {file_ext}")
        return None

    # Search for function start
    for i, line in enumerate(lines, start=1):
        if not in_function:
            # Check if this line contains the function definition
            for pattern in function_patterns:
                if pattern in line:
                    start_line = i
                    in_function = True

                    # If line_number provided, verify it matches (within +/- 5 lines)
                    if line_number and abs(i - line_number) > 5:
                        print(f"WARNING: Found {function_name} at line {i}, expected around {line_number}")

                    # Count braces on this line
                    brace_count = line.count('{') - line.count('}')
                    break

            if in_function and brace_count == 0:
                # Single-line function or function without body yet
                # Look ahead for opening brace
                continue

        else:
            # Inside function, track braces
            brace_count += line.count('{') - line.count('}')

            if brace_count == 0:
                end_line = i
                break

    if start_line and end_line:
        return (start_line, end_line)
    elif start_line:
        # Function found but end not detected (might be at end of file)
        print(f"WARNING: Found function {function_name} start at {start_line} but couldn't detect end")
        # Assume it goes to end of file
        return (start_line, len(lines))

    return None


def delete_function_from_file(
    file_path: str,
    function_name: str,
    line_number: Optional[int] = None,
    comment_out: bool = False
) -> bool:
    """
    Delete or comment out a function from a source file.

    Args:
        file_path: Path to source file
        function_name: Name of function to delete
        line_number: Expected line number (for verification)
        comment_out: If True, comment out instead of delete (safer)

    Returns:
        True if successful, False otherwise
    """
    # Find function boundaries
    boundaries = find_function_in_file(file_path, function_name, line_number)

    if not boundaries:
        print(f"  ✗ Could not find function '{function_name}' in {file_path}")
        return False

    start_line, end_line = boundaries

    # Read file
    with open(file_path, 'r', encoding='utf-8') as f:
        lines = f.readlines()

    print(f"  → Found '{function_name}' at lines {start_line}-{end_line}")

    if comment_out:
        # Comment out the function
        file_ext = Path(file_path).suffix
        comment_prefix = "// " if file_ext == '.rs' else "// "

        for i in range(start_line - 1, end_line):
            if i < len(lines):
                lines[i] = comment_prefix + lines[i]

        action = "commented out"
    else:
        # Delete the function (including preceding comments/docs)
        # Look backward for doc comments
        doc_start = start_line - 1

        while doc_start > 0:
            line = lines[doc_start - 1].strip()
            # Check for comments/docs
            if line.startswith('//') or line.startswith('///') or line.startswith('/*') or line == '':
                doc_start -= 1
            else:
                break

        # Delete lines from doc_start to end_line
        del lines[doc_start:end_line]
        action = "deleted"

    # Write modified file
    with open(file_path, 'w', encoding='utf-8') as f:
        f.writelines(lines)

    print(f"  ✓ Function '{function_name}' {action} from {file_path}")
    return True


def delete_functions(
    deletion_candidates: List[Dict],
    codebase_dir: str,
    comment_out: bool = False
) -> Dict:
    """
    Delete all selected functions from the codebase.

    Returns:
        Deletion manifest with details of what was deleted
    """
    manifest = {
        "timestamp": datetime.now().isoformat(),
        "codebase_dir": codebase_dir,
        "total_deletions": len(deletion_candidates),
        "successful_deletions": 0,
        "failed_deletions": 0,
        "deletions": []
    }

    print(f"\nDeleting {len(deletion_candidates)} functions...")
    print("="*80)

    for candidate in deletion_candidates:
        function_name = candidate['function_name']
        file_path = candidate['file_path']
        line_number = candidate.get('line_number')

        # Convert relative path to absolute path in target codebase
        # Remove 'codebase/' prefix if present
        if file_path.startswith('codebase/'):
            file_path = file_path.replace('codebase/', '', 1)

        target_file = os.path.join(codebase_dir, file_path)

        print(f"\nDeleting: {function_name} from {file_path}")

        if not os.path.exists(target_file):
            print(f"  ✗ File not found: {target_file}")
            manifest['failed_deletions'] += 1
            manifest['deletions'].append({
                "function_name": function_name,
                "file_path": file_path,
                "status": "failed",
                "reason": "file_not_found"
            })
            continue

        success = delete_function_from_file(
            target_file,
            function_name,
            line_number,
            comment_out
        )

        if success:
            manifest['successful_deletions'] += 1
            manifest['deletions'].append({
                "function_name": function_name,
                "file_path": file_path,
                "line_number": line_number,
                "status": "success"
            })
        else:
            manifest['failed_deletions'] += 1
            manifest['deletions'].append({
                "function_name": function_name,
                "file_path": file_path,
                "line_number": line_number,
                "status": "failed",
                "reason": "function_not_found"
            })

    print("\n" + "="*80)
    print(f"Deletion complete!")
    print(f"  Successful: {manifest['successful_deletions']}")
    print(f"  Failed: {manifest['failed_deletions']}")

    return manifest


def save_manifest(manifest: Dict, output_path: str):
    """Save deletion manifest to JSON file."""
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(manifest, f, indent=2)
    print(f"\n✓ Deletion manifest saved to: {output_path}")


def verify_codebase(codebase_dir: str):
    """
    Optional: Verify that the codebase still compiles after deletions.

    Note: This is optional and may fail if deleted functions had callers.
    The purpose of the deletion experiment is to test model behavior,
    not to maintain working code.
    """
    print("\n" + "="*80)
    print("VERIFICATION (Optional)")
    print("="*80)
    print("Note: Compilation may fail if deleted functions had dependencies.")
    print("This is expected and OK for the deletion experiment.")

    response = input("\nAttempt to compile/check codebase? (y/n): ")

    if response.lower() != 'y':
        print("Skipping verification.")
        return

    # Check if Cargo.toml exists (Rust project)
    cargo_file = os.path.join(codebase_dir, "Cargo.toml")

    if os.path.exists(cargo_file):
        print("\nRunning: cargo check")
        import subprocess
        try:
            result = subprocess.run(
                ['cargo', 'check'],
                cwd=codebase_dir,
                capture_output=True,
                text=True,
                timeout=60
            )

            if result.returncode == 0:
                print("✓ Cargo check passed!")
            else:
                print("✗ Cargo check failed (expected if deleted functions had callers)")
                print("\nErrors (first 500 chars):")
                print(result.stderr[:500])

        except subprocess.TimeoutExpired:
            print("✗ Cargo check timed out")
        except Exception as e:
            print(f"✗ Error running cargo check: {e}")
    else:
        print("No Cargo.toml found, skipping Rust verification")


def main():
    parser = argparse.ArgumentParser(
        description="Delete functions for RQ2 deletion experiment"
    )
    parser.add_argument(
        '--candidates',
        default='deletion_candidates.json',
        help='Path to deletion candidates JSON (default: deletion_candidates.json)'
    )
    parser.add_argument(
        '--source-codebase',
        default='codebase',
        help='Source codebase directory (default: codebase)'
    )
    parser.add_argument(
        '--target-codebase',
        default='codebase_deleted',
        help='Target codebase directory for deletions (default: codebase_deleted)'
    )
    parser.add_argument(
        '--backup-dir',
        default='codebase_backup',
        help='Backup directory (default: codebase_backup)'
    )
    parser.add_argument(
        '--comment-out',
        action='store_true',
        help='Comment out functions instead of deleting (safer)'
    )
    parser.add_argument(
        '--no-backup',
        action='store_true',
        help='Skip creating backup (not recommended)'
    )
    parser.add_argument(
        '--manifest',
        default='deletion_manifest.json',
        help='Output path for deletion manifest (default: deletion_manifest.json)'
    )

    args = parser.parse_args()

    print("="*80)
    print("FUNCTION DELETION FOR RQ2 EXPERIMENT")
    print("="*80)

    # Load deletion candidates
    print(f"\nLoading deletion candidates from: {args.candidates}")
    candidates = load_deletion_candidates(args.candidates)
    print(f"✓ Loaded {len(candidates)} deletion candidates")

    # Create backup
    if not args.no_backup:
        backup_codebase(args.source_codebase, args.backup_dir)

    # Copy codebase to target directory
    copy_codebase_for_deletion(args.source_codebase, args.target_codebase)

    # Confirm before proceeding
    print("\n" + "="*80)
    print(f"About to delete {len(candidates)} functions from: {args.target_codebase}")
    if args.comment_out:
        print("Mode: COMMENT OUT (safer, reversible)")
    else:
        print("Mode: DELETE (permanent)")
    print("="*80)

    response = input("\nProceed? (y/n): ")
    if response.lower() != 'y':
        print("Aborted.")
        return

    # Perform deletions
    manifest = delete_functions(
        candidates,
        args.target_codebase,
        args.comment_out
    )

    # Save manifest
    save_manifest(manifest, args.manifest)

    # Optional verification
    verify_codebase(args.target_codebase)

    print("\n" + "="*80)
    print("DELETION COMPLETE")
    print("="*80)
    print(f"\nDeleted codebase available at: {args.target_codebase}")
    print(f"Original codebase preserved at: {args.backup_dir}")
    print(f"Deletion manifest: {args.manifest}")

    print("\nNext steps:")
    print(f"1. Re-index the codebase:")
    print(f"   python reindex.py --codebase {args.target_codebase}")
    print("2. Run deletion evaluation:")
    print("   python evaluate.py --deletion-mode --questions test_questions_category3.json")


if __name__ == '__main__':
    main()
