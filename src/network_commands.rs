// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::DeploymentInventory;
use ant_service_management::{
    antctl_proto::{ant_ctl_client::AntCtlClient, GetStatusRequest, NodeServiceRestartRequest},
    rpc::{RpcActions, RpcClient},
    ServiceStatus,
};
use color_eyre::{
    eyre::{bail, eyre, Report},
    Result,
};
use futures::StreamExt;
use libp2p::PeerId;
use rand::Rng;
use std::{collections::BTreeSet, net::SocketAddr, time::Duration};
use tonic::{transport::Channel, Request};

const MAX_CONCURRENT_RPC_REQUESTS: usize = 10;

// Used internally for easier debugging
struct DaemonRpcClient {
    addr: SocketAddr,
    rpc: AntCtlClient<Channel>,
}

/// Perform fixed interval churn in the network by restarting nodes.
/// This causes concurrent_churns nodes per vm to churn at a time.
pub async fn perform_fixed_interval_network_churn(
    inventory: DeploymentInventory,
    sleep_interval: Duration,
    concurrent_churns: usize,
    retain_peer_id: bool,
    max_churn_cycles: usize,
) -> Result<()> {
    let safenodemand_endpoints = inventory
        .node_vms
        .iter()
        .filter_map(|node_vm| node_vm.safenodemand_endpoint)
        .map(|endpoint| (inventory.node_count(), endpoint))
        .chain(
            inventory
                .bootstrap_node_vms
                .iter()
                .filter_map(|node_vm| node_vm.safenodemand_endpoint)
                .map(|endpoint| (inventory.bootstrap_node_count(), endpoint)),
        )
        .collect::<BTreeSet<_>>();

    let max_churn_cycles = std::cmp::max(max_churn_cycles, 1);
    println!("===== Configurations =====");

    let mut n_cycles = 0;
    while n_cycles < max_churn_cycles {
        println!("===== Churn Cycle: {} =====", n_cycles + 1);
        // churn one VM at a time.
        for (node_count, daemon_endpoint) in safenodemand_endpoints.iter() {
            let max_concurrent_churns = std::cmp::min(concurrent_churns, *node_count);
            println!("===== Restarting nodes @ {} =====", daemon_endpoint.ip());
            let mut daemon_client = get_safenode_manager_rpc_client(*daemon_endpoint).await?;
            let nodes_to_churn = get_running_node_list(&mut daemon_client).await?;

            let mut concurrent_churns = 0;
            for (peer_id, node_service_number) in nodes_to_churn {
                // we don't call restart concurrently as the daemon does not handle concurrent node registry reads/writes.
                restart_node(peer_id, retain_peer_id, &mut daemon_client).await?;

                println!(
                    "safenode-{node_service_number:?}.service has been restarted. PeerId: {peer_id:?}"
                );

                concurrent_churns += 1;
                if concurrent_churns >= max_concurrent_churns {
                    println!("Sleeping {sleep_interval:?} before churning.");
                    tokio::time::sleep(sleep_interval).await;
                    concurrent_churns = 0;
                }
            }
        }

        n_cycles += 1;
    }
    Ok(())
}

pub async fn perform_random_interval_network_churn(
    inventory: DeploymentInventory,
    time_frame: Duration,
    churn_count: usize,
    retain_peer_id: bool,
    max_churn_cycles: usize,
) -> Result<()> {
    if churn_count == 0 {
        bail!("Churn count cannot be 0");
    }

    let safenodemand_endpoints = inventory
        .node_vms
        .iter()
        .filter_map(|node_vm| node_vm.safenodemand_endpoint)
        .chain(
            inventory
                .bootstrap_node_vms
                .iter()
                .filter_map(|node_vm| node_vm.safenodemand_endpoint),
        )
        .collect::<BTreeSet<_>>();

    let max_churn_cycles = std::cmp::max(max_churn_cycles, 1);
    let mut n_cycles = 0;

    // print the time to churn all these nodes
    {
        let total_num_nodes = inventory.peers().len() - 1;
        let n_timeframes_to_churn_all_nodes = if total_num_nodes % churn_count > 0 {
            total_num_nodes / churn_count + 1
        } else {
            total_num_nodes / churn_count
        };
        let total_time_per_cycle =
            Duration::from_secs(n_timeframes_to_churn_all_nodes as u64 * time_frame.as_secs());
        println!("===== Configurations =====");
        println!("Initializing churn of {churn_count} nodes every {time_frame:?}.");
        println!("This can take {total_time_per_cycle:?} for all {total_num_nodes:?} node. We perform {max_churn_cycles} such churn cycle(s)");
    }

    while n_cycles < max_churn_cycles {
        println!("===== Churn Cycle: {} =====", n_cycles + 1);
        // get all the updated peer list during each cycle (as it may change if retain_peer_id is false)
        let all_running_nodes =
            get_all_running_node_list(safenodemand_endpoints.iter().cloned()).await?;

        // deal batches of churn_count at a time.
        for (batch_idx, batch) in all_running_nodes.chunks(churn_count).enumerate() {
            println!("===== Time Frame: {} =====", batch_idx + 1);
            // for each batch, generate a random set of intervals
            let mut rng = rand::thread_rng();
            let mut intervals = Vec::new();
            // the final cut should be at the end because we would want to utilize the whole "time frame".
            intervals.push(time_frame.as_secs());

            // this would give us churn_count cuts within the timeframe.
            for _ in 0..churn_count - 1 {
                intervals.push(rng.gen_range(1..time_frame.as_secs()));
            }
            intervals.sort_unstable();
            println!("{intervals:?}");

            let mut previous_interval = 0;
            let mut previous_daemon_client: Option<(SocketAddr, _)> = None;
            for ((daemon_endpoint, peer_id, node_service_number), interval) in
                batch.iter().zip(intervals)
            {
                let sleep_time = Duration::from_secs(interval - previous_interval);

                // reuse previous_daemon_rpc if endpoints match, which most probably they will all_running_nodes is not
                // shuffled. This is to prevent excessive dialing.
                let mut daemon_client = match previous_daemon_client.take() {
                    Some((endpoint, client)) if endpoint == *daemon_endpoint => client,
                    _ => get_safenode_manager_rpc_client(*daemon_endpoint).await?,
                };

                restart_node(*peer_id, retain_peer_id, &mut daemon_client).await?;
                println!(
                    "safenode-{node_service_number:?}.service @ {daemon_endpoint:?} has been restarted. PeerId: {peer_id:?}"
                );
                println!("Sleeping for {sleep_time:?} before restarting the next node.");
                tokio::time::sleep(sleep_time).await;

                previous_daemon_client = Some((*daemon_endpoint, daemon_client));
                previous_interval = interval;
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
        .bootstrap_node_vms
        .iter()
        .flat_map(|node_vm| node_vm.rpc_endpoint.values())
        .chain(
            inventory
                .node_vms
                .iter()
                .flat_map(|node_vm| node_vm.rpc_endpoint.values()),
        );
    let mut stream = futures::stream::iter(node_endpoints)
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

// Return the list of all the running nodes form all the VMs. Along with their daemon endpoint
async fn get_all_running_node_list(
    all_daemon_endpoints: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<(SocketAddr, PeerId, u32)>> {
    let mut stream = futures::stream::iter(all_daemon_endpoints)
        .map(|endpoint| async move {
            let mut daemon_client = get_safenode_manager_rpc_client(endpoint).await?;
            let running_nodes = get_running_node_list(&mut daemon_client).await?;
            let running_nodes = running_nodes
                .into_iter()
                .map(|(peer_id, number)| (endpoint, peer_id, number))
                .collect::<Vec<_>>();

            Ok::<_, Report>(running_nodes)
        })
        .buffer_unordered(MAX_CONCURRENT_RPC_REQUESTS);

    let mut all_running_nodes = vec![];
    while let Some(result) = stream.next().await {
        all_running_nodes.extend(result?);
    }

    Ok(all_running_nodes)
}

// Return the list of the nodes that are currently running, along with their service number.
async fn get_running_node_list(daemon_client: &mut DaemonRpcClient) -> Result<Vec<(PeerId, u32)>> {
    let response = daemon_client
        .rpc
        .get_status(Request::new(GetStatusRequest {}))
        .await
        .map_err(|err| {
            eyre!(
                "Failed to get status from {:?} with err: {err:?}",
                daemon_client.addr
            )
        })?;

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
    daemon_client: &mut DaemonRpcClient,
) -> Result<()> {
    let _response = daemon_client
        .rpc
        .restart_node_service(Request::new(NodeServiceRestartRequest {
            peer_id: peer_id.to_bytes(),
            delay_millis: 0,
            retain_peer_id,
        }))
        .await
        .map_err(|err| {
            eyre!(
                "Failed to restart node service with {peer_id:?} at {:?} with err: {err:?}",
                daemon_client.addr
            )
        })?;

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
async fn get_safenode_manager_rpc_client(socket_addr: SocketAddr) -> Result<DaemonRpcClient> {
    // get the new PeerId for the current NodeIndex
    let endpoint = format!("https://{socket_addr}");
    let mut attempts = 0;
    loop {
        if let Ok(rpc_client) = AntCtlClient::connect(endpoint.clone()).await {
            let rpc_client = DaemonRpcClient {
                addr: socket_addr,
                rpc: rpc_client,
            };
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
