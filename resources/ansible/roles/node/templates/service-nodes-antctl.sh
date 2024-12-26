#!/bin/bash

ADD_ARGUMENTS=""

ADD_ARGUMENTS+=" --data-dir-path=/mnt/antnode-storage/data"
ADD_ARGUMENTS+=" --log-dir-path=/mnt/antnode-storage/log"
ADD_ARGUMENTS+=" --count=${NODES_TO_ADD}"

if [ -z "${GENESIS_MULTIADDR}"]; then
    ADD_ARGUMENTS+=" --peer=${GENESIS_MULTIADDR}"
fi

if [ -z "${NETWORK_CONTACTS_URL}"]; then
    ADD_ARGUMENTS+=" --network-contacts-url=${NETWORK_CONTACTS_URL}"
fi

if [ -z "${NETWORK_ID}"]; then
    ADD_ARGUMENTS+=" --network-id=${NETWORK_ID}"
fi

ADD_ARGUMENTS+=" --testnet"
ADD_ARGUMENTS+=" --rpc-address=${NODE_RPC_IP}"
ADD_ARGUMENTS+=" --rewards-address=${REWARDS_ADDRESS}"
ADD_ARGUMENTS+=" --max-archived-log-files=${MAX_ARCHIVED_LOG_FILES}"
ADD_ARGUMENTS+=" --max-log-files=${MAX_LOG_FILES}"

if [ -z "${MAKE_VM_PRIVATE}"]; then
    ADD_ARGUMENTS+=" --node-ip=${PRIVATE_IP_ETH1}"
    ADD_ARGUMENTS+=" --home-network"
fi

RPC_PORT="--rpc-port"
if [ ! -z "${USE_PORT_RANGE}"]; then
    RPC_PORT+=" ${RPC_PORT}"
fi

if [ -z "${USE_PORT_RANGE}"]; then
    RPC_PORT+=" ${RPC_START_PORT}"
    RPC_PORT+=" -"
    RPC_PORT+=" ${RPC_END_PORT}"
fi

ADD_ARGUMENTS+=" \`${RPC_PORT}\`"
if [ ! -z "${USE_PORT_RANGE}"]; then
    ADD_ARGUMENTS+=" --metrics-port=${METRICS_PORT}"
fi

if [ -z "${USE_PORT_RANGE}"]; then
    ADD_ARGUMENTS+=" --metrics-port=\`${METRICS_START_PORT} - ${METRICS_END_PORT}\`"
fi

if [ -z "${LOG_FORMAT}"]; then
    ADD_ARGUMENTS+=" --log-format=${LOG_FORMAT}"
fi

if [ -z "${ENV_VARIABLES}" ]; then
    ADD_ARGUMENTS+=" --env=${ENV_VARIABLES}"
fi

if [ -z "${VERSION}"]; then
    ADD_ARGUMENTS+=" --version=${VERSION}"
fi

if [ -z "${EVM_NETWORK_TYPE}"]; then
    ADD_ARGUMENTS+=" ${EVM_NETWORK_TYPE}"
fi

if [[ "${EVM_NETWORK_TYPE}" == "evm-custom" ]]; then
    ADD_ARGUMENTS+=" --rpc-url=${EVM_RPC_URL}"
    ADD_ARGUMENTS+=" --payment-token-address=${EVM_PAYMENT_TOKEN_ADDRESS}"
    ADD_ARGUMENTS+=" --data-payments-address=${EVM_DATA_PAYMENTS_ADDRESS}"
fi


while true; do
    # Your code here
    sleep 30
    antctl reset
    antctl -v add ${ADD_ARGUMENTS}
    antctl -v start --interval ${INTERVAL}
    date >> ~/test_date.txt
    echo ${ADD_ARGUMENTS} >> ~/test_date.txt
done