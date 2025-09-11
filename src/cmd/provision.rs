// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{get_options_from_s3, OptionsType};
use clap::{arg, Subcommand};
use color_eyre::Result;
use sn_testnet_deploy::{
    ansible::provisioning::{PrivateNodeProvisionInventory, ProvisionOptions},
    deploy::DeployOptions,
    get_bootstrap_cache_url, get_genesis_multiaddr,
    inventory::DeploymentInventoryService,
    CloudProvider, NodeType, TestnetDeployBuilder,
};

#[derive(Subcommand, Debug)]
pub enum ProvisionCommands {
    /// Provision clients for an environment
    #[clap(name = "clients")]
    Clients {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision full cone private nodes for an environment
    #[clap(name = "full-cone-private-nodes")]
    FullConePrivateNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision port restricted cone private nodes for an environment
    #[clap(name = "port-restricted-cone-private-nodes")]
    PortRestrictedConePrivateNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision generic nodes for an environment
    #[clap(name = "generic-nodes")]
    GenericNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision peer cache nodes for an environment
    #[clap(name = "peer-cache-nodes")]
    PeerCacheNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision symmetric private nodes for an environment
    #[clap(name = "symmetric-private-nodes")]
    SymmetricPrivateNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Provision UPnP nodes for an environment
    #[clap(name = "upnp-nodes")]
    UpnpNodes {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
}

async fn init_provision(
    name: &str,
) -> Result<(
    DeployOptions,
    ProvisionOptions,
    sn_testnet_deploy::ansible::provisioning::AnsibleProvisioner,
    sn_testnet_deploy::ssh::SshClient,
)> {
    let deploy_options: DeployOptions = get_options_from_s3(name, OptionsType::Deploy).await?;
    let provision_options: ProvisionOptions =
        get_options_from_s3(name, OptionsType::Provision).await?;

    let mut builder = TestnetDeployBuilder::default();
    builder
        .ansible_verbose_mode(false)
        .deployment_type(deploy_options.environment_type.clone())
        .environment_name(name)
        .provider(CloudProvider::DigitalOcean);
    let testnet_deployer = builder.build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    inventory_service
        .generate_or_retrieve_inventory(name, true, Some(deploy_options.binary_option.clone()))
        .await?;

    let provisioner = testnet_deployer.ansible_provisioner;
    let ssh_client = testnet_deployer.ssh_client;

    Ok((deploy_options, provision_options, provisioner, ssh_client))
}

async fn handle_provision_nodes(name: String, node_type: NodeType) -> Result<()> {
    let (deploy_options, mut provision_options, provisioner, ssh_client) =
        init_provision(&name).await?;

    let (genesis_multiaddr, genesis_ip) =
        get_genesis_multiaddr(&provisioner.ansible_runner, &ssh_client).map_err(|err| {
            println!("Failed to get genesis multiaddr {err:?}");
            err
        })?;
    let genesis_network_contacts = get_bootstrap_cache_url(&genesis_ip);

    let private_node_inventory = PrivateNodeProvisionInventory::new(
        &provisioner,
        deploy_options.full_cone_private_node_vm_count,
        deploy_options.symmetric_private_node_vm_count,
        Some(deploy_options.port_restricted_cone_private_node_vm_count),
    )?;

    match node_type {
        NodeType::FullConePrivateNode => {
            if private_node_inventory.should_provision_full_cone_private_nodes() {
                provisioner.provision_full_cone(
                    &provision_options,
                    Some(genesis_multiaddr),
                    Some(genesis_network_contacts),
                    private_node_inventory,
                    None,
                )?;
            } else {
                println!("Full cone private nodes have not been requested for this environment");
            }
        }
        NodeType::PortRestrictedConePrivateNode => {
            if private_node_inventory.should_provision_port_restricted_cone_private_nodes() {
                provisioner.provision_port_restricted_cone(
                    &provision_options,
                    Some(genesis_multiaddr),
                    Some(genesis_network_contacts),
                    private_node_inventory,
                    None,
                )?;
            } else {
                println!("Port restricted cone private nodes have not been requested for this environment");
            }
        }
        NodeType::SymmetricPrivateNode => {
            if private_node_inventory.should_provision_symmetric_private_nodes() {
                provisioner.print_ansible_run_banner("Provision Symmetric NAT Gateway");
                provisioner
                    .provision_symmetric_nat_gateway(&provision_options, &private_node_inventory)
                    .map_err(|err| {
                        println!("Failed to provision Symmetric NAT gateway {err:?}");
                        err
                    })?;

                provisioner.print_ansible_run_banner("Provision Symmetric Private Nodes");
                provisioner.provision_symmetric_private_nodes(
                    &mut provision_options,
                    Some(genesis_multiaddr),
                    Some(genesis_network_contacts),
                    &private_node_inventory,
                )?;
            } else {
                println!("Symmetric private nodes have not been requested for this environment");
            }
        }
        _ => {
            provisioner.print_ansible_run_banner(&format!("Provision {node_type} Nodes"));
            provisioner.provision_nodes(
                &provision_options,
                Some(genesis_multiaddr),
                Some(genesis_network_contacts),
                node_type,
            )?;
        }
    }

    Ok(())
}

pub async fn handle_provision_peer_cache_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::PeerCache).await
}

pub async fn handle_provision_generic_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::Generic).await
}

pub async fn handle_provision_symmetric_private_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::SymmetricPrivateNode).await
}

pub async fn handle_provision_full_cone_private_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::FullConePrivateNode).await
}

pub async fn handle_provision_port_restricted_cone_private_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::PortRestrictedConePrivateNode).await
}

pub async fn handle_provision_upnp_nodes(name: String) -> Result<()> {
    handle_provision_nodes(name, NodeType::Upnp).await
}

pub async fn handle_provision_clients(name: String) -> Result<()> {
    let (_, provision_options, provisioner, ssh_client) = init_provision(&name).await?;
    let (genesis_multiaddr, genesis_ip) =
        get_genesis_multiaddr(&provisioner.ansible_runner, &ssh_client).map_err(|err| {
            println!("Failed to get genesis multiaddr {err:?}");
            err
        })?;
    let genesis_network_contacts = get_bootstrap_cache_url(&genesis_ip);

    provisioner.print_ansible_run_banner("Provision Clients");
    provisioner
        .provision_uploaders(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
        )
        .await
        .map_err(|err| {
            println!("Failed to provision clients: {err:?}");
            err
        })?;

    provisioner.print_ansible_run_banner("Provision Downloaders");
    provisioner
        .provision_downloaders(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
        )
        .await
        .map_err(|err| {
            println!("Failed to provision downloaders: {err:?}");
            err
        })?;

    Ok(())
}
