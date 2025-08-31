#!/usr/bin/env python3
"""
Directory Manifest Generator

Generates a CSV manifest file for a directory containing:
- Relative file paths
- SHA256 hashes
- File sizes in bytes

This file does not get used as part of the downloader implementation.
It's a utility for generating a manifest for a directory, which then gets used in the validation process.

Usage: python3 generate_manifest.py <directory_path> [output_file]
"""

import os
import sys
import hashlib
import csv
import argparse
from pathlib import Path


def calculate_sha256(file_path):
    """Calculate SHA256 hash of a file."""
    sha256_hash = hashlib.sha256()
    try:
        with open(file_path, "rb") as f:
            for chunk in iter(lambda: f.read(4096), b""):
                sha256_hash.update(chunk)
        return sha256_hash.hexdigest()
    except Exception as e:
        print(f"Error calculating hash for {file_path}: {e}", file=sys.stderr)
        return None


def get_file_size(file_path):
    """Get file size in bytes."""
    try:
        return os.path.getsize(file_path)
    except Exception as e:
        print(f"Error getting size for {file_path}: {e}", file=sys.stderr)
        return 0


def generate_manifest(directory_path, output_file=None):
    """Generate manifest for all files in directory."""
    directory = Path(directory_path)
    
    if not directory.exists():
        print(f"Error: Directory '{directory_path}' does not exist", file=sys.stderr)
        return False
    
    if not directory.is_dir():
        print(f"Error: '{directory_path}' is not a directory", file=sys.stderr)
        return False
    
    if output_file is None:
        output_file = f"{directory.name}_manifest.csv"
    
    files_data = []
    
    print(f"Scanning directory: {directory_path}")
    
    for file_path in directory.rglob('*'):
        if file_path.is_file():
            print(f"Processing: {file_path}")
            
            relative_path = file_path.relative_to(directory)
            
            file_hash = calculate_sha256(file_path)
            file_size = get_file_size(file_path)
            
            if file_hash is not None:
                files_data.append({
                    'file_path': str(relative_path).replace('\\', '/'),  # Normalize path separators
                    'file_hash': file_hash,
                    'file_size': file_size
                })
    
    if not files_data:
        print("Warning: No files found in directory", file=sys.stderr)
        return False
    
    files_data.sort(key=lambda x: x['file_path'])
    
    try:
        with open(output_file, 'w', newline='', encoding='utf-8') as csvfile:
            fieldnames = ['file_path', 'file_hash', 'file_size']
            writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
            
            writer.writeheader()
            for file_data in files_data:
                writer.writerow(file_data)
        
        print(f"\nManifest generated successfully!")
        print(f"Output file: {output_file}")
        print(f"Total files: {len(files_data)}")
        print(f"Total size: {sum(f['file_size'] for f in files_data):,} bytes")
        
        return True
        
    except Exception as e:
        print(f"Error writing manifest file: {e}", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Generate CSV manifest for directory contents",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python3 generate_manifest.py /path/to/directory
  python3 generate_manifest.py ./my_files custom_manifest.csv
  python3 generate_manifest.py ~/Documents documents_manifest.csv
        """
    )
    
    parser.add_argument(
        'directory',
        help='Directory to scan for files'
    )
    
    parser.add_argument(
        'output',
        nargs='?',
        help='Output CSV file (default: DIRECTORY_NAME_manifest.csv)'
    )
    
    parser.add_argument(
        '--quiet', '-q',
        action='store_true',
        help='Suppress progress output'
    )
    
    args = parser.parse_args()
    
    if args.quiet:
        original_stdout = sys.stdout
        sys.stdout = open(os.devnull, 'w')
    try:
        success = generate_manifest(args.directory, args.output)
        
        if args.quiet:
            sys.stdout.close()
            sys.stdout = original_stdout
        
        sys.exit(0 if success else 1)
        
    except KeyboardInterrupt:
        if args.quiet:
            sys.stdout.close()
            sys.stdout = original_stdout
        print("\nOperation cancelled by user", file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
