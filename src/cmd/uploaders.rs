// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::*;
use alloy::primitives::U256;
use ant_releases::ReleaseType;
use color_eyre::eyre::{eyre, Result};
use sn_testnet_deploy::{
    ansible::{extra_vars::ExtraVarsDocBuilder, inventory::AnsibleInventoryType, AnsiblePlaybook},
    inventory::DeploymentInventoryService,
    upscale::UpscaleOptions,
    TestnetDeployBuilder,
};

#[derive(Subcommand, Debug)]
pub enum UploadersCommands {
    /// Start all uploaders for an environment
    Start {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Stop all uploaders for an environment.
    Stop {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Upgrade the uploaders for a given environment.
    Upgrade {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,

        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,

        /// Optionally supply a version for the safe client binary to upgrade to.
        ///
        /// If not provided, the latest version will be used.
        #[arg(long)]
        version: Option<String>,
    },
    /// Upscale uploaders for an existing network.
    Upscale {
        /// Supply a version number for the autonomi binary to be used for new uploader VMs.
        ///
        /// There should be no 'v' prefix.
        #[arg(long, verbatim_doc_comment)]
        autonomi_version: String,
        /// The desired number of uploader VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_uploader_vm_count: Option<u16>,
        /// The desired number of uploaders to be running after the scale.
        ///
        /// If you want each uploader VM to run multiple uploader services, specify the total desired count.
        #[clap(long, verbatim_doc_comment)]
        desired_uploaders_count: Option<u16>,
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The amount of gas tokens to transfer to each uploader.
        /// Must be a decimal value between 0 and 1, e.g. "0.1"
        #[clap(long)]
        gas_amount: Option<String>,
        /// Set to only use Terraform to upscale the VMs and not run Ansible.
        #[clap(long, default_value_t = false)]
        infra_only: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Set to only run the Terraform plan rather than applying the changes.
        ///
        /// Can be useful to preview the upscale to make sure everything is ok and that no other
        /// changes slipped in.
        ///
        /// The plan will run and then the command will exit without doing anything else.
        #[clap(long, default_value_t = false)]
        plan: bool,
        /// Set to skip the Terraform infrastructure run and only run the Ansible provisioning.
        #[clap(long, default_value_t = false)]
        provision_only: bool,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
}

pub async fn handle_uploaders_command(cmd: UploadersCommands) -> Result<()> {
    match cmd {
        UploadersCommands::Start { name, provider } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            let ansible_runner = testnet_deployer.ansible_provisioner.ansible_runner;
            ansible_runner.run_playbook(
                AnsiblePlaybook::StartUploaders,
                AnsibleInventoryType::Uploaders,
                None,
            )?;
            Ok(())
        }
        UploadersCommands::Stop { name, provider } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            let ansible_runner = testnet_deployer.ansible_provisioner.ansible_runner;
            ansible_runner.run_playbook(
                AnsiblePlaybook::StopUploaders,
                AnsibleInventoryType::Uploaders,
                None,
            )?;
            Ok(())
        }
        UploadersCommands::Upgrade {
            name,
            provider,
            version,
        } => {
            let version = get_version_from_option(version, &ReleaseType::Ant).await?;

            let testnet_deploy = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deploy);

            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The '{}' environment does not exist", name));
            }

            let ansible_runner = testnet_deploy.ansible_provisioner.ansible_runner;
            let mut extra_vars = ExtraVarsDocBuilder::default();
            extra_vars.add_variable("testnet_name", &name);
            extra_vars.add_variable("ant_version", &version.to_string());
            ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeUploaders,
                AnsibleInventoryType::Uploaders,
                Some(extra_vars.build()),
            )?;

            Ok(())
        }
        UploadersCommands::Upscale {
            autonomi_version,
            desired_uploader_vm_count,
            desired_uploaders_count,
            funding_wallet_secret_key,
            gas_amount,
            infra_only,
            name,
            plan,
            provision_only,
            provider,
        } => {
            let gas_amount = if let Some(amount) = gas_amount {
                let amount: f64 = amount.parse().map_err(|_| {
                    eyre!("Invalid gas amount format. Must be a decimal value, e.g. '0.1'")
                })?;
                if amount <= 0.0 || amount >= 1.0 {
                    return Err(eyre!("Gas amount must be between 0 and 1"));
                }
                // Convert to wei (1 ETH = 1e18 wei)
                let wei_amount = (amount * 1e18) as u64;
                Some(U256::from(wei_amount))
            } else {
                None
            };

            println!("Upscaling uploaders...");
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            testnet_deployer
                .upscale_uploaders(&UpscaleOptions {
                    ansible_verbose: false,
                    current_inventory: inventory,
                    desired_full_cone_private_node_count: None,
                    desired_full_cone_private_node_vm_count: None,
                    desired_node_count: None,
                    desired_node_vm_count: None,
                    desired_peer_cache_node_count: None,
                    desired_peer_cache_node_vm_count: None,
                    desired_symmetric_private_node_count: None,
                    desired_symmetric_private_node_vm_count: None,
                    desired_uploader_vm_count,
                    desired_uploaders_count,
                    funding_wallet_secret_key,
                    gas_amount,
                    max_archived_log_files: 1,
                    max_log_files: 1,
                    infra_only,
                    interval: Duration::from_millis(2000),
                    plan,
                    provision_only,
                    public_rpc: false,
                    ant_version: Some(autonomi_version),
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
                        eprintln!(
                            "Please try running the `inventory` command or workflow separately"
                        );
                        return Ok(());
                    }
                }
            };

            inventory.print_report(false)?;
            inventory.save()?;

            Ok(())
        }
    }
}
