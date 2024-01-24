#!/bin/bash

NODE_ROOT_DIR_PATH="/var/safenode-manager/services"
NODE_LOG_DIR_PATH="/var/log/safenode"
LOGFILE=/var/log/safenode/resource-usage.log

exec > >(tee -a $LOGFILE) 2>&1

while true; do
  echo "------------------------------------------------------"
  echo "Report for $(date)"
  echo "------------------------------------------------------"
  echo "Checking $(hostname) on $(hostname -I | awk '{print $1}')"
  printf "%-52s %-8s %-10s %-10s %-16s %-15s %-10s %-12s %-10s\n" \
    "Node                                                " \
    "PID" \
    "Memory (MB)" \
    "CPU (%)" \
    "Record Count" \
    "Connections" \
    "Earned" \
    "Store Cost" \
    "RT Nodes"
  running_process_count=0
  for root_dir in "$NODE_ROOT_DIR_PATH"/*; do
    if [ ! -d "$root_dir" ]; then continue; fi
    safe_node_instance=$(basename "$root_dir")
    node_log_path="${NODE_LOG_DIR_PATH}/${safe_node_instance}"
    
    peer_id=$(
      rg 'Self PeerID' "${node_log_path}" --no-line-number --no-filename |
      grep -o '12D3Koo[^ ]*'
    )
    pid=$(cat "$root_dir/safenode.pid")

    if [ -z "$pid" ]; then
      echo "No PID found for $peer_id"
      continue
    fi
    if [ ! -d "/proc/$pid" ]; then
      echo "PID $pid for $peer_id is not currently running"
      continue
    fi
    rss=$(ps -p $pid -o rss=)
    cpu=$(top -b -n1 -p $pid | awk 'NR>7 {print $9}')
    count=$(find "$root_dir/record_store" -name '*' -not -name '*.pid' -type f | wc -l)
    con_count=$(ss -tunpa | grep ESTAB | grep =$pid -c)
    earned=$(
      rg 'new wallet balance is [^,]*' $node_log_path -o --no-line-number --no-filename |
      awk '{split($0, arr, " "); print arr[5]}' |
      sort -n |
      tail -n 1
    )
    store_cost=$(
      rg 'Cost is now [^ ]*' $node_log_path -o --no-line-number --no-filename |
      awk '{split($0, arr, " "); print arr[4]}' |
      sort -n |
      tail -n 1
    )
    rt_nodes=$(
      rg 'kbuckets [^,]*' $node_log_path -o --no-line-number --no-filename |
      awk '{split($0, arr, " "); print arr[2]}' |
      sort -n |
      tail -n 1
    )
    printf "%-52s %-8s %-11s %-13s %-16s %-12s %-13s %-12s %-10s\n" \
      "$peer_id" \
      "$pid" \
      "$(awk "BEGIN {print $rss/1024}")" \
      "$cpu" \
      "$count" \
      "$con_count" \
      "$earned" \
      "$store_cost" \
      "$rt_nodes"
    running_process_count=$((running_process_count + 1))
  done
  echo "Total node processes: $running_process_count"
  total_connections=$(ss -tunpa | grep ESTAB | grep safenode -c)
  echo "Total live connections: $total_connections"

  # sleep 15 minutes before running again
  sleep 900
done