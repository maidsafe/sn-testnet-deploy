#!/bin/bash

max_retries=10
retry_delay=5

for i in $(seq 1 $max_retries); do
  echo "Attempt $i of $max_retries..."
  
  apt-add-repository ppa:ansible/ansible -y && \
  apt update -y && \
  apt install ansible -y && \
  echo "Ansible installed successfully!" && exit 0
  
  echo "Failed attempt $i. Retrying in $retry_delay seconds..."
  sleep $retry_delay
done

echo "Failed to install Ansible after $max_retries attempts."
exit 1
