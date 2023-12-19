#!/bin/bash

NODE_DATA_DIR_PATH=~/.local/share/safe/node
LOGFILE=$NODE_DATA_DIR_PATH/resource-usage.log

exec > >(tee -a $LOGFILE) 2>&1

while true; do
  echo "------------------------------------------------------"
  echo "Report for $(date)"
  echo "------------------------------------------------------"
  echo "Checking $(hostname) on $(hostname -I | awk '{print $1}')"
  printf "%-52s %-8s %-10s %-10s %-20s %-10s %-10s %-10s\n" \
    "Node                                                " \
    "PID" \
    "Memory (MB)" \
    "CPU (%)" \
    "Record Count" \
    "Connections" \
    "earned" \
    "store_cost"
  running_process_count=0
  for folder in $NODE_DATA_DIR_PATH/*; do
    if [ ! -d "$folder" ]; then continue; fi
    peer_id=$(basename "$folder")
    pid=$(cat "$folder/safenode.pid")
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
    count=$(find "$folder/record_store" -name '*' -not -name '*.pid' -type f | wc -l)
    con_count=$(ss -tunpa | grep ESTAB | grep =$pid -c)
    earned=$(
      rg 'new wallet balance is [^,]*' $folder/logs -o --no-line-number --no-filename |
      awk '{split($0, arr, " "); print arr[5]}' |
      sort -n |
      tail -n 1
    )
    store_cost=$(
      rg 'Cost is now [^ ]*' $folder/logs -o --no-line-number --no-filename |
      awk '{split($0, arr, " "); print arr[4]}' |
      sort -n |
      tail -n 1
    )
    printf "%-52s %-8s %-10s %-10s %-20s %-10s %-10s %-10s\n" \
      "$peer_id" \
      "$pid" \
      "$(awk "BEGIN {print $rss/1024}")" \
      "$cpu" \
      "$count" \
      "$con_count" \
      "$earned" \
      "$store_cost"
    running_process_count=$((running_process_count + 1))
  done
  echo "Total node processes: $running_process_count"
  total_connections=$(ss -tunpa | grep ESTAB | grep safenode -c)
  echo "Total live connections: $total_connections"

  # sleep 15 minutes before running again
  sleep 900
done
