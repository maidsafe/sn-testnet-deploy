// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::provisioning::{NodeType, ProvisionOptions},
    error::Result,
    get_genesis_multiaddr, write_environment_details, BinaryOption, DeploymentInventory,
    DeploymentType, EnvironmentDetails, EnvironmentType, LogFormat, TestnetDeployer,
};
use colored::Colorize;
use std::net::SocketAddr;

#[derive(Clone)]
pub struct DeployOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub bootstrap_node_vm_count: Option<u16>,
    pub current_inventory: DeploymentInventory,
    pub environment_type: EnvironmentType,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: Option<u16>,
    pub public_rpc: bool,
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
                environment_type: options.environment_type.clone(),
                deployment_type: DeploymentType::New,
            },
        )
        .await?;

        self.create_or_update_infra(
            &options.name,
            Some(1),
            None,
            options.bootstrap_node_vm_count,
            options.node_vm_count,
            options.uploader_vm_count,
            build_custom_binaries,
            &options.environment_type.get_tfvars_filename(),
        )
        .await
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        let mut n = 1;
        let mut total = if build_custom_binaries { 9 } else { 8 };
        if !options.current_inventory.is_empty() {
            total -= 4;
        }

        let provision_options = ProvisionOptions::from(options.clone());
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
            Err(_) => {
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
            Err(_) => {
                node_provision_failed = true;
            }
        }
        n += 1;

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
