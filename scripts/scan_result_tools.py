#!/usr/bin/env python3
"""
Analyze scan repair results for Safe Network testnets.

This tool provides commands to analyze CSV files generated during network
scanning and repair operations.
"""

import argparse
import csv
import re
import sys
from pathlib import Path


def count_lines_in_csv(csv_path):
    """
    Count non-empty lines in a CSV file.

    Args:
        csv_path: Path to the CSV file

    Returns:
        Number of non-empty lines, or None if file could not be read
    """
    try:
        with open(csv_path, 'r') as f:
            lines = [line for line in f if line.strip()]
            return len(lines)
    except Exception as e:
        print(f"Warning: Could not read {csv_path}: {e}", file=sys.stderr)
        return None


def count_repair_csv_by_status(csv_path, status_column_index, cost_column_index):
    """
    Count entries in a repair CSV file by upload_status and cost_paid.

    Args:
        csv_path: Path to the CSV file
        status_column_index: Column index for upload_status (2 for initial_repair, 3 for network_scan_repair)
        cost_column_index: Column index for cost_paid (3 for initial_repair, 4 for network_scan_repair)

    Returns:
        Tuple of (success_count, failed_count, success_with_cost_count), or (None, None, None) if file could not be read
    """
    try:
        success_count = 0
        failed_count = 0
        paid_count = 0

        with open(csv_path, 'r') as f:
            reader = csv.reader(f)
            for row in reader:
                if not row or len(row) <= max(status_column_index, cost_column_index):
                    continue

                status = row[status_column_index].strip().lower()
                if status == 'success':
                    success_count += 1
                    cost_paid = float(row[cost_column_index].strip())
                    if cost_paid != 0.0:
                        paid_count += 1
                elif status == 'failed':
                    failed_count += 1

        return (success_count, failed_count, paid_count)
    except Exception as e:
        print(f"Warning: Could not read {csv_path}: {e}", file=sys.stderr)
        return (None, None, None)


def get_host_summary(path):
    """
    Get a summary of all metrics for each host in a testnet.

    Args:
        path: Path to the testnet directory containing host subdirectories

    Returns:
        Dictionary mapping host names to their metrics:
        {
            'host-name': {
                'whitelist': count,
                'badlist': count,
                'repair_success': count,
                'repair_failed': count,
                'repair_paid': count
            }
        }
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    host_summaries = {}

    for host_dir in sorted(testnet_path.iterdir()):
        if not host_dir.is_dir():
            continue

        host_name = host_dir.name
        host_summaries[host_name] = {
            'whitelist': 0,
            'badlist': 0,
            'repair_success': 0,
            'repair_failed': 0,
            'repair_paid': 0
        }

        whitelist_path = host_dir / "chunk_whitelist.csv"
        if whitelist_path.exists():
            count = count_lines_in_csv(whitelist_path)
            if count is not None:
                host_summaries[host_name]['whitelist'] = count

        badlist_path = host_dir / "chunk_badlist.csv"
        if badlist_path.exists():
            count = count_lines_in_csv(badlist_path)
            if count is not None:
                host_summaries[host_name]['badlist'] = count

        for csv_path in host_dir.glob("network_scan_repair_*.csv"):
            success, failed, paid = count_repair_csv_by_status(csv_path, 3, 4)
            if success is not None and failed is not None and paid is not None:
                host_summaries[host_name]['repair_success'] += success
                host_summaries[host_name]['repair_failed'] += failed
                host_summaries[host_name]['repair_paid'] += paid

        for csv_path in host_dir.glob("initial_repair_*.csv"):
            success, failed, paid = count_repair_csv_by_status(csv_path, 2, 3)
            if success is not None and failed is not None and paid is not None:
                host_summaries[host_name]['repair_success'] += success
                host_summaries[host_name]['repair_failed'] += failed
                host_summaries[host_name]['repair_paid'] += paid

    return host_summaries


def count_csv_entries(path, csv_filename):
    """
    Count total lines across all CSV files of a given type for a testnet.

    Args:
        path: Path to the testnet directory containing host subdirectories
        csv_filename: Name of the CSV file to count (e.g., "chunk_whitelist.csv", "chunk_badlist.csv")

    Returns:
        Total count of entries
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    total_count = 0
    files_processed = 0

    for csv_path in testnet_path.glob(f"*/{csv_filename}"):
        line_count = count_lines_in_csv(csv_path)
        if line_count is not None:
            total_count += line_count
            files_processed += 1
            print(f"  {csv_path.parent.name}: {line_count} entries")

    if files_processed == 0:
        print(f"Warning: No {csv_filename} files found in {testnet_path}", file=sys.stderr)

    return total_count


def count_repair_entries(path):
    """
    Count total lines across all repair CSV files (network_scan_repair_*.csv and initial_repair_*.csv),
    broken down by upload_status (success/failed) and success with cost_paid != 0.

    Args:
        path: Path to the testnet directory containing host subdirectories

    Returns:
        Tuple of (total_success, total_failed, total_success_with_cost)
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    total_success = 0
    total_failed = 0
    total_success_with_cost = 0
    files_processed = 0

    # network_scan_repair_*.csv files have upload_status in column 3, cost_paid in column 4 (0-indexed)
    for csv_path in testnet_path.glob("*/network_scan_repair_*.csv"):
        success_count, failed_count, success_with_cost = count_repair_csv_by_status(csv_path, 3, 4)
        if success_count is not None and failed_count is not None and success_with_cost is not None:
            total_success += success_count
            total_failed += failed_count
            total_success_with_cost += success_with_cost
            files_processed += 1
            total = success_count + failed_count
            print(f"  {csv_path.parent.name}/{csv_path.name}: {total} entries (success: {success_count}, failed: {failed_count}, paid: {success_with_cost})")

    # initial_repair_*.csv files have upload_status in column 2, cost_paid in column 3 (0-indexed)
    for csv_path in testnet_path.glob("*/initial_repair_*.csv"):
        success_count, failed_count, success_with_cost = count_repair_csv_by_status(csv_path, 2, 3)
        if success_count is not None and failed_count is not None and success_with_cost is not None:
            total_success += success_count
            total_failed += failed_count
            total_success_with_cost += success_with_cost
            files_processed += 1
            total = success_count + failed_count
            print(f"  {csv_path.parent.name}/{csv_path.name}: {total} entries (success: {success_count}, failed: {failed_count}, paid: {success_with_cost})")

    if files_processed == 0:
        print(f"Warning: No repair CSV files found in {testnet_path}", file=sys.stderr)

    return (total_success, total_failed, total_success_with_cost)


def extract_timestamp_from_filename(filename):
    """
    Extract timestamp from repair CSV filename.

    Args:
        filename: Filename like "initial_repair_20251114_082048.csv" or "network_scan_repair_20251114_004152.csv"

    Returns:
        Timestamp string like "20251114_082048", or None if no match
    """
    match = re.search(r'(\d{8}_\d{6})\.csv$', filename)
    return match.group(1) if match else None


def get_latest_file(file_list):
    """
    Find the latest file from a list based on timestamp in filename.

    Args:
        file_list: List of Path objects

    Returns:
        Path object of the latest file

    Raises:
        ValueError: If file_list is empty or no files have valid timestamps
    """
    if not file_list:
        raise ValueError("Cannot find latest file: file list is empty")

    files_with_timestamps = []
    for file_path in file_list:
        timestamp = extract_timestamp_from_filename(file_path.name)
        if timestamp:
            files_with_timestamps.append((file_path, timestamp))

    if not files_with_timestamps:
        raise ValueError(f"Cannot find latest file: no files with valid timestamps found in {len(file_list)} file(s)")

    # Sort by timestamp (descending) and return the latest
    files_with_timestamps.sort(key=lambda x: x[1], reverse=True)
    return files_with_timestamps[0][0]


def calculate_lost_chunks(path):
    """
    Calculate the number of lost chunks.

    Formula: total_badlist_rows - (total_last_initial_repair_rows + total_last_network_scan_repair_rows)

    Args:
        path: Path to the testnet directory containing host subdirectories

    Returns:
        Tuple of (lost_chunks, total_badlist, total_initial_repair, total_network_scan_repair)
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    total_badlist = 0
    total_initial_repair = 0
    total_network_scan_repair = 0

    for host_dir in sorted(testnet_path.iterdir()):
        if not host_dir.is_dir():
            continue

        host_name = host_dir.name

        badlist_path = host_dir / "chunk_badlist.csv"
        if badlist_path.exists():
            count = count_lines_in_csv(badlist_path)
            if count is not None:
                total_badlist += count
                print(f"  {host_name}/chunk_badlist.csv: {count} entries")

        initial_repair_files = list(host_dir.glob("initial_repair_*.csv"))
        if initial_repair_files:
            try:
                latest_initial = get_latest_file(initial_repair_files)
                count = count_lines_in_csv(latest_initial)
                if count is not None:
                    total_initial_repair += count
                    print(f"  {host_name}/{latest_initial.name}: {count} entries (latest initial_repair)")
            except ValueError as e:
                print(f"Warning: {host_name}: {e}", file=sys.stderr)

        network_scan_files = list(host_dir.glob("network_scan_repair_*.csv"))
        if network_scan_files:
            try:
                latest_network = get_latest_file(network_scan_files)
                count = count_lines_in_csv(latest_network)
                if count is not None:
                    total_network_scan_repair += count
                    print(f"  {host_name}/{latest_network.name}: {count} entries (latest network_scan_repair)")
            except ValueError as e:
                print(f"Warning: {host_name}: {e}", file=sys.stderr)

    lost_chunks = total_badlist - (total_initial_repair + total_network_scan_repair)
    return (lost_chunks, total_badlist, total_initial_repair, total_network_scan_repair)


def combine_badlist(path, output_path):
    """
    Combine all chunk_badlist.csv files into a single CSV file.

    Args:
        path: Path to the testnet directory containing host subdirectories
        output_path: Path to the output CSV file

    Returns:
        Number of total rows written (excluding header)
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    badlist_files = list(testnet_path.glob("*/chunk_badlist.csv"))

    if not badlist_files:
        print(f"Error: No chunk_badlist.csv files found in {testnet_path}", file=sys.stderr)
        sys.exit(1)

    total_rows = 0
    header_written = False

    try:
        with open(output_path, 'w') as outfile:
            for badlist_path in sorted(badlist_files):
                print(f"  Processing {badlist_path.parent.name}/chunk_badlist.csv...")
                try:
                    with open(badlist_path, 'r') as infile:
                        lines = infile.readlines()
                        if not lines:
                            continue

                        # Write header only once for the first file
                        if not header_written:
                            outfile.write(lines[0])
                            header_written = True

                        for line in lines[1:]:
                            if line.strip():
                                outfile.write(line)
                                total_rows += 1
                except Exception as e:
                    print(f"Warning: Could not read {badlist_path}: {e}", file=sys.stderr)
                    continue
        print(f"\nSuccessfully wrote {total_rows} rows to {output_path}")
        return total_rows
    except Exception as e:
        print(f"Error: Could not write to {output_path}: {e}", file=sys.stderr)
        sys.exit(1)


def print_repair_errors(path):
    """
    Print errors from failed repair operations, excluding serialization errors.

    Processes all network_scan_repair_*.csv and initial_repair_*.csv files,
    printing the error column value for any row with upload_status='failed'
    that does not start with "Serialization error".

    Args:
        path: Path to the testnet directory containing host subdirectories

    Returns:
        Total count of non-serialization errors found
    """
    testnet_path = Path(path)

    if not testnet_path.exists():
        print(f"Error: Testnet directory '{testnet_path}' does not exist", file=sys.stderr)
        sys.exit(1)

    if not testnet_path.is_dir():
        print(f"Error: '{testnet_path}' is not a directory", file=sys.stderr)
        sys.exit(1)

    total_errors = 0
    files_processed = 0

    # Process network_scan_repair_*.csv files (upload_status in column 3, error in column 5)
    for csv_path in testnet_path.glob("*/network_scan_repair_*.csv"):
        try:
            with open(csv_path, 'r') as f:
                reader = csv.reader(f)
                for row in reader:
                    if not row or len(row) <= 5:
                        continue

                    status = row[3].strip().lower()
                    if status == 'failed':
                        error_msg = row[5].strip()
                        if not error_msg.startswith("Serialization error"):
                            print(f"{csv_path.parent.name}/{csv_path.name}: {error_msg}")
                            print()
                            total_errors += 1

            files_processed += 1
        except Exception as e:
            print(f"Warning: Could not read {csv_path}: {e}", file=sys.stderr)
            continue

    # Process initial_repair_*.csv files (upload_status in column 2, error in column 4)
    for csv_path in testnet_path.glob("*/initial_repair_*.csv"):
        try:
            with open(csv_path, 'r') as f:
                reader = csv.reader(f)
                for row in reader:
                    if not row or len(row) <= 4:
                        continue

                    status = row[2].strip().lower()
                    if status == 'failed':
                        error_msg = row[4].strip()
                        if not error_msg.startswith("Serialization error"):
                            print(f"{csv_path.parent.name}/{csv_path.name}: {error_msg}")
                            print()
                            total_errors += 1

            files_processed += 1
        except Exception as e:
            print(f"Warning: Could not read {csv_path}: {e}", file=sys.stderr)
            continue

    if files_processed == 0:
        print(f"Warning: No repair CSV files found in {testnet_path}", file=sys.stderr)

    return total_errors


def main():
    """Main entry point for the scan results analyzer."""
    parser = argparse.ArgumentParser(
        description="Analyze Safe Network scan repair results",
        formatter_class=argparse.RawDescriptionHelpFormatter
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    whitelist_parser = subparsers.add_parser(
        "whitelist-count",
        help="Count total entries across all chunk_whitelist.csv files"
    )
    whitelist_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    badlist_parser = subparsers.add_parser(
        "badlist-count",
        help="Count total entries across all chunk_badlist.csv files"
    )
    badlist_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    repair_parser = subparsers.add_parser(
        "repair-count",
        help="Count total entries across all network_scan_repair_*.csv and initial_repair_*.csv files"
    )
    repair_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    summary_parser = subparsers.add_parser(
        "summary",
        help="Display summary of all metrics (whitelist, badlist, repair) per host with totals"
    )
    summary_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    lost_chunks_parser = subparsers.add_parser(
        "lost-chunks",
        help="Calculate the number of lost chunks (badlist - last initial_repair - last network_scan_repair)"
    )
    lost_chunks_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    combine_badlist_parser = subparsers.add_parser(
        "combine-badlist",
        help="Combine all chunk_badlist.csv files into a single CSV file"
    )
    combine_badlist_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )
    combine_badlist_parser.add_argument(
        "--output-path",
        required=True,
        help="Output CSV file path"
    )

    print_errors_parser = subparsers.add_parser(
        "print-errors",
        help="Print non-serialization errors from failed repair operations"
    )
    print_errors_parser.add_argument(
        "--path",
        required=True,
        help="Path to the testnet directory containing host subdirectories"
    )

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(1)

    if args.command == "whitelist-count":
        print(f"Analyzing whitelist entries in {args.path}...")
        print()
        total = count_csv_entries(args.path, "chunk_whitelist.csv")
        print()
        print(f"Total whitelist entries: {total}")
    elif args.command == "badlist-count":
        print(f"Analyzing badlist entries in {args.path}...")
        print()
        total = count_csv_entries(args.path, "chunk_badlist.csv")
        print()
        print(f"Total badlist entries: {total}")
    elif args.command == "repair-count":
        print(f"Analyzing repair entries in {args.path}...")
        print()
        total_success, total_failed, total_paid = count_repair_entries(args.path)
        print()
        total = total_success + total_failed
        print(f"Total repair entries: {total}")
        print(f"  Success: {total_success}")
        print(f"  Failed: {total_failed}")
        print(f"  Paid: {total_paid}")
    elif args.command == "summary":
        host_summaries = get_host_summary(args.path)
        if not host_summaries:
            print("No hosts found")
            sys.exit(0)

        totals = {
            'whitelist': 0,
            'badlist': 0,
            'whitelist_plus_badlist': 0,
            'repair_success': 0,
            'repair_failed': 0,
            'repair_total': 0,
            'repair_paid': 0
        }

        for host_name in sorted(host_summaries.keys()):
            metrics = host_summaries[host_name]
            print(f"{host_name}:")
            print(f"  white list: {metrics['whitelist']}; bad list: {metrics['badlist']}; repair success: {metrics['repair_success']}; repair failed: {metrics['repair_failed']}; repair paid: {metrics['repair_paid']}")

            totals['whitelist'] += metrics['whitelist']
            totals['badlist'] += metrics['badlist']
            totals['whitelist_plus_badlist'] += metrics['whitelist'] +  metrics['badlist']
            totals['repair_success'] += metrics['repair_success']
            totals['repair_failed'] += metrics['repair_failed']
            totals['repair_total'] += metrics['repair_success'] + metrics['repair_failed']
            totals['repair_paid'] += metrics['repair_paid']

        print("=" * 50)
        print("TOTALS:")
        print(f"  white list: {totals['whitelist']}")
        print(f"  bad list: {totals['badlist']}")
        print(f"  repair success: {totals['repair_success']}")
        print(f"  repair failed: {totals['repair_failed']}")
        print(f"  repair paid: {totals['repair_paid']}")
        print(f"  total repair entries: {totals['repair_total']}")
    elif args.command == "lost-chunks":
        print(f"Calculating lost chunks in {args.path}...")
        print()
        lost_chunks, total_badlist, total_initial_repair, total_network_scan_repair = calculate_lost_chunks(args.path)
        print()
        print("=" * 50)
        print("CALCULATION:")
        print(f"  Total badlist entries: {total_badlist}")
        print(f"  Total latest initial_repair entries: {total_initial_repair}")
        print(f"  Total latest network_scan_repair entries: {total_network_scan_repair}")
        print()
        print(f"Lost chunks: {total_badlist} - ({total_initial_repair} + {total_network_scan_repair}) = {lost_chunks}")
    elif args.command == "combine-badlist":
        print(f"Combining badlist files in {args.path}...")
        print()
        combine_badlist(args.path, args.output_path)
    elif args.command == "print-errors":
        print(f"Analyzing repair errors in {args.path}...")
        print()
        total_errors = print_repair_errors(args.path)
        print()
        print(f"Total non-serialization errors: {total_errors}")


if __name__ == "__main__":
    main()
