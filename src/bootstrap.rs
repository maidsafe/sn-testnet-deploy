// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use std::{path::PathBuf, time::Duration};

use crate::{
    ansible::provisioning::{PrivateNodeProvisionInventory, ProvisionOptions},
    error::Result,
    write_environment_details, BinaryOption, DeploymentType, EnvironmentDetails, EnvironmentType,
    EvmDetails, EvmNetwork, InfraRunOptions, LogFormat, NodeType, TestnetDeployer,
};
use colored::Colorize;
use log::error;

#[derive(Clone)]
pub struct BootstrapOptions {
    pub binary_option: BinaryOption,
    pub chunk_size: Option<u64>,
    pub environment_type: EnvironmentType,
    pub evm_data_payments_address: Option<String>,
    pub evm_network: EvmNetwork,
    pub evm_payment_token_address: Option<String>,
    pub evm_rpc_url: Option<String>,
    pub full_cone_private_node_count: u16,
    pub full_cone_private_node_vm_count: Option<u16>,
    pub full_cone_private_node_volume_size: Option<u16>,
    pub interval: Duration,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub name: String,
    pub network_contacts_url: Option<String>,
    pub network_id: u8,
    pub node_count: u16,
    pub node_env_variables: Option<Vec<(String, String)>>,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    pub output_inventory_dir_path: PathBuf,
    pub peer: Option<String>,
    pub region: String,
    pub rewards_address: String,
    pub symmetric_private_node_count: u16,
    pub symmetric_private_node_vm_count: Option<u16>,
    pub symmetric_private_node_volume_size: Option<u16>,
    pub upnp_private_node_count: u16,
    pub upnp_private_node_vm_count: Option<u16>,
    pub upnp_private_node_volume_size: Option<u16>,
}

impl TestnetDeployer {
    pub async fn bootstrap(&self, options: &BootstrapOptions) -> Result<()> {
        let build_custom_binaries = options.binary_option.should_provision_build_machine();

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::Bootstrap,
                environment_type: options.environment_type.clone(),
                evm_details: EvmDetails {
                    network: options.evm_network.clone(),
                    data_payments_address: options.evm_data_payments_address.clone(),
                    payment_token_address: options.evm_payment_token_address.clone(),
                    rpc_url: options.evm_rpc_url.clone(),
                },
                funding_wallet_address: None,
                network_id: Some(options.network_id),
                region: options.region.clone(),
                rewards_address: Some(options.rewards_address.clone()),
            },
        )
        .await?;

        self.create_or_update_infra(&InfraRunOptions {
            client_image_id: None,
            client_vm_count: Some(0),
            client_vm_size: None,
            enable_build_vm: build_custom_binaries,
            evm_node_count: Some(0),
            evm_node_vm_size: None,
            evm_node_image_id: None,
            full_cone_vm_size: None, // We can take the value from tfvars for bootstrap deployments.
            full_cone_private_node_vm_count: options.full_cone_private_node_vm_count,
            full_cone_private_node_volume_size: options.full_cone_private_node_volume_size,
            genesis_vm_count: Some(0),
            genesis_node_volume_size: None,
            name: options.name.clone(),
            nat_gateway_image_id: None,
            node_image_id: None,
            node_vm_count: options.node_vm_count,
            node_vm_size: options.node_vm_size.clone(),
            node_volume_size: options.node_volume_size,
            peer_cache_image_id: None,
            peer_cache_node_vm_count: Some(0),
            peer_cache_node_vm_size: None,
            peer_cache_node_volume_size: None,
            region: options.region.clone(),
            symmetric_nat_gateway_vm_size: None, // We can take the value from tfvars for bootstrap deployments.
            symmetric_private_node_vm_count: options.symmetric_private_node_vm_count,
            symmetric_private_node_volume_size: options.symmetric_private_node_volume_size,
            tfvars_filenames: Some(
                options
                    .environment_type
                    .get_tfvars_filenames(&options.name, &options.region),
            ),
            upnp_vm_size: None,
            upnp_private_node_vm_count: options.upnp_private_node_vm_count,
            upnp_private_node_volume_size: options.upnp_private_node_volume_size,
        })
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        let mut provision_options = ProvisionOptions::from(options.clone());
        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");
            self.ansible_provisioner
                .build_autonomi_binaries(&provision_options, None)
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
        }

        let mut failed_to_provision = false;

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Public Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            options.peer.clone(),
            options.network_contacts_url.clone(),
            NodeType::Generic,
        ) {
            Ok(()) => {
                println!("Provisioned public nodes");
            }
            Err(e) => {
                println!("Failed to provision public nodes: {e:?}");
                failed_to_provision = true;
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision UPnP Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            options.peer.clone(),
            options.network_contacts_url.clone(),
            NodeType::Upnp,
        ) {
            Ok(()) => {
                println!("Provisioned UPnP nodes");
            }
            Err(e) => {
                error!("Failed to provision UPnP nodes: {e:?}");
                failed_to_provision = true;
            }
        }

        let private_node_inventory = PrivateNodeProvisionInventory::new(
            &self.ansible_provisioner,
            options.full_cone_private_node_vm_count,
            options.symmetric_private_node_vm_count,
        )?;

        if private_node_inventory.should_provision_full_cone_private_nodes() {
            match self.ansible_provisioner.provision_full_cone(
                &provision_options,
                options.peer.clone(),
                options.network_contacts_url.clone(),
                private_node_inventory.clone(),
                None,
            ) {
                Ok(()) => {
                    println!("Provisioned full cone nodes and gateway");
                }
                Err(err) => {
                    error!("Failed to provision full cone nodes and gateway: {err}");
                    failed_to_provision = true;
                }
            }
        }

        if private_node_inventory.should_provision_symmetric_private_nodes() {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Symmetric NAT Gateway");
            self.ansible_provisioner
                .provision_symmetric_nat_gateway(&provision_options, &private_node_inventory)
                .map_err(|err| {
                    println!("Failed to provision symmetric NAT gateway {err:?}");
                    err
                })?;

            self.ansible_provisioner
                .print_ansible_run_banner("Provision Symmetric Private Nodes");
            match self.ansible_provisioner.provision_symmetric_private_nodes(
                &mut provision_options,
                options.peer.clone(),
                options.network_contacts_url.clone(),
                &private_node_inventory,
            ) {
                Ok(()) => {
                    println!("Provisioned symmetric private nodes");
                }
                Err(err) => {
                    error!("Failed to provision symmetric private nodes: {err}");
                    failed_to_provision = true;
                }
            }
        }

        if failed_to_provision {
            println!("{}", "WARNING!".yellow());
            println!("Some nodes failed to provision without error.");
            println!("This usually means a small number of nodes failed to start on a few VMs.");
            println!("However, most of the time the deployment will still be usable.");
            println!("See the output from Ansible to determine which VMs had failures.");
        }

        Ok(())
    }
}
