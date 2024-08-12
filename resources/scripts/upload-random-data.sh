#!/usr/bin/env bash

# Target rate of 1.5mb/s

# Get the input argument
CONTACT_PEER="${1:-}"

# Prepare contact peer argument
CONTACT_PEER_ARG=""
if [ -n "$CONTACT_PEER" ]; then
    CONTACT_PEER_ARG="--peer $CONTACT_PEER"
fi

# error out if safe is not installed
if ! command -v safe &> /dev/null; then
    echo "Error: 'safe' not found in PATH."
    echo "Error: 'safe' not found in PATH." >> uploader.log
    exit 1
fi

# Only the bootstrap peer should be provided here.
if [ -z "$CONTACT_PEER" ]; then
    echo "No contact peer provided. Please provide the bootstrap peer."
    echo "No contact peer provided. Please provide the bootstrap peer." >> uploader.log
    exit 1
fi

# Example usage
total_files=10000  # Total number of files to generate and upload

write_metrics_on_success() {
    local time=$1
    local file_size=$2
    metrics_header
    number_of_chunks=$(echo "$stdout" | rg -o 'Among [0-9]+' | rg -o '[0-9]+')
    store_cost=$(echo "$stdout" | rg -o 'Made payment of NanoTokens\([0-9]+' | rg -o '[0-9]+' | head -n 1)
    royalty_fees=$(echo "$stdout" | rg -o 'Made payment of NanoTokens\([0-9]+' | rg -o '[0-9]+' | tail -n 1)

    echo "$time,$file_size_kb,$number_of_chunks,$store_cost,$royalty_fees" >> "./uploader_metrics.csv"
}

write_metrics_on_failure() {
    local time=$1
    local file_size_kb=$2
    metrics_header
    echo "$time,$file_size_kb" >> "./uploader_metrics.csv"
}

metrics_header() {
      if [ ! -f "./uploader_metrics.csv" ]; then
        echo "Total Time(s),File Size (KB),Number of Chunks,Store Cost (NanoTokens),Royalty Fees (NanoTokens)" > "./uploader_metrics.csv"
    fi
}

# Function to generate a 10MB file of random data
generate_random_data_file_and_upload() {
  tmpfile=$(mktemp)
  dd if=/dev/urandom of="$tmpfile" bs=15M count=1 iflag=fullblock &> /dev/null

  echo "Generated random data file at $tmpfile" >> "./uploader.log"
  file_size_kb=$(du -k "$tmpfile" | cut -f1)

  # Upload the random data file using SAFE CLI
  now=$(date +"%s")
  stdout=$(safe $CONTACT_PEER_ARG files upload "$tmpfile")
  echo "$stdout" >> "./uploader.log"

  if [ $? -eq 0 ]; then
    echo "Successfully uploaded $tmpfile using SAFE CLI" >> "./uploader.log"
    elapsed=$(($(date +"%s") - $now))
    write_metrics_on_success $elapsed $file_size_kb $stdout
  else
    echo "Failed to upload $tmpfile using SAFE CLI" >> "./uploader.log"
    elapsed=$(($(date +"%s") - $now))
    write_metrics_on_failure $elapsed $file_size_kb
  fi

  # Remove the temporary file
  rm "$tmpfile"

  # Log and sleep for 10 seconds
  echo "Sleeping for 10 seconds..." >> "./uploader.log"
  sleep 10
}

# Loop to generate and upload random data files
for i in $(seq 1 $total_files); do
  echo "$(date +"%A, %B %d, %Y %H:%M:%S")" >> "./uploader.log"
  echo "Generating and uploading file $i of $total_files..." >> "./uploader.log"
  generate_random_data_file_and_upload

  echo "$(safe $CONTACT_PEER_ARG wallet balance)" >> "./uploader.log"
done