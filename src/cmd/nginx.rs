// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::get_custom_inventory;
use clap::Subcommand;
use color_eyre::{eyre::eyre, Result};
use sn_testnet_deploy::{inventory::DeploymentInventoryService, TestnetDeployBuilder};

#[derive(Subcommand, Debug)]
pub enum NginxCommands {
    /// Upgrade the nginx configuration on an environment.
    UpgradeConfig {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will upgrade nginx on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
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

pub async fn handle_upgrade_nginx_config(
    custom_inventory: Option<Vec<String>>,
    forks: usize,
    name: String,
    provider: sn_testnet_deploy::CloudProvider,
) -> Result<()> {
    // Use 50 forks for inventory retrieval for better performance
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

    // Recreate deployer with the specified forks for the nginx upgrade
    let testnet_deployer = TestnetDeployBuilder::default()
        .ansible_forks(forks)
        .environment_name(&name)
        .provider(provider)
        .build()?;

    let provisioner = testnet_deployer.ansible_provisioner;
    provisioner.upgrade_nginx_config(&name, custom_inventory)?;

    Ok(())
}
