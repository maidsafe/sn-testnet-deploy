#!/usr/bin/env bash
OUR_MACHINE_IP=1
OUR_GENESIS_PEER_ID=2

wget https://sn-node.s3.eu-west-2.amazonaws.com/the-test-data.zip
safe --peer $NODE1 wallet get-faucet ${OUR_MACHINE_IP}:8000
unzip the-test-data.zip
time safe --peer $NODE1 files upload test-data/ -c 10