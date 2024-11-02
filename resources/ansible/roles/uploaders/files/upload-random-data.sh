#!/usr/bin/env bash

# Target rate of 1.5mb/s

CONTACT_PEER="${1:-}"

CONTACT_PEER_ARG=""
if [ -n "$CONTACT_PEER" ]; then
  CONTACT_PEER_ARG="--peer $CONTACT_PEER"
fi

if ! command -v autonomi &> /dev/null; then
  echo "Error: 'autonomi' not found in PATH."
  exit 1
fi

if [ -z "$CONTACT_PEER" ]; then
  echo "No contact peer provided. Please provide the bootstrap peer."
  exit 1
fi

total_files=10000

write_metrics_on_success() {
  local time=$1
  local file_size_kb=$2
  metrics_header
  number_of_chunks=$(echo "$stdout" | rg -o 'Number of chunks uploaded: [0-9]+' | rg -o '[0-9]+')
  store_cost=$(echo "$stdout" | rg -o 'Total cost: [0-9]+' | rg -o '[0-9]+' | head -n 1)

  echo "$time,$file_size_kb,$number_of_chunks,$store_cost" >> "./uploader_metrics.csv"
}

write_metrics_on_failure() {
  local time=$1
  local file_size_kb=$2
  metrics_header
  echo "$time,$file_size_kb" >> "./uploader_metrics.csv"
}

metrics_header() {
  if [ ! -f "./uploader_metrics.csv" ]; then
    echo "Total Time(s),File Size (KB),Number of Chunks,Store Cost (AttoTokens)" > "./uploader_metrics.csv"
  fi
}

prune_chunk_artifacts() {
  echo "Pruning chunk artifacts more than 60 minutes old..."
  rm -f old_chunk_artifacts.txt

  start_time=$(date +%s)
  find ~/.local/share/safe/client/chunk_artifacts -type d -mmin +60 > old_chunk_artifacts.txt
  artifact_count=$(wc -l < old_chunk_artifacts.txt)
  xargs -a old_chunk_artifacts.txt rm -rf
  end_time=$(date +%s)

  elapsed_time=$((end_time - start_time))
  echo "Removed $artifact_count old chunk artifacts in $elapsed_time seconds"
}

# Generate a 10MB file of random data and log its reference
generate_random_data_file_and_upload() {
  tmpfile=$(mktemp)
  dd if=/dev/urandom of="$tmpfile" bs=15M count=1 iflag=fullblock &> /dev/null

  echo "Generated random data file at $tmpfile"
  file_size_kb=$(du -k "$tmpfile" | cut -f1)

  now=$(date +"%s")
  stdout=$(autonomi $CONTACT_PEER_ARG file upload "$tmpfile" 2>&1)
  echo "$stdout"

  if [ $? -eq 0 ]; then
    echo "Successfully uploaded $tmpfile using SAFE CLI"

    file_ref=$(echo "$stdout" | grep -oP 'Uploaded ".*" to address \K\S+')
    if [ -z "$file_ref" ]; then
      echo "Error: Unable to extract file reference."
    else
      echo "$file_ref" >> "./uploaded_files.log"
    fi

    elapsed=$(($(date +"%s") - $now))
    write_metrics_on_success $elapsed $file_size_kb
  else
    echo "Failed to upload $tmpfile using SAFE CLI"
    elapsed=$(($(date +"%s") - $now))
    write_metrics_on_failure $elapsed $file_size_kb
  fi

  rm "$tmpfile"
}

for i in $(seq 1 $total_files); do
  echo "$(date +"%A, %B %d, %Y %H:%M:%S")"
  echo "Generating and uploading file $i of $total_files..."
  generate_random_data_file_and_upload
  # prune_chunk_artifacts
  # TODO: re-enable when the new CLI has a `wallet balance` command
  # echo "$(autonomi $CONTACT_PEER_ARG wallet balance)"
done
