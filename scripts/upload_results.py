#!/usr/bin/env python3

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


def parse_log_file(log_path: Path) -> List[UploadAttempt]:
    """
    Parse the upload log file and extract upload attempts.

    Args:
        log_path: Path to the log file

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
                attempt = parse_upload_attempt(lines, i)
                if attempt:
                    attempts.append(attempt)

        i += 1

    return attempts


def parse_upload_attempt(lines: List[str], start_idx: int) -> Optional[UploadAttempt]:
    """
    Parse a single upload attempt starting from the banner line.

    Args:
        lines: All lines from the log file
        start_idx: Index of the banner line

    Returns:
        UploadAttempt object or None if parsing fails
    """
    file_name = None
    size_kb = None
    success = False
    address = None
    duration_seconds = None
    start_time = None

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

        if 'Successfully uploaded:' in line:
            success = True

        if address is None and 'At address:' in line:
            match = re.search(r'At address:\s*([a-f0-9]+)', line)
            if match:
                address = match.group(1).strip()

        if duration_seconds is None and 'Elapsed time:' in line:
            match = re.search(r'Elapsed time:\s*([\d.]+)\s*seconds', line)
            if match:
                duration_seconds = float(match.group(1))

        if 'Failed to upload' in line and file_name and file_name in line:
            success = False

        if duration_seconds is not None:
            break

        if i > start_idx + 2 and '==========================================' in line:
            break

    if file_name and size_kb is not None and duration_seconds is not None and start_time:
        return UploadAttempt(
            file_name=file_name,
            size_kb=size_kb,
            success=success,
            address=address,
            duration_seconds=duration_seconds,
            start_time=start_time
        )

    return None


def format_size_mb(size_kb: int) -> str:
    """Convert KB to MB and format as string."""
    return f"{size_kb / 1024:.2f}"


def format_duration_minutes(duration_seconds: float) -> str:
    """Convert seconds to minutes and format as string."""
    return f"{duration_seconds / 60:.2f}"


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
    print(f"{'Result':<8} {'Start Time':<20} {'File Name':<40} {'Size (MB)':<12} {'Duration (min)'}")
    print("-" * 100)

    for attempt in attempts:
        file_name = Path(attempt.file_name).name if '/' in attempt.file_name else attempt.file_name
        result = "✓" if attempt.success else "✗"
        size_mb = format_size_mb(attempt.size_kb)
        duration_min = format_duration_minutes(attempt.duration_seconds)

        if len(file_name) > 40:
            file_name = file_name[:37] + "..."

        print(f"{result:<8} {attempt.start_time:<20} {file_name:<40} {size_mb:<12} {duration_min}")

    print("-" * 100)

    successful_count = sum(1 for a in attempts if a.success)
    failed_count = len(attempts) - successful_count
    total_size_mb = sum(a.size_kb for a in attempts) / 1024
    total_duration_min = sum(a.duration_seconds for a in attempts) / 60

    print(f"\nSummary:")
    print(f"  Total attempts: {len(attempts)}")
    print(f"  Successful: {successful_count} (✓)")
    print(f"  Failed: {failed_count} (✗)")
    print(f"  Total data processed: {total_size_mb:.2f} MB")
    print(f"  Total time: {total_duration_min:.2f} minutes")
    if successful_count > 0:
        success_rate = (successful_count / len(attempts)) * 100
        print(f"  Success rate: {success_rate:.1f}%")
    print()


def main():
    """Main entry point."""
    if len(sys.argv) != 2:
        print("Usage: upload_results.py <log_file_path>")
        print("\nExample:")
        print("  upload_results.py static-upload-results/DEV_01-2025_12_27_00_12_50/DEV-01-ant-client-1/service_log")
        sys.exit(1)

    log_path = Path(sys.argv[1])

    if not log_path.exists():
        print(f"Error: Log file not found: {log_path}")
        sys.exit(1)

    print(f"Parsing log file: {log_path}")
    attempts = parse_log_file(log_path)

    if not attempts:
        print("No upload attempts found in log file.")
        sys.exit(0)

    print_successful_uploads_table(attempts)
    print_all_attempts_table(attempts)


if __name__ == "__main__":
    main()
