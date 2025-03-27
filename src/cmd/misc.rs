// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use color_eyre::{
    eyre::{eyre, Result},
    Help,
};
use sn_testnet_deploy::{
    ansible::{extra_vars::ExtraVarsDocBuilder, inventory::AnsibleInventoryType, AnsiblePlaybook},
    get_environment_details,
    infra::InfraRunOptions,
    inventory::{get_data_directory, DeploymentInventory, DeploymentInventoryService},
    notify_slack, TestnetDeployBuilder,
};

#[allow(clippy::too_many_arguments)]
pub async fn handle_extend_volume_size(
    ansible_verbose: bool,
    genesis_node_volume_size: Option<u16>,
    full_cone_private_node_volume_size: Option<u16>,
    node_volume_size: Option<u16>,
    name: String,
    peer_cache_node_volume_size: Option<u16>,
    provider: sn_testnet_deploy::CloudProvider,
    symmetric_private_node_volume_size: Option<u16>,
) -> Result<()> {
    if peer_cache_node_volume_size.is_none()
        && genesis_node_volume_size.is_none()
        && node_volume_size.is_none()
        && symmetric_private_node_volume_size.is_none()
        && full_cone_private_node_volume_size.is_none()
    {
        return Err(eyre!("At least one volume size must be provided"));
    }

    println!("Extending attached volume size...");
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_verbose_mode(ansible_verbose)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    testnet_deployer.init().await?;

    let environemt_details =
        get_environment_details(&name, &testnet_deployer.s3_repository).await?;

    let mut infra_run_options = InfraRunOptions::generate_existing(
        &name,
        &testnet_deployer.terraform_runner,
        &environemt_details,
    )
    .await?;
    println!("Obtained infra run options from previous deployment {infra_run_options:?}");
    let mut node_types = Vec::new();

    if peer_cache_node_volume_size.is_some() {
        infra_run_options.peer_cache_node_volume_size = peer_cache_node_volume_size;
        node_types.push(AnsibleInventoryType::PeerCacheNodes);
    }
    if genesis_node_volume_size.is_some() {
        infra_run_options.genesis_node_volume_size = genesis_node_volume_size;
        node_types.push(AnsibleInventoryType::Genesis);
    }
    if node_volume_size.is_some() {
        infra_run_options.node_volume_size = node_volume_size;
        node_types.push(AnsibleInventoryType::Nodes);
    }
    if symmetric_private_node_volume_size.is_some() {
        infra_run_options.symmetric_private_node_volume_size = symmetric_private_node_volume_size;
        node_types.push(AnsibleInventoryType::SymmetricPrivateNodes);
    }
    if full_cone_private_node_volume_size.is_some() {
        infra_run_options.full_cone_private_node_volume_size = full_cone_private_node_volume_size;
        node_types.push(AnsibleInventoryType::FullConePrivateNodes);
    }

    println!("Running infra update with new volume sizes: {infra_run_options:?}");
    testnet_deployer
        .create_or_update_infra(&infra_run_options)
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

    for node_type in node_types {
        println!("Extending volume size for {node_type} nodes...");
        testnet_deployer
            .ansible_provisioner
            .ansible_runner
            .run_playbook(AnsiblePlaybook::ExtendVolumeSize, node_type, None)?;
    }

    Ok(())
}

pub async fn handle_inventory(
    force_regeneration: bool,
    full: bool,
    name: String,
    network_contacts_file_name: Option<String>,
    peer_cache: bool,
    provider: sn_testnet_deploy::CloudProvider,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
        .environment_name(&name)
        .provider(provider)
        .build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, force_regeneration, None)
        .await?;

    if peer_cache {
        inventory.print_peer_cache_webserver();
    } else {
        inventory.print_report(full)?;
    }

    inventory.save()?;

    inventory_service
        .upload_network_contacts(&inventory, network_contacts_file_name)
        .await?;

    Ok(())
}

pub async fn handle_notify(name: String) -> Result<()> {
    let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
    if !inventory_path.exists() {
        return Err(eyre!("There is no inventory for the {name} testnet")
            .suggestion("Please run the inventory command to generate it"));
    }

    let inventory = DeploymentInventory::read(&inventory_path)?;
    notify_slack(inventory).await?;
    Ok(())
}

pub async fn handle_configure_swapfile(
    name: String,
    provider: sn_testnet_deploy::CloudProvider,
    peer_cache: bool,
    size: u16,
) -> Result<()> {
    let testnet_deployer = TestnetDeployBuilder::default()
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

    let ansible_runner = testnet_deployer.ansible_provisioner.ansible_runner;
    ansible_runner.run_playbook(
        AnsiblePlaybook::ConfigureSwapfile,
        AnsibleInventoryType::Nodes,
        Some(build_swapfile_extra_vars_doc(size)?),
    )?;

    if peer_cache {
        ansible_runner.run_playbook(
            AnsiblePlaybook::ConfigureSwapfile,
            AnsibleInventoryType::PeerCacheNodes,
            Some(build_swapfile_extra_vars_doc(size)?),
        )?;
    }

    Ok(())
}

fn build_swapfile_extra_vars_doc(size: u16) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("swapfile_size", &format!("{size}G"));
    Ok(extra_vars.build())
}
