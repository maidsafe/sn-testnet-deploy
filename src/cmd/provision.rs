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
    /// Provision full cone private nodes for an environment
    #[clap(name = "full-cone-private-nodes")]
    FullConePrivateNodes {
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
}

async fn handle_provision_nodes(name: String, node_type: NodeType) -> Result<()> {
    println!("Retrieving deployment options for {}", name);

    let deploy_options: DeployOptions = get_options_from_s3(&name, OptionsType::Deploy).await?;
    let mut provision_options: ProvisionOptions =
        get_options_from_s3(&name, OptionsType::Provision).await?;

    let mut builder = TestnetDeployBuilder::default();
    builder
        .ansible_verbose_mode(false)
        .deployment_type(deploy_options.environment_type.clone())
        .environment_name(&name)
        .provider(CloudProvider::DigitalOcean);
    let testnet_deployer = builder.build()?;

    let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
    inventory_service
        .generate_or_retrieve_inventory(&name, true, Some(deploy_options.binary_option.clone()))
        .await?;

    let provisioner = testnet_deployer.ansible_provisioner;
    let ssh_client = testnet_deployer.ssh_client;

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
    )?;

    if private_node_inventory.should_provision_full_cone_private_nodes() {
        provisioner.provision_full_cone(
            &provision_options,
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
            private_node_inventory.clone(),
            None,
        )?;
        return Ok(());
    }

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
            Some(genesis_multiaddr.clone()),
            Some(genesis_network_contacts.clone()),
            &private_node_inventory,
        )?;
        return Ok(());
    }

    provisioner.print_ansible_run_banner(&format!("Provision {} Nodes", node_type));
    provisioner.provision_nodes(
        &provision_options,
        Some(genesis_multiaddr.clone()),
        Some(genesis_network_contacts.clone()),
        node_type,
    )?;

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
