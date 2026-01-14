#!/usr/bin/env python3

import argparse
import re
import sys
from pathlib import Path
from typing import List, Optional
from dataclasses import dataclass


@dataclass
class UploadAttempt:
    file_name: str
    size_kb: int
    success: bool
    address: Optional[str]
    duration_seconds: float
    start_time: str
    total_chunks: Optional[int] = None
    successful_chunks: Optional[int] = None


def parse_log_file(log_path: Path, payment_type: str) -> List[UploadAttempt]:
    """
    Parse the upload log file and extract upload attempts.

    Args:
        log_path: Path to the log file
        payment_type: Payment type ('single-node' or 'merkle')

    Returns:
        List of UploadAttempt objects
    """
    attempts = []

    with open(log_path, 'r') as f:
        lines = f.readlines()

    i = 0
    while i < len(lines):
        line = lines[i]

        if '==========================================' in line:
            if i + 1 < len(lines) and 'Uploading Content' in lines[i + 1]:
                attempt = parse_upload_attempt(lines, i, payment_type)
                if attempt:
                    attempts.append(attempt)

        i += 1

    return attempts


def parse_upload_attempt(lines: List[str], start_idx: int, payment_type: str) -> Optional[UploadAttempt]:
    """
    Parse a single upload attempt starting from the banner line.

    Args:
        lines: All lines from the log file
        start_idx: Index of the banner line
        payment_type: Payment type ('single-node' or 'merkle')

    Returns:
        UploadAttempt object or None if parsing fails
    """
    file_name = None
    size_kb = None
    success = False
    address = None
    duration_seconds = None
    duration_found_at = None
    start_time = None
    total_chunks = None
    successful_chunk_addresses = set()
    all_chunks_already_exist = False

    timestamp_match = re.match(r'^(\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})', lines[start_idx])
    if timestamp_match:
        start_time = timestamp_match.group(1)

    max_lines = min(start_idx + 10000, len(lines))

    for i in range(start_idx, max_lines):
        line = lines[i]

        if file_name is None and 'File/Directory:' in line:
            match = re.search(r'File/Directory:\s*(.+)$', line)
            if match:
                file_name = match.group(1).strip()

        if size_kb is None and 'Size:' in line and 'KB' in line:
            match = re.search(r'Size:\s*(\d+)KB', line)
            if match:
                size_kb = int(match.group(1))

        # Handle both "Successfully uploaded:" and "Successfully uploaded /path"
        if 'Successfully uploaded' in line and 'Failed' not in line:
            success = True

        if address is None and 'At address:' in line:
            match = re.search(r'At address:\s*([a-f0-9]+)', line)
            if match:
                address = match.group(1).strip()

        # For merkle "all chunks exist" case, parse address from: - "filename": "address"
        if address is None and file_name:
            # Match pattern like: - "VIDEO_TS.IFO": "633d180f..."
            basename = Path(file_name).name if '/' in file_name else file_name
            if f'"{basename}"' in line and '": "' in line:
                match = re.search(r'": "([a-f0-9]{64})"', line)
                if match:
                    address = match.group(1)

        if duration_seconds is None and 'Elapsed time:' in line:
            match = re.search(r'Elapsed time:\s*([\d.]+)\s*seconds', line)
            if match:
                duration_seconds = float(match.group(1))
                duration_found_at = i

        if 'Failed to upload' in line and file_name and file_name in line:
            success = False

        # Parse chunk statistics based on payment type
        if payment_type == 'single-node':
            if total_chunks is None and 'Processing estimated total' in line:
                match = re.search(r'Processing estimated total (\d+) chunks', line)
                if match:
                    total_chunks = int(match.group(1))
        elif payment_type == 'merkle':
            # For merkle, get total chunks from "🚀 Starting upload of N chunks in M Merkle Tree(s)..."
            # This is the authoritative source, so always use it (overwrite any earlier value)
            if 'Starting upload of' in line and 'Merkle Tree' in line:
                match = re.search(r'Starting upload of (\d+) chunks in \d+ Merkle Tree', line)
                if match:
                    total_chunks = int(match.group(1))
            # Fallback: when all chunks already exist, get count from "Encrypted X/Y chunks"
            # Only use this if we haven't found "Starting upload" yet
            elif total_chunks is None and 'Encrypted' in line and 'chunks in' in line:
                match = re.search(r'Encrypted (\d+)/(\d+) chunks in', line)
                if match:
                    total_chunks = int(match.group(2))

        # Chunk stored messages have the same format for both payment types
        chunk_match = re.search(r'\(\d+/\d+\) Chunk stored at: ([a-f0-9]{64})', line)
        if chunk_match:
            successful_chunk_addresses.add(chunk_match.group(1))

        # Also count chunks that succeeded on retry
        retry_match = re.search(r'Retry succeeded for chunk: ([a-f0-9]{64})', line)
        if retry_match:
            successful_chunk_addresses.add(retry_match.group(1))

        # Detect when all chunks already exist on network (nothing to upload)
        if 'chunks already exist on the network' in line and 'nothing to upload' in line:
            all_chunks_already_exist = True

        # After finding duration, continue for a few more lines to catch success/failure status
        if duration_found_at is not None and i > duration_found_at + 5:
            break

        if i > start_idx + 2 and '==========================================' in line:
            break

    if file_name and size_kb is not None and duration_seconds is not None and start_time:
        # Set successful_chunks based on what was stored or already existed
        if total_chunks is not None:
            if all_chunks_already_exist:
                # All chunks were already on the network
                successful_chunks = total_chunks
            else:
                # Cap at total_chunks to handle cases where "already exists" chunks + new chunks > total
                successful_chunks = min(len(successful_chunk_addresses), total_chunks)
        else:
            successful_chunks = None
        return UploadAttempt(
            file_name=file_name,
            size_kb=size_kb,
            success=success,
            address=address,
            duration_seconds=duration_seconds,
            start_time=start_time,
            total_chunks=total_chunks,
            successful_chunks=successful_chunks
        )

    return None


def format_size_mb(size_kb: int) -> str:
    """Convert KB to MB and format as string."""
    return f"{size_kb / 1024:.2f}"


def format_duration_minutes(duration_seconds: float) -> str:
    """Convert seconds to minutes and format as string.

    If duration is less than 60 minutes, returns decimal minutes (e.g., "45.23").
    If duration is 60 minutes or more, returns HHhMMm format (e.g., "1h30m").
    """
    total_minutes = duration_seconds / 60
    if total_minutes < 60:
        return f"{total_minutes:.2f}m"
    else:
        hours = int(total_minutes // 60)
        minutes = int(total_minutes % 60)
        return f"{hours}h{minutes:02d}m"


def format_chunks(attempt: UploadAttempt) -> str:
    """Format chunk statistics as 'successful/total' or 'N/A'."""
    if attempt.total_chunks is not None and attempt.successful_chunks is not None:
        return f"{attempt.successful_chunks}/{attempt.total_chunks}"
    elif attempt.successful_chunks is not None:
        return f"{attempt.successful_chunks}/?"
    else:
        return "N/A"


def print_successful_uploads_table(attempts: List[UploadAttempt]) -> None:
    """Print table of successful uploads with addresses."""
    successful = [a for a in attempts if a.success and a.address]

    if not successful:
        print("No successful uploads found.\n")
        return

    print("\n" + "=" * 80)
    print("SUCCESSFUL UPLOADS")
    print("=" * 80)
    print(f"{'File Name':<60} {'Address'}")
    print("-" * 80)

    for attempt in successful:
        file_name = Path(attempt.file_name).name if '/' in attempt.file_name else attempt.file_name
        print(f"{file_name:<60} {attempt.address}")

    print("-" * 80)
    print(f"Total successful uploads: {len(successful)}\n")


def print_all_attempts_table(attempts: List[UploadAttempt]) -> None:
    """Print table of all upload attempts."""
    print("\n" + "=" * 100)
    print("UPLOAD ATTEMPTS")
    print("=" * 100)
    print(f"{'Result':<8} {'Start Time':<20} {'File Name':<40} {'Chunks':<12} {'Duration'}")
    print("-" * 100)

    for attempt in attempts:
        file_name = Path(attempt.file_name).name if '/' in attempt.file_name else attempt.file_name
        result = "✓" if attempt.success else "✗"
        chunks = format_chunks(attempt)
        duration_min = format_duration_minutes(attempt.duration_seconds)

        if len(file_name) > 40:
            file_name = file_name[:37] + "..."

        print(f"{result:<8} {attempt.start_time:<20} {file_name:<40} {chunks:<12} {duration_min}")

    print("-" * 100)

    successful_count = sum(1 for a in attempts if a.success)
    failed_count = len(attempts) - successful_count
    total_size_mb = sum(a.size_kb for a in attempts) / 1024
    total_duration_min = sum(a.duration_seconds for a in attempts) / 60

    total_chunks_all = sum(a.total_chunks for a in attempts if a.total_chunks is not None)
    successful_chunks_all = sum(a.successful_chunks for a in attempts if a.successful_chunks is not None)

    print(f"\nSummary:")
    print(f"  Total attempts: {len(attempts)}")
    print(f"  Successful: {successful_count} (✓)")
    print(f"  Failed: {failed_count} (✗)")
    print(f"  Total data processed: {total_size_mb:.2f} MB")
    if total_chunks_all > 0:
        chunk_success_rate = (successful_chunks_all / total_chunks_all) * 100
        print(f"  Total chunks: {successful_chunks_all}/{total_chunks_all} ({chunk_success_rate:.1f}% success)")
    print(f"  Total time: {total_duration_min:.2f} minutes")
    if successful_count > 0:
        success_rate = (successful_count / len(attempts)) * 100
        print(f"  Upload success rate: {success_rate:.1f}%")
    print()


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description='Parse upload log files and generate statistics',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Example:
  upload_results.py static-upload-results/DEV_01-2025_12_27/DEV-01-ant-client-1/service_log --payment-type single-node
        """
    )
    parser.add_argument('log_file_path', type=str, help='Path to the log file')
    parser.add_argument(
        '--payment-type',
        type=str,
        required=True,
        choices=['single-node', 'merkle'],
        help='Payment type used in the upload (single-node or merkle)'
    )

    args = parser.parse_args()

    log_path = Path(args.log_file_path)

    if not log_path.exists():
        print(f"Error: Log file not found: {log_path}")
        sys.exit(1)

    print(f"Parsing log file: {log_path}")
    print(f"Payment type: {args.payment_type}")

    attempts = parse_log_file(log_path, args.payment_type)

    if not attempts:
        print("No upload attempts found in log file.")
        sys.exit(0)

    print_successful_uploads_table(attempts)
    print_all_attempts_table(attempts)


if __name__ == "__main__":
    main()
