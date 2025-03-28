// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{get_binary_option, upload_options_to_s3, OptionsType};

use alloy::primitives::U256;
use color_eyre::{eyre::eyre, Help, Result};
use sn_testnet_deploy::{
    bootstrap::BootstrapOptions, calculate_size_per_attached_volume, deploy::DeployOptions,
    error::Error, inventory::DeploymentInventoryService, upscale::UpscaleOptions, BinaryOption,
    CloudProvider, EnvironmentType, EvmNetwork, LogFormat, TestnetDeployBuilder,
};
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub async fn handle_bootstrap(
    ansible_verbose: bool,
    antctl_version: Option<String>,
    antnode_features: Option<Vec<String>>,
    antnode_version: Option<String>,
    branch: Option<String>,
    chunk_size: Option<u64>,
    env_variables: Option<Vec<(String, String)>>,
    environment_type: EnvironmentType,
    evm_data_payments_address: Option<String>,
    evm_network_type: EvmNetwork,
    evm_payment_token_address: Option<String>,
    evm_rpc_url: Option<String>,
    forks: Option<usize>,
    full_cone_private_node_count: Option<u16>,
    full_cone_private_node_vm_count: Option<u16>,
    full_cone_private_node_volume_size: Option<u16>,
    interval: Duration,
    log_format: Option<LogFormat>,
    max_archived_log_files: u16,
    max_log_files: u16,
    name: String,
    network_contacts_url: Option<String>,
    network_id: u8,
    node_count: Option<u16>,
    node_vm_count: Option<u16>,
    node_vm_size: Option<String>,
    node_volume_size: Option<u16>,
    peer: Option<String>,
    provider: CloudProvider,
    repo_owner: Option<String>,
    rewards_address: String,
    symmetric_private_node_count: Option<u16>,
    symmetric_private_node_vm_count: Option<u16>,
    symmetric_private_node_volume_size: Option<u16>,
) -> Result<()> {
    if network_contacts_url.is_none() && peer.is_none() {
        return Err(eyre!(
            "Either bootstrap-peer or bootstrap-network-contacts-url must be provided"
        ));
    }

    if evm_network_type == EvmNetwork::Anvil {
        return Err(eyre!(
            "The anvil network type cannot be used for bootstrapping. 
            Use the custom network type, supplying the Anvil contract addresses and RPC URL
            from the previous network. They can be found in the network's inventory."
        ));
    }

    if evm_network_type == EvmNetwork::Custom
        && (evm_data_payments_address.is_none()
            || evm_payment_token_address.is_none()
            || evm_rpc_url.is_none())
    {
        return Err(eyre!(
            "When using a custom EVM network, you must supply evm-data-payments-address, evm-payment-token-address, and evm-rpc-url"
        ));
    }

    if evm_network_type != EvmNetwork::Custom && evm_rpc_url.is_some() {
        return Err(eyre!(
            "EVM RPC URL can only be set for a custom EVM network"
        ));
    }

    let binary_option = get_binary_option(
        branch,
        repo_owner,
        None,
        antnode_version,
        antctl_version,
        antnode_features,
    )
    .await?;

    let mut builder = TestnetDeployBuilder::default();
    builder
        .ansible_verbose_mode(ansible_verbose)
        .deployment_type(environment_type.clone())
        .environment_name(&name)
        .provider(provider);
    if let Some(forks) = forks {
        builder.ansible_forks(forks);
    }
    let testnet_deployer = builder.build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    inventory_service
        .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
        .await?;

    match testnet_deployer.init().await {
        Ok(_) => {}
        Err(e @ Error::LogsForPreviousTestnetExist(_)) => {
            return Err(eyre!(e)
                .wrap_err(format!(
                    "Logs already exist for a previous testnet with the \
                            name '{name}'"
                ))
                .suggestion(
                    "If you wish to keep them, retrieve the logs with the 'logs get' \
                        command, then remove them with 'logs rm'. If you don't need them, \
                        simply run 'logs rm'. Then you can proceed with deploying your \
                        new testnet.",
                ));
        }
        Err(e) => {
            return Err(eyre!(e));
        }
    }

    let node_count = node_count.unwrap_or(environment_type.get_default_node_count());
    let symmetric_private_node_count = symmetric_private_node_count
        .unwrap_or(environment_type.get_default_symmetric_private_node_count());
    let full_cone_private_node_count = full_cone_private_node_count
        .unwrap_or(environment_type.get_default_full_cone_private_node_count());

    testnet_deployer
        .bootstrap(&BootstrapOptions {
            binary_option,
            network_contacts_url,
            peer,
            environment_type: environment_type.clone(),
            node_env_variables: env_variables,
            evm_data_payments_address,
            evm_network: evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            full_cone_private_node_count,
            full_cone_private_node_vm_count,
            full_cone_private_node_volume_size: full_cone_private_node_volume_size.or_else(|| {
                Some(calculate_size_per_attached_volume(
                    full_cone_private_node_count,
                ))
            }),
            interval,
            log_format,
            name: name.clone(),
            network_id,
            node_count,
            node_vm_count,
            node_vm_size,
            node_volume_size: node_volume_size
                .or_else(|| Some(calculate_size_per_attached_volume(node_count))),
            max_archived_log_files,
            max_log_files,
            output_inventory_dir_path: inventory_service
                .working_directory_path
                .join("ansible")
                .join("inventory"),
            symmetric_private_node_vm_count,
            symmetric_private_node_count,
            symmetric_private_node_volume_size: symmetric_private_node_volume_size.or_else(|| {
                Some(calculate_size_per_attached_volume(
                    symmetric_private_node_count,
                ))
            }),
            rewards_address,
            chunk_size,
        })
        .await?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let new_inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;
    new_inventory.print_report(false)?;
    new_inventory.save()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_deploy(
    ansible_verbose: bool,
    ant_version: Option<String>,
    antctl_version: Option<String>,
    antnode_features: Option<Vec<String>>,
    antnode_version: Option<String>,
    branch: Option<String>,
    chunk_size: Option<u64>,
    client_env_variables: Option<Vec<(String, String)>>,
    client_vm_count: Option<u16>,
    client_vm_size: Option<String>,
    disable_telegraf: bool,
    enable_downloaders: bool,
    environment_type: crate::EnvironmentType,
    evm_data_payments_address: Option<String>,
    evm_network_type: EvmNetwork,
    evm_node_vm_size: Option<String>,
    evm_payment_token_address: Option<String>,
    evm_rpc_url: Option<String>,
    full_cone_nat_gateway_vm_size: Option<String>,
    full_cone_private_node_count: Option<u16>,
    full_cone_private_node_vm_count: Option<u16>,
    full_cone_private_node_volume_size: Option<u16>,
    forks: Option<usize>,
    funding_wallet_secret_key: Option<String>,
    genesis_node_volume_size: Option<u16>,
    initial_gas: Option<U256>,
    initial_tokens: Option<U256>,
    interval: std::time::Duration,
    log_format: Option<LogFormat>,
    max_archived_log_files: u16,
    max_log_files: u16,
    name: String,
    network_id: Option<u8>,
    network_contacts_file_name: Option<String>,
    node_count: Option<u16>,
    node_env_variables: Option<Vec<(String, String)>>,
    node_vm_count: Option<u16>,
    node_vm_size: Option<String>,
    node_volume_size: Option<u16>,
    peer_cache_node_count: Option<u16>,
    peer_cache_node_vm_count: Option<u16>,
    peer_cache_node_vm_size: Option<String>,
    peer_cache_node_volume_size: Option<u16>,
    symmetric_nat_gateway_vm_size: Option<String>,
    symmetric_private_node_count: Option<u16>,
    symmetric_private_node_vm_count: Option<u16>,
    symmetric_private_node_volume_size: Option<u16>,
    provider: crate::CloudProvider,
    public_rpc: bool,
    repo_owner: Option<String>,
    rewards_address: String,
    to_genesis: bool,
    uploaders_count: u16,
) -> Result<()> {
    if evm_network_type == EvmNetwork::Custom {
        if evm_data_payments_address.is_none() {
            return Err(eyre!(
                "Data payments address must be provided for custom EVM network"
            ));
        }
        if evm_payment_token_address.is_none() {
            return Err(eyre!(
                "Payment token address must be provided for custom EVM network"
            ));
        }
        if evm_rpc_url.is_none() {
            return Err(eyre!("RPC URL must be provided for custom EVM network"));
        }
    }

    if funding_wallet_secret_key.is_none() && evm_network_type != EvmNetwork::Anvil {
        return Err(eyre!(
            "Wallet secret key is required for Arbitrum or Sepolia networks"
        ));
    }

    let binary_option = get_binary_option(
        branch,
        repo_owner,
        ant_version,
        antnode_version,
        antctl_version,
        antnode_features,
    )
    .await?;

    let mut builder = TestnetDeployBuilder::default();
    builder
        .ansible_verbose_mode(ansible_verbose)
        .deployment_type(environment_type.clone())
        .environment_name(&name)
        .provider(provider);
    if let Some(forks) = forks {
        builder.ansible_forks(forks);
    }
    let testnet_deployer = builder.build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
        .await?;
    // let inventory = DeploymentInventory::empty(&name, binary_option.clone());

    match testnet_deployer.init().await {
        Ok(_) => {}
        Err(e @ sn_testnet_deploy::error::Error::LogsForPreviousTestnetExist(_)) => {
            return Err(eyre!(e)
                .wrap_err(format!(
                    "Logs already exist for a previous testnet with the \
                            name '{name}'"
                ))
                .suggestion(
                    "If you wish to keep them, retrieve the logs with the 'logs get' \
                        command, then remove them with 'logs rm'. If you don't need them, \
                        simply run 'logs rm'. Then you can proceed with deploying your \
                        new testnet.",
                ));
        }
        Err(e) => {
            return Err(eyre!(e));
        }
    }

    let peer_cache_node_count =
        peer_cache_node_count.unwrap_or(environment_type.get_default_peer_cache_node_count());
    let node_count = node_count.unwrap_or(environment_type.get_default_node_count());
    let symmetric_private_node_count = symmetric_private_node_count
        .unwrap_or(environment_type.get_default_symmetric_private_node_count());
    let full_cone_private_node_count = full_cone_private_node_count
        .unwrap_or(environment_type.get_default_full_cone_private_node_count());

    let deploy_options = DeployOptions {
        binary_option: binary_option.clone(),
        chunk_size,
        client_env_variables,
        client_vm_count,
        client_vm_size,
        current_inventory: inventory,
        enable_downloaders,
        enable_telegraf: !disable_telegraf,
        environment_type: environment_type.clone(),
        evm_data_payments_address,
        evm_network: evm_network_type,
        evm_node_vm_size,
        evm_payment_token_address,
        evm_rpc_url,
        full_cone_nat_gateway_vm_size,
        full_cone_private_node_count,
        full_cone_private_node_vm_count,
        full_cone_private_node_volume_size: full_cone_private_node_volume_size.or_else(|| {
            Some(calculate_size_per_attached_volume(
                full_cone_private_node_count,
            ))
        }),
        funding_wallet_secret_key,
        genesis_node_volume_size: genesis_node_volume_size
            .or_else(|| Some(calculate_size_per_attached_volume(1))),
        initial_gas,
        initial_tokens,
        interval,
        log_format,
        max_archived_log_files,
        max_log_files,
        name: name.clone(),
        network_id,
        node_count,
        node_env_variables,
        node_vm_count,
        node_vm_size,
        node_volume_size: node_volume_size
            .or_else(|| Some(calculate_size_per_attached_volume(node_count))),
        output_inventory_dir_path: inventory_service
            .working_directory_path
            .join("ansible")
            .join("inventory"),
        peer_cache_node_count,
        peer_cache_node_vm_count,
        peer_cache_node_vm_size,
        peer_cache_node_volume_size: peer_cache_node_volume_size
            .or_else(|| Some(calculate_size_per_attached_volume(peer_cache_node_count))),
        public_rpc,
        rewards_address,
        symmetric_nat_gateway_vm_size,
        symmetric_private_node_count,
        symmetric_private_node_vm_count,
        symmetric_private_node_volume_size: symmetric_private_node_volume_size.or_else(|| {
            Some(calculate_size_per_attached_volume(
                symmetric_private_node_count,
            ))
        }),
        uploaders_count,
    };

    if to_genesis {
        let (provision_options, _) = testnet_deployer.deploy_to_genesis(&deploy_options).await?;

        upload_options_to_s3(&name, &deploy_options, OptionsType::Deploy).await?;
        upload_options_to_s3(&name, &provision_options, OptionsType::Provision).await?;
    } else {
        testnet_deployer.deploy(&deploy_options).await?;
    }

    let max_retries = 3;
    let mut retries = 0;
    let inventory = loop {
        match inventory_service
            .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
            .await
        {
            Ok(inv) => break inv,
            Err(e) if retries < max_retries => {
                retries += 1;
                eprintln!("Failed to generate inventory on attempt {retries}: {:?}", e);
                eprintln!("Will retry up to {max_retries} times...");
            }
            Err(_) => {
                eprintln!("Failed to generate inventory after {max_retries} attempts");
                eprintln!("Please try running the `inventory` command or workflow separately");
                return Ok(());
            }
        }
    };

    inventory.print_report(false)?;
    inventory.save()?;

    inventory_service
        .upload_network_contacts(&inventory, network_contacts_file_name)
        .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_upscale(
    ansible_verbose: bool,
    ant_version: Option<String>,
    antctl_version: Option<String>,
    antnode_version: Option<String>,
    branch: Option<String>,
    desired_client_vm_count: Option<u16>,
    desired_node_count: Option<u16>,
    desired_full_cone_private_node_count: Option<u16>,
    desired_full_cone_private_node_vm_count: Option<u16>,
    desired_node_vm_count: Option<u16>,
    desired_peer_cache_node_count: Option<u16>,
    desired_peer_cache_node_vm_count: Option<u16>,
    desired_symmetric_private_node_count: Option<u16>,
    desired_symmetric_private_node_vm_count: Option<u16>,
    desired_uploaders_count: Option<u16>,
    enable_downloaders: bool,
    funding_wallet_secret_key: Option<String>,
    infra_only: bool,
    interval: Duration,
    max_archived_log_files: u16,
    max_log_files: u16,
    name: String,
    plan: bool,
    provider: CloudProvider,
    public_rpc: bool,
    repo_owner: Option<String>,
) -> Result<()> {
    if branch.is_some() && repo_owner.is_none() {
        return Err(eyre!(
            "The --repo-owner argument is required when --branch is used"
        ));
    }

    if branch.is_some()
        && (antnode_version.is_some() || antctl_version.is_some() || ant_version.is_some())
    {
        return Err(eyre!(
            "The version arguments cannot be used when --branch is specified"
        ));
    }

    if desired_client_vm_count.is_some() && ant_version.is_none() && branch.is_none() {
        return Err(eyre!(
            "The --ant-version or --branch argument is required when upscaling the Clients"
        ));
    }

    if (desired_client_vm_count.is_some() || desired_uploaders_count.is_some())
        && funding_wallet_secret_key.is_none()
    {
        return Err(eyre!(
            "The funding wallet secret key is required to upscale the Clients"
        ));
    }

    println!("Upscaling deployment...");
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_verbose_mode(ansible_verbose)
        .environment_name(&name)
        .provider(provider)
        .build()?;
    testnet_deployer.init().await?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    let mut inventory = inventory_service
        .generate_or_retrieve_inventory(&name, true, None)
        .await?;

    if branch.is_some() {
        println!("The upscale will use the binaries built in the original deployment");
        inventory.binary_option = BinaryOption::BuildFromSource {
            antnode_features: None,
            branch: branch.unwrap(),
            repo_owner: repo_owner.unwrap(),
        };
    } else if antnode_version.is_some() || antctl_version.is_some() {
        match &inventory.binary_option {
            BinaryOption::Versioned {
                ant_version: _,
                antnode_version: existing_antnode_version,
                antctl_version: existing_antctl_version,
            } => {
                let existing_antnode_version =
                    existing_antnode_version.as_ref().ok_or_else(|| {
                        eyre!("The existing deployment must have an antnode version to override it")
                    })?;
                let existing_antctl_version =
                    existing_antctl_version.as_ref().ok_or_else(|| {
                        eyre!("The existing deployment must have an antctl version to override it")
                    })?;

                let new_antnode_version = antnode_version
                    .map(|v| v.parse().expect("Invalid antnode version"))
                    .unwrap_or_else(|| existing_antnode_version.clone());
                let new_antctl_version = antctl_version
                    .map(|v| v.parse().expect("Invalid antctl version"))
                    .unwrap_or_else(|| existing_antctl_version.clone());

                println!("The upscale will use the following override binary versions:");
                println!("antnode: {}", new_antnode_version);
                println!("antctl: {}", new_antctl_version);

                inventory.binary_option = BinaryOption::Versioned {
                    ant_version: None,
                    antnode_version: Some(new_antnode_version),
                    antctl_version: Some(new_antctl_version),
                };
            }
            BinaryOption::BuildFromSource { .. } => {
                return Err(eyre!(
                    "Cannot override versions when the deployment uses BuildFromSource"
                ));
            }
        }
    }

    inventory.binary_option.print();

    testnet_deployer
        .upscale(&UpscaleOptions {
            ansible_verbose,
            ant_version,
            current_inventory: inventory,
            desired_client_vm_count,
            desired_node_count,
            desired_full_cone_private_node_count,
            desired_full_cone_private_node_vm_count,
            desired_node_vm_count,
            desired_peer_cache_node_count,
            desired_peer_cache_node_vm_count,
            desired_symmetric_private_node_count,
            desired_symmetric_private_node_vm_count,
            desired_uploaders_count,
            enable_downloaders,
            funding_wallet_secret_key,
            gas_amount: None,
            infra_only,
            interval,
            max_archived_log_files,
            max_log_files,
            plan,
            provision_only: false,
            public_rpc,
            token_amount: None,
        })
        .await?;

    if plan {
        return Ok(());
    }

    println!("Generating new inventory after upscale...");
    let max_retries = 3;
    let mut retries = 0;
    let inventory = loop {
        match inventory_service
            .generate_or_retrieve_inventory(&name, true, None)
            .await
        {
            Ok(inv) => break inv,
            Err(e) if retries < max_retries => {
                retries += 1;
                eprintln!("Failed to generate inventory on attempt {retries}: {:?}", e);
                eprintln!("Will retry up to {max_retries} times...");
            }
            Err(_) => {
                eprintln!("Failed to generate inventory after {max_retries} attempts");
                eprintln!("Please try running the `inventory` command or workflow separately");
                return Ok(());
            }
        }
    };

    inventory.print_report(false)?;
    inventory.save()?;

    Ok(())
}
