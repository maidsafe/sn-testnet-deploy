// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::DeploymentInventory;
use color_eyre::{
    eyre::{bail, eyre, OptionExt},
    Result,
};
use futures::StreamExt;
use libp2p::PeerId;
use sn_service_management::{
    rpc::{RpcActions, RpcClient},
    safenode_manager_proto::{
        safe_node_manager_client::SafeNodeManagerClient, GetStatusRequest,
        NodeServiceRestartRequest,
    },
    ServiceStatus,
};
use std::{collections::BTreeSet, net::SocketAddr, time::Duration};
use tonic::{transport::Channel, Request};

/// Perform churn in the network by restarting nodes.
pub async fn perform_network_churns(
    inventory: DeploymentInventory,
    sleep_interval: Duration,
    concurrent_churns: usize,
    retain_peer_id: bool,
    max_churn_cycles: usize,
) -> Result<()> {
    // We should not restart the genesis node as it is used as the bootstrap peer for all other nodes.
    let genesis_ip = inventory
        .vm_list
        .iter()
        .find_map(|(name, addr)| {
            if name.contains("genesis") {
                Some(*addr)
            } else {
                None
            }
        })
        .ok_or_eyre("Could not get the genesis VM's addr")?;
    let safenodemand_endpoints = inventory
        .safenodemand_endpoints
        .values()
        .filter(|addr| addr.ip() != genesis_ip)
        .cloned()
        .collect::<BTreeSet<_>>();

    let nodes_per_vm = {
        let mut vms_to_ignore = 0;
        inventory.vm_list.iter().for_each(|(name, _addr)| {
            if name.contains("build") || name.contains("genesis") {
                vms_to_ignore += 1;
            }
        });
        // subtract 1 node for genesis. And ignore build & genesis node.
        (inventory.node_count as usize - 1) / (inventory.vm_list.len() - vms_to_ignore)
    };

    let max_concurrent_churns = std::cmp::min(concurrent_churns, nodes_per_vm);
    let mut n_cycles = 0;
    while n_cycles < max_churn_cycles {
        println!("==== CHURN CYCLE {} ====", n_cycles + 1);
        // churn one VM at a time.
        for daemon_endpoint in safenodemand_endpoints.iter() {
            println!("==== Restarting nodes @ {} ====", daemon_endpoint.ip());
            let mut daemon_client = get_safenode_manager_rpc_client(*daemon_endpoint).await?;
            let nodes_to_churn = get_running_node_list(&mut daemon_client).await?;

            let mut concurrent_churns = 0;
            for (peer_id, node_number) in nodes_to_churn {
                // we don't call restart concurrently as the daemon does not handle concurrent node registry reads/writes.
                restart_node(peer_id, retain_peer_id, &mut daemon_client).await?;

                println!(
                    "safenode-{node_number:?}.service has been restarted. PeerId: {peer_id:?}"
                );

                concurrent_churns += 1;
                if concurrent_churns >= max_concurrent_churns {
                    println!("Sleeping {:?} before churning.", { sleep_interval });
                    tokio::time::sleep(sleep_interval).await;
                    concurrent_churns = 0;
                }
            }
        }

        n_cycles += 1;
    }
    Ok(())
}

/// Update the log levels of a running
pub async fn update_node_log_levels(
    inventory: DeploymentInventory,
    log_level: String,
    concurrent_updates: usize,
) -> Result<()> {
    let node_endpoints = inventory
        .rpc_endpoints
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut stream = futures::stream::iter(node_endpoints.iter())
        .map(|endpoint| update_log_level(*endpoint, log_level.clone()))
        .buffer_unordered(concurrent_updates);

    let mut failed_list = vec![];
    let mut last_error = Ok(());
    let mut success_counter = 0;
    while let Some((endpoint, result)) = stream.next().await {
        match result {
            Ok(_) => success_counter += 1,
            Err(err) => {
                failed_list.push(endpoint);
                last_error = Err(err);
            }
        }
    }

    println!("==== Update Log Levels Summary ====");
    println!("Successfully updated: {success_counter} nodes");
    if !failed_list.is_empty() {
        println!("Failed to update: {} nodes", failed_list.len());
        println!("Last error: {last_error:?}")
    }
    Ok(())
}

// ==== Private helpers ====

// Return the list of the nodes that are currently running, along with their service number.
async fn get_running_node_list(
    daemon_client: &mut SafeNodeManagerClient<Channel>,
) -> Result<Vec<(PeerId, u32)>> {
    let response = daemon_client
        .get_status(Request::new(GetStatusRequest {}))
        .await?;

    let peers = response
        .get_ref()
        .nodes
        .iter()
        .filter_map(|node| {
            if node.status == ServiceStatus::Running as i32 {
                let peer_id = match &node.peer_id {
                    Some(peer_id) => peer_id,
                    None => return Some(Err(eyre!("PeerId has not been set"))),
                };
                match PeerId::from_bytes(peer_id) {
                    Ok(peer_id) => Some(Ok((peer_id, node.number))),
                    Err(err) => Some(Err(err.into())),
                }
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(peers)
}

// Restart a remote safenode service by sending a RPC to the safenode manager daemon.
async fn restart_node(
    peer_id: PeerId,
    retain_peer_id: bool,
    daemon_client: &mut SafeNodeManagerClient<Channel>,
) -> Result<()> {
    let _response = daemon_client
        .restart_node_service(Request::new(NodeServiceRestartRequest {
            peer_id: peer_id.to_bytes(),
            delay_millis: 0,
            retain_peer_id,
        }))
        .await?;

    Ok(())
}

async fn update_log_level(endpoint: SocketAddr, log_levels: String) -> (SocketAddr, Result<()>) {
    let client = RpcClient::from_socket_addr(endpoint);
    let res = client
        .update_log_level(log_levels)
        .await
        .map_err(|err| eyre!("{err:?}"));
    (endpoint, res)
}

// Connect to a RPC socket addr with retry
async fn get_safenode_manager_rpc_client(
    socket_addr: SocketAddr,
) -> Result<SafeNodeManagerClient<tonic::transport::Channel>> {
    // get the new PeerId for the current NodeIndex
    let endpoint = format!("https://{socket_addr}");
    let mut attempts = 0;
    loop {
        if let Ok(rpc_client) = SafeNodeManagerClient::connect(endpoint.clone()).await {
            break Ok(rpc_client);
        }
        attempts += 1;
        println!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        tokio::time::sleep(Duration::from_secs(1)).await;
        if attempts >= 10 {
            bail!("Failed to connect to {endpoint:?} even after 10 retries");
        }
    }
}
