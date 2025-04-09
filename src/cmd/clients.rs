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
    clients::{ClientsDeployBuilder, ClientsDeployOptions},
    inventory::DeploymentInventoryService,
    upscale::UpscaleOptions,
    EvmDetails, TestnetDeployBuilder,
};

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ClientsCommands {
    /// Clean a deployed client environment.
    Clean {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Deploy a new client environment.
    Deploy {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// Supply a version number for the ant binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        ant_version: Option<String>,
        /// The branch of the Github repository to build from.
        ///
        /// If used, the ant binary will be built from this branch. It is typically used for testing
        /// changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        branch: Option<String>,
        /// Specify the chunk size for the custom binaries using a 64-bit integer.
        ///
        /// This option only applies if the --branch and --repo-owner arguments are used.
        #[clap(long, value_parser = parse_chunk_size)]
        chunk_size: Option<u64>,
        /// Provide environment variables for the antnode RPC client.
        ///
        /// This is useful to set the client's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --client-env CLIENT_LOG=all,RUST_LOG=debug
        #[clap(name = "client-env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        client_env_variables: Option<Vec<(String, String)>>,
        /// The number of client VMs to create.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        client_vm_count: Option<u16>,
        /// Override the size of the client VMs.
        #[clap(long)]
        client_vm_size: Option<String>,
        /// Set to disable the download-verifier downloader on the VMs.
        #[clap(long)]
        disable_download_verifier: bool,
        /// Set to disable the performance-verifier downloader on the VMs.
        #[clap(long)]
        disable_performance_verifier: bool,
        /// Set to disable the random-verifier downloader on the VMs.
        #[clap(long)]
        disable_random_verifier: bool,
        /// Set to disable Telegraf metrics collection on all nodes.
        #[clap(long)]
        disable_telegraf: bool,
        /// Set to disable uploaders on the VMs. Use this when you only want to run downloader services.
        #[clap(long)]
        disable_uploaders: bool,
        /// The type of deployment.
        ///
        /// Possible values are 'development', 'production' or 'staging'. The value used will
        /// determine the sizes of VMs, the number of VMs, and the number of nodes deployed on
        /// them. The specification will increase in size from development, to staging, to
        /// production.
        ///
        /// The default is 'development'.
        #[clap(long, default_value_t = EnvironmentType::Development, value_parser = parse_deployment_type, verbatim_doc_comment)]
        environment_type: EnvironmentType,
        /// The address of the data payments contract.
        #[arg(long)]
        evm_data_payments_address: Option<String>,
        /// The EVM network type to use for the deployment.
        ///
        /// Possible values are 'arbitrum-one' or 'custom'.
        ///
        /// If not used, the default is 'arbitrum-one'.
        #[clap(long, default_value = "arbitrum-one", value_parser = parse_evm_network)]
        evm_network_type: EvmNetwork,
        /// The address of the payment token contract.
        #[arg(long)]
        evm_payment_token_address: Option<String>,
        /// The RPC URL for the EVM network.
        ///
        /// This argument only applies if the EVM network type is 'custom'.
        #[arg(long)]
        evm_rpc_url: Option<String>,
        /// The expected hash of the file to download for verification.
        ///
        /// This is only used when --file-address is provided.
        #[arg(long)]
        expected_hash: Option<String>,
        /// The expected size of the file to download for verification.
        ///
        /// This is only used when --file-address is provided.
        #[arg(long)]
        expected_size: Option<u64>,
        /// The address of the file to download for verification.
        ///
        /// If provided, both --expected-hash and --expected-size must also be provided.
        #[arg(long)]
        file_address: Option<String>,
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The secret key for the wallet that will fund all the ANT instances.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The amount of gas to initially transfer to each ANT instance, in U256
        ///
        /// 1 ETH = 1_000_000_000_000_000_000. Defaults to 0.1 ETH
        #[arg(long)]
        initial_gas: Option<U256>,
        /// The amount of tokens to initially transfer to each ANT instance, in U256
        ///
        /// 1 Token = 1_000_000_000_000_000_000. Defaults to 100 token.
        #[arg(long)]
        initial_tokens: Option<U256>,
        /// Maximum number of uploads to perform before stopping.
        ///
        /// If not specified, uploaders will continue uploading indefinitely.
        #[clap(long)]
        max_uploads: Option<u32>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the network ID to use for the node services. This is used to partition the network and will not allow
        /// nodes with different network IDs to join.
        ///
        /// By default, the network ID is set to 1, which represents the mainnet.
        #[clap(long, verbatim_doc_comment)]
        network_id: Option<u8>,
        /// The networks contacts URL from an existing network.
        #[arg(long)]
        network_contacts_url: Option<String>,
        /// A peer from an existing network that the Ant client can connect to.
        ///
        /// Should be in the form of a multiaddr.
        #[arg(long)]
        peer: Option<String>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The region to deploy to.
        ///
        /// Defaults to "lon1" for Digital Ocean.
        #[clap(long, default_value = "lon1")]
        region: String,
        /// The owner/org of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this repository. It is typically used for
        /// testing changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        repo_owner: Option<String>,
        /// The desired number of uploaders per client VM.
        #[clap(long, default_value_t = 1)]
        uploaders_count: u16,
        /// Pre-funded wallet secret keys to use for the ANT instances.
        ///
        /// Can be specified multiple times, once for each ANT instance.
        /// If provided, the number of keys must match the total number of uploaders (VM count * uploaders per VM).
        /// When using this option, the deployer will not fund the wallets.
        #[clap(long, value_name = "SECRET_KEY", number_of_values = 1)]
        wallet_secret_key: Vec<String>,
    },
    /// Enable downloaders on all client VMs in an environment.
    EnableDownloaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Start all downloaders on all client VMs in an environment.
    StartDownloaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Start all uploaders on all client VMs in an environment.
    StartUploaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Stop all downloaders on all client VMs in an environment.
    StopDownloaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Stop all uploaders on all client VMs in an environment.
    StopUploaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Upgrade the Ant binary on all client VMs in an environment.
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
    /// Upscale clients for an existing environment.
    Upscale {
        /// Supply a version number for the autonomi binary to be used for new Client VMs.
        ///
        /// There should be no 'v' prefix.
        #[arg(long, verbatim_doc_comment)]
        autonomi_version: String,
        /// The desired number of Client VMs to be running after the upscale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_client_vm_count: Option<u16>,
        /// The desired number of uploaders to be running after the upscale.
        ///
        /// If you want each Client VM to run multiple uploader services, specify the total desired count.
        #[clap(long, verbatim_doc_comment)]
        desired_uploaders_count: Option<u16>,
        /// Set to disable the download-verifier downloader on the VMs.
        #[clap(long)]
        disable_download_verifier: bool,
        /// Set to disable the performance-verifier downloader on the VMs.
        #[clap(long)]
        disable_performance_verifier: bool,
        /// Set to disable the random-verifier downloader on the VMs.
        #[clap(long)]
        disable_random_verifier: bool,
        /// The secret key for the wallet that will fund all the ANT instances.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The amount of gas tokens to transfer to each ANT instance.
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

pub async fn handle_clients_command(cmd: ClientsCommands) -> Result<()> {
    match cmd {
        ClientsCommands::EnableDownloaders { name, provider } => {
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
                AnsiblePlaybook::Downloaders,
                AnsibleInventoryType::Clients,
                None,
            )?;
            Ok(())
        }
        ClientsCommands::Clean { name, provider } => {
            println!("Cleaning Client environment '{}'...", name);
            let client_deployer = ClientsDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            client_deployer.clean().await?;
            println!("Client environment '{}' cleaned", name);
            Ok(())
        }
        ClientsCommands::Deploy {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_telegraf,
            disable_download_verifier,
            disable_random_verifier,
            disable_performance_verifier,
            disable_uploaders,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            forks,
            funding_wallet_secret_key,
            initial_gas,
            initial_tokens,
            max_uploads,
            name,
            network_id,
            network_contacts_url,
            peer,
            provider,
            region,
            repo_owner,
            uploaders_count,
            wallet_secret_key,
            file_address,
            expected_hash,
            expected_size,
        } => {
            if (branch.is_some() && repo_owner.is_none())
                || (branch.is_none() && repo_owner.is_some())
            {
                return Err(eyre!(
                    "Both --branch and --repo-owner must be provided together"
                ));
            }

            if ant_version.is_some() && (branch.is_some() || repo_owner.is_some()) {
                return Err(eyre!("Cannot specify both version and branch/repo-owner"));
            }

            if evm_network_type == EvmNetwork::Custom {
                if evm_data_payments_address.is_none() {
                    return Err(eyre!(
                        "Data payments address must be provided for custom EVM network"
                    ));
                }
                if evm_payment_token_address.is_none() {
                    return Err(eyre!(
                        "Payment token address must be provided for custom EVM network"
                    ));
                }
                if evm_rpc_url.is_none() {
                    return Err(eyre!("RPC URL must be provided for custom EVM network"));
                }
            }

            if funding_wallet_secret_key.is_none()
                && evm_network_type != EvmNetwork::Anvil
                && wallet_secret_key.is_empty()
            {
                return Err(eyre!(
                    "For Sepolia or Arbitrum One, either a funding wallet secret key or pre-funded wallet secret keys are required"
                ));
            }

            if file_address.is_some() && (expected_hash.is_none() || expected_size.is_none()) {
                return Err(eyre!(
                    "When --file-address is provided, both --expected-hash and --expected-size must also be provided"
                ));
            }

            let total_uploaders = client_vm_count.unwrap_or(1) as usize * uploaders_count as usize;
            if !wallet_secret_key.is_empty() && wallet_secret_key.len() != total_uploaders {
                return Err(eyre!(
                    "Number of wallet secret keys ({}) must match total number of uploaders ({})",
                    wallet_secret_key.len(),
                    total_uploaders,
                ));
            }

            let binary_option =
                get_binary_option(branch, repo_owner, ant_version, None, None, None).await?;

            let mut builder = ClientsDeployBuilder::new();
            builder
                .ansible_verbose_mode(ansible_verbose)
                .deployment_type(environment_type.clone())
                .environment_name(&name)
                .provider(provider);
            if let Some(forks_value) = forks {
                builder.ansible_forks(forks_value);
            }
            let client_deployer = builder.build()?;
            client_deployer.init().await?;

            let inventory_service = DeploymentInventoryService::from(&client_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_client_inventory(
                    &name,
                    &region,
                    true,
                    Some(binary_option.clone()),
                )
                .await?;
            let evm_details = EvmDetails {
                network: evm_network_type,
                data_payments_address: evm_data_payments_address,
                payment_token_address: evm_payment_token_address,
                rpc_url: evm_rpc_url,
            };

            let options = ClientsDeployOptions {
                binary_option,
                chunk_size,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                enable_download_verifier: !disable_download_verifier,
                enable_random_verifier: !disable_random_verifier,
                enable_performance_verifier: !disable_performance_verifier,
                enable_telegraf: !disable_telegraf,
                enable_uploaders: !disable_uploaders,
                environment_type,
                evm_details,
                expected_hash,
                expected_size,
                file_address,
                funding_wallet_secret_key,
                initial_gas,
                initial_tokens,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads,
                name: name.clone(),
                network_id,
                network_contacts_url,
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer,
                uploaders_count,
                wallet_secret_keys: if wallet_secret_key.is_empty() {
                    None
                } else {
                    Some(wallet_secret_key)
                },
            };

            client_deployer.deploy(options).await?;

            println!("Client deployment for '{}' completed successfully", name);
            Ok(())
        }
        ClientsCommands::StartDownloaders { name, provider } => {
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
                AnsiblePlaybook::StartDownloaders,
                AnsibleInventoryType::Clients,
                None,
            )?;
            Ok(())
        }
        ClientsCommands::StartUploaders { name, provider } => {
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
                AnsibleInventoryType::Clients,
                None,
            )?;
            Ok(())
        }
        ClientsCommands::StopDownloaders { name, provider } => {
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
                AnsiblePlaybook::StopDownloaders,
                AnsibleInventoryType::Clients,
                None,
            )?;
            Ok(())
        }
        ClientsCommands::StopUploaders { name, provider } => {
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
                AnsibleInventoryType::Clients,
                None,
            )?;
            Ok(())
        }
        ClientsCommands::Upgrade {
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
                AnsiblePlaybook::UpgradeClients,
                AnsibleInventoryType::Clients,
                Some(extra_vars.build()),
            )?;

            Ok(())
        }
        ClientsCommands::Upscale {
            autonomi_version,
            desired_client_vm_count,
            desired_uploaders_count,
            disable_download_verifier,
            disable_random_verifier,
            disable_performance_verifier,
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

            println!("Upscaling Clients...");
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
                .upscale_clients(&UpscaleOptions {
                    ansible_verbose: false,
                    current_inventory: inventory,
                    desired_client_vm_count,
                    desired_full_cone_private_node_count: None,
                    desired_full_cone_private_node_vm_count: None,
                    desired_node_count: None,
                    desired_node_vm_count: None,
                    desired_peer_cache_node_count: None,
                    desired_peer_cache_node_vm_count: None,
                    desired_symmetric_private_node_count: None,
                    desired_symmetric_private_node_vm_count: None,
                    desired_uploaders_count,
                    enable_download_verifier: !disable_download_verifier,
                    enable_random_verifier: !disable_random_verifier,
                    enable_performance_verifier: !disable_performance_verifier,
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
