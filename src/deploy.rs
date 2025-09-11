// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        inventory::AnsibleInventoryType,
        provisioning::{PrivateNodeProvisionInventory, ProvisionOptions},
    },
    error::Result,
    funding::get_address_from_sk,
    get_anvil_node_data, get_bootstrap_cache_url, get_genesis_multiaddr, write_environment_details,
    BinaryOption, DeploymentInventory, DeploymentType, EnvironmentDetails, EnvironmentType,
    EvmDetails, EvmNetwork, InfraRunOptions, LogFormat, NodeType, TestnetDeployer,
};
use alloy::{hex::ToHexExt, primitives::U256};
use colored::Colorize;
use log::error;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
pub struct DeployOptions {
    pub binary_option: BinaryOption,
    pub chunk_size: Option<u64>,
    pub client_env_variables: Option<Vec<(String, String)>>,
    pub client_vm_count: Option<u16>,
    pub client_vm_size: Option<String>,
    pub current_inventory: DeploymentInventory,
    pub enable_delayed_verifier: bool,
    pub enable_performance_verifier: bool,
    pub enable_random_verifier: bool,
    pub enable_telegraf: bool,
    pub environment_type: EnvironmentType,
    pub evm_data_payments_address: Option<String>,
    pub evm_network: EvmNetwork,
    pub evm_node_vm_size: Option<String>,
    pub evm_payment_token_address: Option<String>,
    pub evm_rpc_url: Option<String>,
    pub full_cone_vm_size: Option<String>,
    pub full_cone_private_node_count: u16,
    pub full_cone_private_node_vm_count: Option<u16>,
    pub full_cone_private_node_volume_size: Option<u16>,
    pub funding_wallet_secret_key: Option<String>,
    pub genesis_node_volume_size: Option<u16>,
    pub initial_gas: Option<U256>,
    pub initial_tokens: Option<U256>,
    pub interval: Duration,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub name: String,
    pub network_id: u8,
    pub network_dashboard_branch: Option<String>,
    pub node_count: u16,
    pub node_env_variables: Option<Vec<(String, String)>>,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    pub output_inventory_dir_path: PathBuf,
    pub peer_cache_node_count: u16,
    pub peer_cache_node_vm_count: Option<u16>,
    pub peer_cache_node_vm_size: Option<String>,
    pub peer_cache_node_volume_size: Option<u16>,
    pub port_restricted_cone_vm_size: Option<String>,
    pub port_restricted_cone_private_node_count: u16,
    pub port_restricted_cone_private_node_vm_count: u16,
    pub port_restricted_cone_private_node_volume_size: Option<u16>,
    pub symmetric_nat_gateway_vm_size: Option<String>,
    pub symmetric_private_node_count: u16,
    pub symmetric_private_node_vm_count: Option<u16>,
    pub symmetric_private_node_volume_size: Option<u16>,
    pub public_rpc: bool,
    pub region: String,
    pub rewards_address: String,
    pub uploaders_count: u16,
    pub upload_interval: u16,
    pub upload_size: u16,
    pub upnp_vm_size: Option<String>,
    pub upnp_private_node_count: u16,
    pub upnp_private_node_vm_count: Option<u16>,
    pub upnp_private_node_volume_size: Option<u16>,
}

impl TestnetDeployer {
    pub async fn deploy_to_genesis(
        &self,
        options: &DeployOptions,
    ) -> Result<(ProvisionOptions, (String, String))> {
        let build_custom_binaries = options.binary_option.should_provision_build_machine();

        self.create_or_update_infra(&InfraRunOptions {
            client_image_id: None,
            client_vm_count: options.client_vm_count,
            client_vm_size: options.client_vm_size.clone(),
            enable_build_vm: build_custom_binaries,
            evm_node_count: match options.evm_network {
                EvmNetwork::Anvil => Some(1),
                EvmNetwork::ArbitrumOne => Some(0),
                EvmNetwork::ArbitrumSepoliaTest => Some(0),
                EvmNetwork::Custom => Some(0),
            },
            evm_node_vm_size: options.evm_node_vm_size.clone(),
            evm_node_image_id: None,
            full_cone_vm_size: options.full_cone_vm_size.clone(),
            full_cone_private_node_vm_count: options.full_cone_private_node_vm_count,
            full_cone_private_node_volume_size: options.full_cone_private_node_volume_size,
            genesis_vm_count: Some(1),
            genesis_node_volume_size: options.genesis_node_volume_size,
            name: options.name.clone(),
            nat_gateway_image_id: None,
            node_image_id: None,
            node_vm_count: options.node_vm_count,
            node_vm_size: options.node_vm_size.clone(),
            node_volume_size: options.node_volume_size,
            peer_cache_image_id: None,
            peer_cache_node_vm_count: options.peer_cache_node_vm_count,
            peer_cache_node_vm_size: options.peer_cache_node_vm_size.clone(),
            peer_cache_node_volume_size: options.peer_cache_node_volume_size,
            port_restricted_cone_vm_size: options.port_restricted_cone_vm_size.clone(),
            port_restricted_private_node_vm_count: Some(
                options.port_restricted_cone_private_node_vm_count,
            ),
            port_restricted_private_node_volume_size: options
                .port_restricted_cone_private_node_volume_size,
            region: options.region.clone(),
            symmetric_nat_gateway_vm_size: options.symmetric_nat_gateway_vm_size.clone(),
            symmetric_private_node_vm_count: options.symmetric_private_node_vm_count,
            symmetric_private_node_volume_size: options.symmetric_private_node_volume_size,
            tfvars_filenames: Some(
                options
                    .environment_type
                    .get_tfvars_filenames(&options.name, &options.region),
            ),
            upnp_vm_size: options.upnp_vm_size.clone(),
            upnp_private_node_vm_count: options.upnp_private_node_vm_count,
            upnp_private_node_volume_size: options.upnp_private_node_volume_size,
        })
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::New,
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
            error!("Funding wallet address not provided");
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
                evm_details: EvmDetails {
                    network: options.evm_network.clone(),
                    data_payments_address: provision_options.evm_data_payments_address.clone(),
                    payment_token_address: provision_options.evm_payment_token_address.clone(),
                    rpc_url: provision_options.evm_rpc_url.clone(),
                },
                funding_wallet_address,
                network_id: Some(options.network_id),
                region: options.region.clone(),
                rewards_address: Some(options.rewards_address.clone()),
            },
        )
        .await?;

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

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Genesis Node");
        self.ansible_provisioner
            .provision_genesis_node(&provision_options)
            .map_err(|err| {
                println!("Failed to provision genesis node {err:?}");
                err
            })?;

        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?;

        Ok((
            provision_options,
            (genesis_multiaddr, get_bootstrap_cache_url(&genesis_ip)),
        ))
    }

    pub async fn deploy(&self, options: &DeployOptions) -> Result<()> {
        let (mut provision_options, (genesis_multiaddr, genesis_network_contacts)) =
            self.deploy_to_genesis(options).await?;

        println!("Obtained multiaddr for genesis node: {genesis_multiaddr}, network contact: {genesis_network_contacts}");

        let mut node_provision_failed = false;
        self.ansible_provisioner
            .print_ansible_run_banner("Provision Peer Cache Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
            NodeType::PeerCache,
        ) {
            Ok(()) => {
                println!("Provisioned Peer Cache nodes");
            }
            Err(err) => {
                error!("Failed to provision Peer Cache nodes: {err}");
                node_provision_failed = true;
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Public Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
            NodeType::Generic,
        ) {
            Ok(()) => {
                println!("Provisioned public nodes");
            }
            Err(err) => {
                error!("Failed to provision public nodes: {err}");
                node_provision_failed = true;
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision UPnP Nodes");
        match self.ansible_provisioner.provision_nodes(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
            NodeType::Upnp,
        ) {
            Ok(()) => {
                println!("Provisioned UPnP nodes");
            }
            Err(err) => {
                error!("Failed to provision UPnP nodes: {err}");
                node_provision_failed = true;
            }
        }

        let private_node_inventory = PrivateNodeProvisionInventory::new(
            &self.ansible_provisioner,
            options.full_cone_private_node_vm_count,
            options.symmetric_private_node_vm_count,
            Some(options.port_restricted_cone_private_node_vm_count),
        )?;

        if private_node_inventory.should_provision_full_cone_private_nodes() {
            match self.ansible_provisioner.provision_full_cone(
                &provision_options,
                Some(genesis_multiaddr.clone()),
                Some(genesis_network_contacts.clone()),
                private_node_inventory.clone(),
                None,
            ) {
                Ok(()) => {
                    println!("Provisioned Full Cone nodes and Gateway");
                }
                Err(err) => {
                    error!("Failed to provision Full Cone nodes and Gateway: {err}");
                    node_provision_failed = true;
                }
            }
        }

        if private_node_inventory.should_provision_port_restricted_cone_private_nodes() {
            match self.ansible_provisioner.provision_port_restricted_cone(
                &provision_options,
                Some(genesis_multiaddr.clone()),
                Some(genesis_network_contacts.clone()),
                private_node_inventory.clone(),
                None,
            ) {
                Ok(()) => {
                    println!("Provisioned Port Restricted Cone nodes and Gateway");
                }
                Err(err) => {
                    error!("Failed to provision Port Restricted Cone nodes and Gateway: {err}");
                    node_provision_failed = true;
                }
            }
        }

        if private_node_inventory.should_provision_symmetric_private_nodes() {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Symmetric NAT Gateway");
            self.ansible_provisioner
                .provision_symmetric_nat_gateway(&provision_options, &private_node_inventory)
                .map_err(|err| {
                    println!("Failed to provision Symmetric NAT gateway {err:?}");
                    err
                })?;

            self.ansible_provisioner
                .print_ansible_run_banner("Provision Symmetric Private Nodes");
            match self.ansible_provisioner.provision_symmetric_private_nodes(
                &mut provision_options,
                Some(genesis_multiaddr.clone()),
                Some(genesis_network_contacts.clone()),
                &private_node_inventory,
            ) {
                Ok(()) => {
                    println!("Provisioned Symmetric private nodes");
                }
                Err(err) => {
                    error!("Failed to provision Symmetric Private nodes: {err}");
                    node_provision_failed = true;
                }
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Uploaders");
        let result = self
            .ansible_provisioner
            .provision_uploaders(
                &provision_options,
                Some(genesis_multiaddr.clone()),
                Some(genesis_network_contacts.clone()),
            )
            .await
            .map_err(|err| {
                println!("Failed to provision Clients {err:?}");
                err
            });
        if let Err(crate::Error::EmptyInventory(AnsibleInventoryType::Clients)) = result {
            println!("No clients were provisioned as part of this deployment.");
        } else if let Err(err) = result {
            return Err(err);
        }
        self.ansible_provisioner
            .print_ansible_run_banner("Provision Downloaders");
        self.ansible_provisioner
            .provision_downloaders(
                &provision_options,
                Some(genesis_multiaddr.clone()),
                Some(genesis_network_contacts.clone()),
            )
            .await
            .map_err(|err| {
                println!("Failed to provision downloaders {err:?}");
                err
            })?;

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
