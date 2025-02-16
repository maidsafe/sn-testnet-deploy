use super::{get_options_from_s3, OptionsType};
use color_eyre::Result;
use sn_testnet_deploy::{
    ansible::provisioning::ProvisionOptions, deploy::DeployOptions, get_bootstrap_cache_url,
    get_genesis_multiaddr, inventory::DeploymentInventoryService, CloudProvider, NodeType,
    TestnetDeployBuilder,
};

pub async fn handle_provision_peer_cache_nodes(name: String) -> Result<()> {
    println!("Retrieving deployment options for {}", name);

    let deploy_options: DeployOptions = get_options_from_s3(&name, OptionsType::Deploy).await?;
    let provision_options: ProvisionOptions =
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

    let provisoner = testnet_deployer.ansible_provisioner;
    let ssh_client = testnet_deployer.ssh_client;

    let (genesis_multiaddr, genesis_ip) =
        get_genesis_multiaddr(&provisoner.ansible_runner, &ssh_client).map_err(|err| {
            println!("Failed to get genesis multiaddr {err:?}");
            err
        })?;
    let genesis_network_contacts = get_bootstrap_cache_url(&genesis_ip);

    provisoner.print_ansible_run_banner("Provision Peer Cache Nodes");
    provisoner.provision_nodes(
        &provision_options,
        Some(genesis_multiaddr.clone()),
        Some(genesis_network_contacts.clone()),
        NodeType::PeerCache,
    )?;

    Ok(())
}