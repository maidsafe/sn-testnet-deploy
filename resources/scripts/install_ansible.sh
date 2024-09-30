#!/bin/bash

export DEBIAN_FRONTEND=noninteractive

max_retries=10
retry_delay=5

for i in $(seq 1 $max_retries); do
  echo "Update Apt index attempt $i of $max_retries..."
  apt-get update -y

  if [[ $? -eq 0 ]]; then
    echo "Apt index updated successfully."
    break
  else
    echo "Failed attempt $i. Retrying in $retry_delay seconds..."
    sleep $retry_delay
  fi
  
  if [[ $i -eq $max_retries ]]; then
    echo "Failed to update Apt index after $max_retries attempts."
    exit 1
  fi
done

for i in $(seq 1 $max_retries); do
  apt-get install python3-pip -y
  if [[ $? -eq 0 ]]; then
    echo "Apt index updated successfully."
    break
  else
    echo "Failed attempt $i. Retrying in $retry_delay seconds..."
    sleep $retry_delay
  fi
done

pip3 install ansible --prefix /usr
