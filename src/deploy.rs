// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{inventory::AnsibleInventoryType, provisioning::ProvisionOptions},
    error::Result,
    get_genesis_multiaddr, write_environment_details, BinaryOption, DeploymentInventory,
    DeploymentType, EnvironmentDetails, EnvironmentType, EvmNetwork, InfraRunOptions, LogFormat,
    NodeType, TestnetDeployer,
};
use colored::Colorize;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone)]
pub struct DeployOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub bootstrap_node_vm_count: Option<u16>,
    pub chunk_size: Option<u64>,
    pub current_inventory: DeploymentInventory,
    pub downloaders_count: u16,
    pub environment_type: EnvironmentType,
    pub env_variables: Option<Vec<(String, String)>>,
    pub evm_network: EvmNetwork,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: Option<u16>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub output_inventory_dir_path: PathBuf,
    pub private_node_vm_count: Option<u16>,
    pub private_node_count: u16,
    pub public_rpc: bool,
    pub uploaders_count: u16,
    pub uploader_vm_count: Option<u16>,
}

impl TestnetDeployer {
    pub async fn deploy(&self, options: &DeployOptions) -> Result<()> {
        let build_custom_binaries = {
            match &options.binary_option {
                BinaryOption::BuildFromSource { .. } => true,
                BinaryOption::Versioned { .. } => false,
            }
        };

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::New,
                environment_type: options.environment_type.clone(),
                evm_network: options.evm_network.clone(),
            },
        )
        .await?;

        self.create_or_update_infra(&InfraRunOptions {
            auditor_vm_count: Some(1),
            bootstrap_node_vm_count: options.bootstrap_node_vm_count,
            enable_build_vm: build_custom_binaries,
            evm_node_count: match options.evm_network {
                EvmNetwork::ArbitrumOne => Some(0),
                EvmNetwork::Custom => Some(1),
            },
            genesis_vm_count: Some(1),
            name: options.name.clone(),
            node_vm_count: options.node_vm_count,
            private_node_vm_count: options.private_node_vm_count,
            tfvars_filename: options.environment_type.get_tfvars_filename(),
            uploader_vm_count: options.uploader_vm_count,
        })
        .await
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        // All the environment types set private_node_vm count to >0 if not specified.
        let should_provision_private_nodes = options
            .private_node_vm_count
            .map(|count| count > 0)
            .unwrap_or(true);

        let mut n = 1;
        let mut total = if build_custom_binaries { 9 } else { 8 };
        if should_provision_private_nodes {
            total += 2;
        }
        if !options.current_inventory.is_empty() {
            total -= 5;
        }
        if matches!(options.evm_network, EvmNetwork::Custom) {
            total += 1;
        }

        let mut provision_options = ProvisionOptions::from(options.clone());
        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Build Custom Binaries");
            self.ansible_provisioner
                .build_safe_network_binaries(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
            n += 1;
        }

        if matches!(options.evm_network, EvmNetwork::Custom) {
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Provision EVM Node");
            self.ansible_provisioner
                .provision_evm_nodes(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to provision evm node {err:?}");
                    err
                })?;
            n += 1;
        }

        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Genesis Node");
        self.ansible_provisioner
            .provision_genesis_node(&provision_options)
            .await
            .map_err(|err| {
                println!("Failed to provision genesis node {err:?}");
                err
            })?;
        n += 1;
        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .await
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?;
        println!("Obtained multiaddr for genesis node: {genesis_multiaddr}");

        let mut node_provision_failed = false;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Bootstrap Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(&provision_options, &genesis_multiaddr, NodeType::Bootstrap)
            .await
        {
            Ok(()) => {
                println!("Provisioned bootstrap nodes");
            }
            Err(err) => {
                log::error!("Failed to provision bootstrap nodes: {err}");
                node_provision_failed = true;
            }
        }
        n += 1;

        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Normal Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(&provision_options, &genesis_multiaddr, NodeType::Normal)
            .await
        {
            Ok(()) => {
                println!("Provisioned normal nodes");
            }
            Err(err) => {
                log::error!("Failed to provision normal nodes: {err}");
                node_provision_failed = true;
            }
        }
        n += 1;

        if should_provision_private_nodes {
            let private_nodes = self
                .ansible_provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::PrivateNodes, true)
                .await
                .map_err(|err| {
                    println!("Failed to obtain the inventory of private node: {err:?}");
                    err
                })?;

            provision_options.private_node_vms = private_nodes;
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Provision NAT Gateway");
            self.ansible_provisioner
                .provision_nat_gateway(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to provision NAT gateway {err:?}");
                    err
                })?;

            n += 1;

            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Provision Private Nodes");
            match self
                .ansible_provisioner
                .provision_private_nodes(&mut provision_options, &genesis_multiaddr)
                .await
            {
                Ok(()) => {
                    println!("Provisioned private nodes");
                }
                Err(err) => {
                    log::error!("Failed to provision private nodes: {err}");
                    node_provision_failed = true;
                }
            }
            n += 1;
        }

        if options.current_inventory.is_empty() {
            // These steps are only necessary on the initial deploy, at which point the inventory
            // will be empty.
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Deploy and Start Faucet");
            self.ansible_provisioner
                .provision_and_start_faucet(&provision_options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision faucet {err:?}");
                    err
                })?;
            n += 1;
            self.ansible_provisioner.print_ansible_run_banner(
                n,
                total,
                "Provision RPC Client on Genesis Node",
            );
            self.ansible_provisioner
                .provision_safenode_rpc_client(&provision_options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision safenode rpc client {err:?}");
                    err
                })?;
            n += 1;
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Provision Auditor");
            self.ansible_provisioner
                .provision_sn_auditor(&provision_options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision sn_auditor {err:?}");
                    err
                })?;
            n += 1;
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Provision Uploaders");
            self.ansible_provisioner
                .provision_uploaders(&provision_options, &genesis_multiaddr, &genesis_ip)
                .await
                .map_err(|err| {
                    println!("Failed to provision uploaders {err:?}");
                    err
                })?;

            n += 1;
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Stop Faucet");
            self.ansible_provisioner
                .provision_and_stop_faucet(&provision_options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to stop faucet {err:?}");
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
