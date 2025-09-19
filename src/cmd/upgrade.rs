// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::get_custom_inventory;
use crate::{DeploymentInventoryService, TestnetDeployBuilder};
use color_eyre::{eyre::eyre, Result};
use sn_testnet_deploy::{CloudProvider, NodeType, UpgradeOptions};
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub async fn handle_upgrade_command(
    ansible_verbose: bool,
    branch: Option<String>,
    custom_inventory: Option<Vec<String>>,
    disable_status: bool,
    env_variables: Option<Vec<(String, String)>>,
    force: bool,
    forks: usize,
    interval: Duration,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
    pre_upgrade_delay: Option<u64>,
    repo_owner: Option<String>,
    version: Option<String>,
) -> Result<()> {
    // The upgrade intentionally uses a small value for `forks`, but this is far too slow
    // for retrieving the inventory from a large deployment. Therefore, we will use 50
    // forks for the initial run to retrieve the inventory, then recreate the deployer
    // using the smaller fork value.
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

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .ansible_verbose_mode(ansible_verbose)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    testnet_deployer.upgrade(UpgradeOptions {
        ansible_verbose,
        branch,
        custom_inventory,
        env_variables,
        force,
        forks,
        interval,
        name: name.clone(),
        node_type,
        provider,
        pre_upgrade_delay,
        repo_owner,
        version,
    })?;

    if !disable_status {
        // Recreate the deployer with an increased number of forks for retrieving the status.
        let testnet_deployer = TestnetDeployBuilder::default()
            .ansible_forks(50)
            .environment_name(&name)
            .provider(provider)
            .build()?;
        testnet_deployer.status().await?;
    }

    Ok(())
}

pub async fn handle_upgrade_antctl_command(
    custom_inventory: Option<Vec<String>>,
    name: String,
    node_type: Option<NodeType>,
    provider: CloudProvider,
    version: String,
) -> Result<()> {
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

    let custom_inventory = if let Some(custom_inventory) = custom_inventory {
        let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.upgrade_antctl(version.parse()?, node_type, custom_inventory)?;
    Ok(())
}
