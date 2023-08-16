// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

mod build_safe_network_binaries;
mod clean;
mod create_infra;
mod get_genesis_multiaddr;
mod init;
mod provision_faucet;
mod provision_genesis_node;
mod provision_remaining_nodes;
mod setup;

const RPC_CLIENT_BIN_NAME: &str = "safenode_rpc_client";
