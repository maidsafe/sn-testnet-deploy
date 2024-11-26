// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{inventory::AnsibleInventoryType, provisioning::ProvisionOptions},
    error::{Error, Result},
    get_genesis_multiaddr, get_multiaddr, DeploymentInventory, DeploymentType, InfraRunOptions,
    NodeType, TestnetDeployer,
};
use colored::Colorize;
use evmlib::common::U256;
use log::debug;
use std::{collections::HashSet, time::Duration};

#[derive(Clone)]
pub struct UpscaleOptions {
    pub ansible_verbose: bool,
    pub current_inventory: DeploymentInventory,
    pub desired_auditor_vm_count: Option<u16>,
    pub desired_bootstrap_node_count: Option<u16>,
    pub desired_bootstrap_node_vm_count: Option<u16>,
    pub desired_node_count: Option<u16>,
    pub desired_node_vm_count: Option<u16>,
    pub desired_private_node_count: Option<u16>,
    pub desired_private_node_vm_count: Option<u16>,
    pub desired_uploader_vm_count: Option<u16>,
    pub desired_uploaders_count: Option<u16>,
    pub downloaders_count: u16,
    pub funding_wallet_secret_key: Option<String>,
    pub gas_amount: Option<U256>,
    pub interval: Duration,
    pub infra_only: bool,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub plan: bool,
    pub public_rpc: bool,
    pub safe_version: Option<String>,
    pub provision_only: bool,
}

impl TestnetDeployer {
    pub async fn upscale(&self, options: &UpscaleOptions) -> Result<()> {
        let is_bootstrap_deploy = matches!(
            options
                .current_inventory
                .environment_details
                .deployment_type,
            DeploymentType::Bootstrap
        );

        if is_bootstrap_deploy
            && (options.desired_auditor_vm_count.is_some()
                || options.desired_bootstrap_node_count.is_some()
                || options.desired_bootstrap_node_vm_count.is_some()
                || options.desired_uploader_vm_count.is_some())
        {
            return Err(Error::InvalidUpscaleOptionsForBootstrapDeployment);
        }

        let desired_bootstrap_node_vm_count = options
            .desired_bootstrap_node_vm_count
            .unwrap_or(options.current_inventory.bootstrap_node_vms.len() as u16);
        if desired_bootstrap_node_vm_count
            < options.current_inventory.bootstrap_node_vms.len() as u16
        {
            return Err(Error::InvalidUpscaleDesiredBootstrapVmCount);
        }
        debug!("Using {desired_bootstrap_node_vm_count} for desired bootstrap node VM count");

        let desired_node_vm_count = options
            .desired_node_vm_count
            .unwrap_or(options.current_inventory.node_vms.len() as u16);
        if desired_node_vm_count < options.current_inventory.node_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredNodeVmCount);
        }
        debug!("Using {desired_node_vm_count} for desired node VM count");

        let desired_private_node_vm_count = options
            .desired_private_node_vm_count
            .unwrap_or(options.current_inventory.private_node_vms.len() as u16);
        if desired_private_node_vm_count < options.current_inventory.private_node_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredPrivateNodeVmCount);
        }
        debug!("Using {desired_private_node_vm_count} for desired private node VM count");

        let desired_uploader_vm_count = options
            .desired_uploader_vm_count
            .unwrap_or(options.current_inventory.uploader_vms.len() as u16);
        if desired_uploader_vm_count < options.current_inventory.uploader_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredUploaderVmCount);
        }
        debug!("Using {desired_uploader_vm_count} for desired uploader VM count");

        let desired_bootstrap_node_count = options
            .desired_bootstrap_node_count
            .unwrap_or(options.current_inventory.bootstrap_node_count() as u16);
        if desired_bootstrap_node_count < options.current_inventory.bootstrap_node_count() as u16 {
            return Err(Error::InvalidUpscaleDesiredBootstrapNodeCount);
        }
        debug!("Using {desired_bootstrap_node_count} for desired bootstrap node count");

        let desired_node_count = options
            .desired_node_count
            .unwrap_or(options.current_inventory.node_count() as u16);
        if desired_node_count < options.current_inventory.node_count() as u16 {
            return Err(Error::InvalidUpscaleDesiredNodeCount);
        }
        debug!("Using {desired_node_count} for desired node count");

        let desired_private_node_count = options
            .desired_private_node_count
            .unwrap_or(options.current_inventory.private_node_count() as u16);
        if desired_private_node_count < options.current_inventory.private_node_count() as u16 {
            return Err(Error::InvalidUpscaleDesiredPrivateNodeCount);
        }
        debug!("Using {desired_private_node_count} for desired private node count");

        if options.plan {
            let vars = vec![
                (
                    "bootstrap_node_vm_count".to_string(),
                    desired_bootstrap_node_vm_count.to_string(),
                ),
                (
                    "node_vm_count".to_string(),
                    desired_node_vm_count.to_string(),
                ),
                (
                    "private_node_vm_count".to_string(),
                    desired_private_node_vm_count.to_string(),
                ),
                (
                    "uploader_vm_count".to_string(),
                    desired_uploader_vm_count.to_string(),
                ),
                (
                    "genesis_vm_count".to_string(),
                    match options
                        .current_inventory
                        .environment_details
                        .deployment_type
                    {
                        DeploymentType::New => "1",
                        DeploymentType::Bootstrap => "0",
                    }
                    .to_string(),
                ),
                (
                    "setup_nat_gateway".to_string(),
                    (desired_private_node_vm_count > 0).to_string(),
                ),
            ];
            self.plan(Some(vars), &options.current_inventory.get_tfvars_filename())?;
            return Ok(());
        }

        let mut infra_run_options = InfraRunOptions::generate_from_deployment(
            &options.current_inventory,
            &self.terraform_runner,
        )?;
        infra_run_options.bootstrap_node_vm_count = Some(desired_bootstrap_node_vm_count);
        infra_run_options.node_vm_count = Some(desired_node_vm_count);
        infra_run_options.private_node_vm_count = Some(desired_private_node_vm_count);
        infra_run_options.uploader_vm_count = Some(desired_uploader_vm_count);

        self.create_or_update_infra(&infra_run_options)
            .map_err(|err| {
                println!("Failed to create infra {err:?}");
                err
            })?;

        if options.infra_only {
            return Ok(());
        }

        let mut provision_options = ProvisionOptions {
            binary_option: options.current_inventory.binary_option.clone(),
            bootstrap_node_count: desired_bootstrap_node_count,
            chunk_size: None,
            downloaders_count: options.downloaders_count,
            env_variables: None,
            evm_network: options
                .current_inventory
                .environment_details
                .evm_network
                .clone(),
            evm_data_payments_address: options
                .current_inventory
                .environment_details
                .evm_data_payments_address
                .clone(),
            evm_payment_token_address: options
                .current_inventory
                .environment_details
                .evm_payment_token_address
                .clone(),
            evm_rpc_url: options
                .current_inventory
                .environment_details
                .evm_rpc_url
                .clone(),
            funding_wallet_secret_key: options.funding_wallet_secret_key.clone(),
            interval: options.interval,
            log_format: None,
            logstash_details: None,
            name: options.current_inventory.name.clone(),
            nat_gateway: None,
            node_count: desired_node_count,
            max_archived_log_files: options.max_archived_log_files,
            max_log_files: options.max_log_files,
            output_inventory_dir_path: self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
            private_node_count: desired_private_node_count,
            private_node_vms: Vec::new(),
            public_rpc: options.public_rpc,
            rewards_address: options
                .current_inventory
                .environment_details
                .rewards_address
                .clone(),
            safe_version: options.safe_version.clone(),
            uploaders_count: options.desired_uploaders_count,
            gas_amount: options.gas_amount,
        };
        let mut node_provision_failed = false;

        let (initial_multiaddr, _) = if is_bootstrap_deploy {
            get_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client).map_err(
                |err| {
                    println!("Failed to get node multiaddr {err:?}");
                    err
                },
            )?
        } else {
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?
        };
        debug!("Retrieved initial peer {initial_multiaddr}");

        let should_provision_private_nodes = desired_private_node_vm_count > 0;

        if !is_bootstrap_deploy {
            self.wait_for_ssh_availability_on_new_machines(
                AnsibleInventoryType::BootstrapNodes,
                &options.current_inventory,
            )?;
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Bootstrap Nodes");
            match self.ansible_provisioner.provision_nodes(
                &provision_options,
                &initial_multiaddr,
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
        }

        self.wait_for_ssh_availability_on_new_machines(
            AnsibleInventoryType::Nodes,
            &options.current_inventory,
        )?;
        self.ansible_provisioner
            .print_ansible_run_banner("Provision Normal Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            &initial_multiaddr,
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

            self.wait_for_ssh_availability_on_new_machines(
                AnsibleInventoryType::NatGateway,
                &options.current_inventory,
            )?;
            self.ansible_provisioner
                .print_ansible_run_banner("Provision NAT Gateway");
            self.ansible_provisioner
                .provision_nat_gateway(&provision_options)
                .map_err(|err| {
                    println!("Failed to provision NAT gateway {err:?}");
                    err
                })?;

            self.wait_for_ssh_availability_on_new_machines(
                AnsibleInventoryType::PrivateNodes,
                &options.current_inventory,
            )?;
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Private Nodes");
            match self
                .ansible_provisioner
                .provision_private_nodes(&mut provision_options, &initial_multiaddr)
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

        // TODO: Uncomment when EVM-based payments are supported for the uploader upscale.
        // if !is_bootstrap_deploy {
        //     self.wait_for_ssh_availability_on_new_machines(
        //         AnsibleInventoryType::Uploaders,
        //         &options.current_inventory,
        //     )?;
        //     self.ansible_provisioner
        //         .print_ansible_run_banner(n, total, "Provision Uploaders");
        //     self.ansible_provisioner
        //         .provision_uploaders(&provision_options, &initial_multiaddr)
        //         .await
        //         .map_err(|err| {
        //             println!("Failed to provision uploaders {err:?}");
        //             err
        //         })?;
        // }

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

    pub async fn upscale_uploaders(&self, options: &UpscaleOptions) -> Result<()> {
        let is_bootstrap_deploy = matches!(
            options
                .current_inventory
                .environment_details
                .deployment_type,
            DeploymentType::Bootstrap
        );

        if is_bootstrap_deploy {
            return Err(Error::InvalidUploaderUpscaleDeploymentType(
                "bootstrap".to_string(),
            ));
        }

        let desired_uploader_vm_count = options
            .desired_uploader_vm_count
            .unwrap_or(options.current_inventory.uploader_vms.len() as u16);
        if desired_uploader_vm_count < options.current_inventory.uploader_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredUploaderVmCount);
        }
        debug!("Using {desired_uploader_vm_count} for desired uploader VM count");

        if options.plan {
            let vars = vec![(
                "uploader_vm_count".to_string(),
                desired_uploader_vm_count.to_string(),
            )];
            self.plan(Some(vars), &options.current_inventory.get_tfvars_filename())?;
            return Ok(());
        }

        if !options.provision_only {
            let mut infra_run_options = InfraRunOptions::generate_from_deployment(
                &options.current_inventory,
                &self.terraform_runner,
            )?;
            infra_run_options.uploader_vm_count = Some(desired_uploader_vm_count);
            self.create_or_update_infra(&infra_run_options)
                .map_err(|err| {
                    println!("Failed to create infra {err:?}");
                    err
                })?;
        }

        if options.infra_only {
            return Ok(());
        }

        let (initial_multiaddr, _) =
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?;
        debug!("Retrieved initial peer {initial_multiaddr}");

        let provision_options = ProvisionOptions {
            binary_option: options.current_inventory.binary_option.clone(),
            bootstrap_node_count: 0,
            chunk_size: None,
            downloaders_count: options.downloaders_count,
            env_variables: None,
            evm_data_payments_address: options
                .current_inventory
                .environment_details
                .evm_data_payments_address
                .clone(),
            evm_network: options
                .current_inventory
                .environment_details
                .evm_network
                .clone(),
            evm_payment_token_address: options
                .current_inventory
                .environment_details
                .evm_payment_token_address
                .clone(),
            evm_rpc_url: options
                .current_inventory
                .environment_details
                .evm_rpc_url
                .clone(),
            funding_wallet_secret_key: options.funding_wallet_secret_key.clone(),
            interval: options.interval,
            log_format: None,
            logstash_details: None,
            name: options.current_inventory.name.clone(),
            nat_gateway: None,
            node_count: 0,
            max_archived_log_files: options.max_archived_log_files,
            max_log_files: options.max_log_files,
            output_inventory_dir_path: self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
            private_node_count: 0,
            private_node_vms: Vec::new(),
            public_rpc: options.public_rpc,
            rewards_address: options
                .current_inventory
                .environment_details
                .rewards_address
                .clone(),
            safe_version: options.safe_version.clone(),
            uploaders_count: options.desired_uploaders_count,
            gas_amount: options.gas_amount,
        };

        self.wait_for_ssh_availability_on_new_machines(
            AnsibleInventoryType::Uploaders,
            &options.current_inventory,
        )?;
        self.ansible_provisioner
            .print_ansible_run_banner("Provision Uploaders");
        self.ansible_provisioner
            .provision_uploaders(&provision_options, &initial_multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to provision uploaders {err:?}");
                err
            })?;

        Ok(())
    }

    fn wait_for_ssh_availability_on_new_machines(
        &self,
        inventory_type: AnsibleInventoryType,
        current_inventory: &DeploymentInventory,
    ) -> Result<()> {
        let inventory = self
            .ansible_provisioner
            .ansible_runner
            .get_inventory(inventory_type, true)?;
        let old_set: HashSet<_> = match inventory_type {
            AnsibleInventoryType::BootstrapNodes => current_inventory
                .bootstrap_node_vms
                .iter()
                .map(|node_vm| &node_vm.vm)
                .cloned()
                .collect(),
            AnsibleInventoryType::Nodes => current_inventory
                .node_vms
                .iter()
                .map(|node_vm| &node_vm.vm)
                .cloned()
                .collect(),
            AnsibleInventoryType::Uploaders => current_inventory
                .uploader_vms
                .iter()
                .map(|uploader_vm| &uploader_vm.vm)
                .cloned()
                .collect(),
            AnsibleInventoryType::NatGateway => {
                current_inventory.nat_gateway_vm.iter().cloned().collect()
            }
            AnsibleInventoryType::PrivateNodes => current_inventory
                .private_node_vms
                .iter()
                .map(|node_vm| &node_vm.vm)
                .cloned()
                .collect(),
            it => return Err(Error::UpscaleInventoryTypeNotSupported(it.to_string())),
        };
        let new_vms: Vec<_> = inventory
            .into_iter()
            .filter(|item| !old_set.contains(item))
            .collect();
        for vm in new_vms.iter() {
            self.ssh_client.wait_for_ssh_availability(
                &vm.public_ip_addr,
                &self.cloud_provider.get_ssh_user(),
            )?;
        }
        Ok(())
    }
}
