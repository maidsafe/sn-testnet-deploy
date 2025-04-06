// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use clap::Subcommand;
use color_eyre::{eyre::eyre, Result};
use sn_testnet_deploy::{inventory::DeploymentInventoryService, TestnetDeployBuilder};

#[derive(Subcommand, Debug)]
pub enum TelegrafCommands {
    /// Start the Telegraf service on all machines in the environment.
    Start {
        /// Provide a list of VM names to use as a custom inventory.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to start the telegraf services on. If not provided, the telegraf services on
        /// all the node VMs will be started. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<sn_testnet_deploy::NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, default_value_t = sn_testnet_deploy::CloudProvider::DigitalOcean, value_parser = super::parse_provider, verbatim_doc_comment)]
        provider: sn_testnet_deploy::CloudProvider,
    },
    /// Stop the Telegraf service on all machines in the environment.
    Stop {
        /// Provide a list of VM names to use as a custom inventory.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to stop the telegraf services on. If not provided, the telegraf services on
        /// all the node VMs will be stopped. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<sn_testnet_deploy::NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, default_value_t = sn_testnet_deploy::CloudProvider::DigitalOcean, value_parser = super::parse_provider, verbatim_doc_comment)]
        provider: sn_testnet_deploy::CloudProvider,
    },
    /// Upgrade the client Telegraf configuration on an environment.
    UpgradeClientConfig {
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, default_value_t = sn_testnet_deploy::CloudProvider::DigitalOcean, value_parser = super::parse_provider, verbatim_doc_comment)]
        provider: sn_testnet_deploy::CloudProvider,
    },
    /// Upgrade the node Telegraf configuration on an environment.
    UpgradeNodeConfig {
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, default_value_t = sn_testnet_deploy::CloudProvider::DigitalOcean, value_parser = super::parse_provider, verbatim_doc_comment)]
        provider: sn_testnet_deploy::CloudProvider,
    },
}

pub async fn handle_start_telegraf_command(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    name: String,
    node_type: Option<sn_testnet_deploy::NodeType>,
    provider: sn_testnet_deploy::CloudProvider,
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
        let custom_vms = super::get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.start_telegraf(node_type, custom_inventory)?;

    Ok(())
}

pub async fn handle_stop_telegraf_command(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    name: String,
    node_type: Option<sn_testnet_deploy::NodeType>,
    provider: sn_testnet_deploy::CloudProvider,
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
        let custom_vms = super::get_custom_inventory(&inventory, &custom_inventory)?;
        Some(custom_vms)
    } else {
        None
    };

    testnet_deployer.stop_telegraf(node_type, custom_inventory)?;

    Ok(())
}

pub async fn handle_upgrade_client_telegraf_config(
    forks: usize,
    name: String,
    provider: sn_testnet_deploy::CloudProvider,
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

    testnet_deployer.upgrade_client_telegraf(&name)?;

    Ok(())
}

pub async fn handle_upgrade_node_telegraf_config(
    forks: usize,
    name: String,
    provider: sn_testnet_deploy::CloudProvider,
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

    testnet_deployer.upgrade_node_telegraf(&name)?;

    Ok(())
}
