// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{get_custom_inventory, get_version_from_option};

use ant_releases::ReleaseType;
use color_eyre::{eyre::eyre, Result};
use libp2p::multiaddr::Multiaddr;
use sn_testnet_deploy::{
    ansible::{
        extra_vars::ExtraVarsDocBuilder,
        inventory::{generate_custom_environment_inventory, AnsibleInventoryType},
        AnsiblePlaybook,
    },
    inventory::DeploymentInventoryService,
    CloudProvider, EvmNetwork, NodeType, TestnetDeployBuilder,
};
use std::{str::FromStr, time::Duration};

pub async fn handle_start_command(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    interval: Duration,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;

    // This is required in the case where the command runs in a remote environment, where
    // there won't be an existing inventory, which is required to retrieve the node
    // registry files used to determine the status.
    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.start(interval, node_type, custom_inventory)?;

    Ok(())
}

pub async fn handle_apply_delete_node_records_cron_command(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.apply_delete_node_records_cron(node_type, custom_inventory)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_reset_command(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;

    // This is required in the case where the command runs in a remote environment, where
    // there won't be an existing inventory, which is required to retrieve the node
    // registry files used to determine the status.
    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.reset(node_type, custom_inventory)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_stop_command(
    custom_inventory: Option<Vec<String>>,
    delay: Option<u64>,
    forks: usize,
    interval: Duration,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
    service_names: Option<Vec<String>>,
) -> Result<()> {
    // Use a large number of forks for retrieving the inventory from a large deployment.
    // Then if a smaller number of forks is specified, we will recreate the deployer
    // with the smaller fork value.
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(50)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.stop(interval, node_type, custom_inventory, delay, service_names)?;

    Ok(())
}

pub async fn handle_status_command(
    forks: usize,
    name: String,
    provider: CloudProvider,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;

    // This is required in the case where the command runs in a remote environment, where
    // there won't be an existing inventory, which is required to retrieve the node
    // registry files used to determine the status.
    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    testnet_deployer.status()?;
    Ok(())
}

pub async fn handle_update_peer_command(
    custom_inventory: Option<Vec<String>>,
    name: String,
    node_type: Option<NodeType>,
    peer: String,
    provider: CloudProvider,
) -> Result<()> {
    if let Err(e) = Multiaddr::from_str(&peer) {
        return Err(eyre!("Invalid peer multiaddr: {}", e));
    }

    let testnet_deployer = TestnetDeployBuilder::default()
        .environment_name(&name)
        .provider(provider)
        .build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("peer", &peer);

    let inventory_type = if let Some(custom_inventory) = custom_inventory {
        println!("Updating peers against a custom inventory");
        generate_custom_environment_inventory(
            &custom_inventory,
            &name,
            &testnet_deployer
                .ansible_provisioner
                .ansible_runner
                .working_directory_path
                .join("inventory"),
        )?;
        AnsibleInventoryType::Custom
    } else {
        let inventory_type = match node_type {
            Some(NodeType::FullConePrivateNode) => AnsibleInventoryType::FullConePrivateNodes,
            Some(NodeType::Genesis) => AnsibleInventoryType::Genesis,
            Some(NodeType::Generic) => AnsibleInventoryType::Nodes,
            Some(NodeType::PeerCache) => AnsibleInventoryType::PeerCacheNodes,
            Some(NodeType::SymmetricPrivateNode) => AnsibleInventoryType::SymmetricPrivateNodes,
            Some(NodeType::Upnp) => AnsibleInventoryType::Upnp,
            Some(NodeType::PortRestrictedConePrivateNode) => {
                AnsibleInventoryType::PortRestrictedConePrivateNodes
            }
            None => AnsibleInventoryType::Nodes,
        };
        println!("Updating peers against {inventory_type:?}");
        inventory_type
    };

    testnet_deployer
        .ansible_provisioner
        .ansible_runner
        .run_playbook(
            AnsiblePlaybook::UpdatePeer,
            inventory_type,
            Some(extra_vars.build()),
        )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_reset_to_n_nodes_command(
    custom_inventory: Option<Vec<String>>,
    evm_network_type: EvmNetwork,
    forks: usize,
    name: String,
    node_count: u16,
    node_type: Option<NodeType>,
    provider: CloudProvider,
    start_interval: Duration,
    stop_interval: Duration,
    version: Option<String>,
) -> Result<()> {
    // We will use 50 forks for the initial run to retrieve the inventory, then recreate the
    // deployer using the custom fork value.
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(50)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    if inventory.is_empty() {
        return Err(eyre!("The {name} environment does not exist"));
    }

    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    testnet_deployer.init().await?;

    let antnode_version = get_version_from_option(version, &ReleaseType::AntNode).await?;
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("environment_name", &name);
    extra_vars.add_variable("evm_network_type", &evm_network_type.to_string());
    extra_vars.add_variable("node_count", &node_count.to_string());
    extra_vars.add_variable("start_interval", &start_interval.as_millis().to_string());
    extra_vars.add_variable("stop_interval", &stop_interval.as_millis().to_string());
    extra_vars.add_variable("version", &antnode_version.to_string());

    let ansible_runner = &testnet_deployer.ansible_provisioner.ansible_runner;

    if let Some(custom_inventory) = custom_inventory {
        println!("Running the playbook with a custom inventory");
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        generate_custom_environment_inventory(
            &custom_vms,
            &name,
            &ansible_runner.working_directory_path.join("inventory"),
        )?;
        ansible_runner.run_playbook(
            AnsiblePlaybook::ResetToNNodes,
            AnsibleInventoryType::Custom,
            Some(extra_vars.build()),
        )?;
        return Ok(());
    }

    if let Some(node_type) = node_type {
        println!("Running the playbook for {node_type:?} nodes");
        ansible_runner.run_playbook(
            AnsiblePlaybook::ResetToNNodes,
            node_type.to_ansible_inventory_type(),
            Some(extra_vars.build()),
        )?;
        return Ok(());
    }

    println!("Running the playbook for all node types");
    for node_inv_type in AnsibleInventoryType::iter_node_type() {
        ansible_runner.run_playbook(
            AnsiblePlaybook::ResetToNNodes,
            node_inv_type,
            Some(extra_vars.build()),
        )?;
    }
    Ok(())
}
