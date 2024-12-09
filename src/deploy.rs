// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{inventory::AnsibleInventoryType, provisioning::ProvisionOptions},
    error::Result,
    funding::get_address_from_sk,
    get_anvil_node_data, get_genesis_multiaddr, write_environment_details, BinaryOption,
    DeploymentInventory, DeploymentType, EnvironmentDetails, EnvironmentType, EvmNetwork,
    InfraRunOptions, LogFormat, NodeType, TestnetDeployer,
};
use alloy::hex::ToHexExt;
use colored::Colorize;
use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct DeployOptions {
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub bootstrap_node_vm_count: Option<u16>,
    pub bootstrap_node_vm_size: Option<String>,
    pub bootstrap_node_volume_size: Option<u16>,
    pub chunk_size: Option<u64>,
    pub current_inventory: DeploymentInventory,
    pub downloaders_count: u16,
    pub environment_type: EnvironmentType,
    pub env_variables: Option<Vec<(String, String)>>,
    pub evm_data_payments_address: Option<String>,
    pub evm_network: EvmNetwork,
    pub evm_node_vm_size: Option<String>,
    pub evm_payment_token_address: Option<String>,
    pub evm_rpc_url: Option<String>,
    pub funding_wallet_secret_key: Option<String>,
    pub genesis_node_volume_size: Option<u16>,
    pub interval: Duration,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    pub output_inventory_dir_path: PathBuf,
    pub private_node_count: u16,
    pub private_node_vm_count: Option<u16>,
    pub private_node_volume_size: Option<u16>,
    pub public_rpc: bool,
    pub rewards_address: String,
    pub uploader_vm_count: Option<u16>,
    pub uploader_vm_size: Option<String>,
    pub uploaders_count: u16,
}

impl TestnetDeployer {
    pub async fn deploy(&self, options: &DeployOptions) -> Result<()> {
        let build_custom_binaries = {
            match &options.binary_option {
                BinaryOption::BuildFromSource { .. } => true,
                BinaryOption::Versioned { .. } => false,
            }
        };

        self.create_or_update_infra(&InfraRunOptions {
            bootstrap_node_vm_count: options.bootstrap_node_vm_count,
            bootstrap_node_vm_size: options.bootstrap_node_vm_size.clone(),
            bootstrap_node_volume_size: options.bootstrap_node_volume_size,
            enable_build_vm: build_custom_binaries,
            evm_node_count: match options.evm_network {
                EvmNetwork::Anvil => Some(1),
                EvmNetwork::ArbitrumOne => Some(0),
                EvmNetwork::ArbitrumSepolia => Some(0),
                EvmNetwork::Custom => Some(0),
            },
            evm_node_vm_size: options.evm_node_vm_size.clone(),
            genesis_vm_count: Some(1),
            genesis_node_volume_size: options.genesis_node_volume_size,
            name: options.name.clone(),
            node_vm_count: options.node_vm_count,
            node_vm_size: options.node_vm_size.clone(),
            node_volume_size: options.node_volume_size,
            private_node_vm_count: options.private_node_vm_count,
            private_node_volume_size: options.private_node_volume_size,
            tfvars_filename: options.environment_type.get_tfvars_filename(&options.name),
            uploader_vm_count: options.uploader_vm_count,
            uploader_vm_size: options.uploader_vm_size.clone(),
        })
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        // All the environment types set private_node_vm count to >0 if not specified.
        let should_provision_private_nodes = options
            .private_node_vm_count
            .map(|count| count > 0)
            .unwrap_or(true);

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::New,
                environment_type: options.environment_type.clone(),
                evm_network: options.evm_network.clone(),
                evm_data_payments_address: options.evm_data_payments_address.clone(),
                evm_payment_token_address: options.evm_payment_token_address.clone(),
                evm_rpc_url: options.evm_rpc_url.clone(),
                funding_wallet_address: None,
                rewards_address: options.rewards_address.clone(),
            },
        )
        .await?;

        let mut provision_options = ProvisionOptions::from(options.clone());
        let anvil_node_data = if options.evm_network == EvmNetwork::Anvil {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Anvil Node");
            self.ansible_provisioner
                .provision_evm_nodes(&provision_options)
                .map_err(|err| {
                    println!("Failed to provision evm node {err:?}");
                    err
                })?;

            Some(
                get_anvil_node_data(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                    .map_err(|err| {
                        println!("Failed to get evm testnet data {err:?}");
                        err
                    })?,
            )
        } else {
            None
        };

        let funding_wallet_address = if let Some(secret_key) = &options.funding_wallet_secret_key {
            let address = get_address_from_sk(secret_key)?;
            Some(address.encode_hex())
        } else if let Some(emv_data) = &anvil_node_data {
            let address = get_address_from_sk(&emv_data.deployer_wallet_private_key)?;
            Some(address.encode_hex())
        } else {
            log::error!("Funding wallet address not provided");
            None
        };

        if let Some(custom_evm) = anvil_node_data {
            provision_options.evm_data_payments_address =
                Some(custom_evm.data_payments_address.clone());
            provision_options.evm_payment_token_address =
                Some(custom_evm.payment_token_address.clone());
            provision_options.evm_rpc_url = Some(custom_evm.rpc_url.clone());
            provision_options.funding_wallet_secret_key =
                Some(custom_evm.deployer_wallet_private_key.clone());
        };

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::New,
                environment_type: options.environment_type.clone(),
                evm_network: options.evm_network.clone(),
                evm_data_payments_address: provision_options.evm_data_payments_address.clone(),
                evm_payment_token_address: provision_options.evm_payment_token_address.clone(),
                evm_rpc_url: provision_options.evm_rpc_url.clone(),
                funding_wallet_address,
                rewards_address: options.rewards_address.clone(),
            },
        )
        .await?;

        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");
            self.ansible_provisioner
                .build_safe_network_binaries(&provision_options)
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Genesis Node");
        self.ansible_provisioner
            .provision_genesis_node(&provision_options)
            .map_err(|err| {
                println!("Failed to provision genesis node {err:?}");
                err
            })?;
        let (genesis_multiaddr, _) =
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?;
        println!("Obtained multiaddr for genesis node: {genesis_multiaddr}");

        let mut node_provision_failed = false;
        self.ansible_provisioner
            .print_ansible_run_banner("Provision Bootstrap Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            &genesis_multiaddr,
            NodeType::Bootstrap,
        ) {
            Ok(()) => {
                println!("Provisioned bootstrap nodes");
            }
            Err(err) => {
                log::error!("Failed to provision bootstrap nodes: {err}");
                node_provision_failed = true;
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Normal Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            &genesis_multiaddr,
            NodeType::Generic,
        ) {
            Ok(()) => {
                println!("Provisioned normal nodes");
            }
            Err(err) => {
                log::error!("Failed to provision normal nodes: {err}");
                node_provision_failed = true;
            }
        }

        if should_provision_private_nodes {
            let private_nodes = self
                .ansible_provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::PrivateNodes, true)
                .map_err(|err| {
                    println!("Failed to obtain the inventory of private node: {err:?}");
                    err
                })?;

            provision_options.private_node_vms = private_nodes;
            self.ansible_provisioner
                .print_ansible_run_banner("Provision NAT Gateway");
            self.ansible_provisioner
                .provision_nat_gateway(&provision_options)
                .map_err(|err| {
                    println!("Failed to provision NAT gateway {err:?}");
                    err
                })?;

            self.ansible_provisioner
                .print_ansible_run_banner("Provision Private Nodes");
            match self
                .ansible_provisioner
                .provision_private_nodes(&mut provision_options, &genesis_multiaddr)
            {
                Ok(()) => {
                    println!("Provisioned private nodes");
                }
                Err(err) => {
                    log::error!("Failed to provision private nodes: {err}");
                    node_provision_failed = true;
                }
            }
        }

        if options.current_inventory.is_empty() {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Uploaders");
            self.ansible_provisioner
                .provision_uploaders(&provision_options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision uploaders {err:?}");
                    err
                })?;
        }

        if node_provision_failed {
            println!();
            println!("{}", "WARNING!".yellow());
            println!("Some nodes failed to provision without error.");
            println!("This usually means a small number of nodes failed to start on a few VMs.");
            println!("However, most of the time the deployment will still be usable.");
            println!("See the output from Ansible to determine which VMs had failures.");
        }

        Ok(())
    }
}
