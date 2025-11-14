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
        /// Comma-separated list of data addresses to track with chunk trackers.
        #[arg(long, value_delimiter = ',')]
        chunk_tracker_data_addresses: Vec<String>,
        /// The number of chunk tracker services to run per client VM.
        #[clap(long, default_value_t = 1)]
        chunk_tracker_services: u16,
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// Specify the network ID for the ant binary.
        ///
        /// This is used to ensure the client connects to the correct network.
        ///
        /// For a production deployment, use 1.
        ///
        /// For an alpha deployment, use 2.
        ///
        /// For a testnet deployment, use anything between 3 and 255.
        #[clap(long, verbatim_doc_comment)]
        network_id: u8,
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
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// Set to start chunk tracker services immediately after provisioning.
        #[clap(long)]
        start_chunk_trackers: bool,
        /// Set to start the download-verifier downloader on the VMs.
        #[clap(long)]
        start_download_verifier: bool,
        /// Set to start the performance-verifier downloader on the VMs.
        #[clap(long)]
        start_performance_verifier: bool,
        /// Set to start the random-verifier downloader on the VMs.
        #[clap(long)]
        start_random_verifier: bool,
        /// Set to start uploaders on the VMs immediately after provisioning.
        #[clap(long)]
        start_uploaders: bool,
        /// The desired number of uploaders per client VM.
        #[clap(long, default_value_t = 1)]
        uploaders_count: u16,
        /// The interval between uploads in seconds.
        ///
        /// This controls how long the random uploader waits between uploads.
        #[clap(long, default_value_t = 10)]
        upload_interval: u16,
        /// Specify the size in megabytes for the random files generated by uploaders.
        ///
        /// Default value is 100MB.
        #[clap(long, default_value_t = 100)]
        upload_size: u16,
        /// Pre-funded wallet secret keys to use for the ANT instances.
        ///
        /// Can be specified multiple times, once for each ANT instance.
        /// If provided, the number of keys must match the total number of uploaders (VM count * uploaders per VM).
        /// When using this option, the deployer will not fund the wallets.
        #[clap(long, value_name = "SECRET_KEY", number_of_values = 1)]
        wallet_secret_key: Vec<String>,
    },
    /// Deploy chunk tracker services on client VMs.
    DeployChunkTrackers {
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
        /// Comma-separated list of data addresses to track with chunk trackers.
        #[arg(long, value_delimiter = ',')]
        chunk_tracker_data_addresses: Vec<String>,
        /// The number of chunk tracker services to run per client VM.
        #[clap(long, default_value_t = 1)]
        chunk_tracker_services: u16,
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the network ID for the ant binary.
        ///
        /// This is used to ensure the client connects to the correct network.
        ///
        /// For a production deployment, use 1.
        ///
        /// For an alpha deployment, use 2.
        ///
        /// For a testnet deployment, use anything between 3 and 255.
        #[clap(long, verbatim_doc_comment)]
        network_id: u8,
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
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// Set to start chunk tracker services immediately after provisioning.
        #[clap(long)]
        start_chunk_trackers: bool,
    },
    /// Deploy data retrieval service on client VMs.
    DeployDataRetrieval {
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
        /// Provide environment variables for the ant client.
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the network ID for the ant binary.
        ///
        /// This is used to ensure the client connects to the correct network.
        ///
        /// For a production deployment, use 1.
        ///
        /// For an alpha deployment, use 2.
        ///
        /// For a testnet deployment, use anything between 3 and 255.
        #[clap(long, verbatim_doc_comment)]
        network_id: u8,
        /// The networks contacts URL from an existing network.
        #[arg(long)]
        network_contacts_url: Option<String>,
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
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// Set to start data retrieval service immediately after provisioning.
        #[clap(long)]
        start_data_retrieval: bool,
    },
    /// Deploy service(s) for repairing individual file addresses on client VMs
    DeployRepairFiles {
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
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
        /// The number of repair services to deploy.
        ///
        /// Default is 1.
        #[clap(long)]
        service_count: Option<u16>,
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// Set to start the repair service immediately after provisioning.
        #[clap(long)]
        start_repair_service: bool,
        /// The secret key for the wallet that will fund chunks that get uploaded for repaired
        /// addresses.
        #[clap(long)]
        wallet_secret_key: String,
    },
    /// Deploy a scan repairing service on client VMs
    DeployScanRepair {
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
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
        /// The scanning frequency for the repair command.
        #[clap(long)]
        scan_frequency: Option<u64>,
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// The interval between repair commands.
        #[clap(long)]
        sleep_interval: Option<u64>,
        /// Set to start the repair service immediately after provisioning.
        #[clap(long)]
        start_service: bool,
        /// The secret key for the wallet that will fund chunks that get uploaded for repaired
        /// addresses.
        #[clap(long)]
        wallet_secret_key: String,
    },
    /// Deploy a new static downloader environment.
    DeployStaticDownloaders {
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
        /// The batch size for the delayed verifier downloader.
        #[clap(long)]
        delayed_verifier_batch_size: Option<u16>,
        /// The quorum value for the delayed verifier downloader.
        /// Can be "majority", "all", or a custom number.
        #[clap(long)]
        delayed_verifier_quorum_value: Option<String>,
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// The address of the file to download for verification.
        #[arg(long)]
        file_address: Option<String>,
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the network ID for the ant binary.
        ///
        /// This is used to ensure the client connects to the correct network.
        ///
        /// For a production deployment, use 1.
        ///
        /// For an alpha deployment, use 2.
        ///
        /// For a testnet deployment, use anything between 3 and 255.
        #[clap(long, verbatim_doc_comment)]
        network_id: u8,
        /// The networks contacts URL from an existing network.
        #[arg(long)]
        network_contacts_url: Option<String>,
        /// A peer from an existing network that the Ant client can connect to.
        ///
        /// Should be in the form of a multiaddr.
        #[arg(long)]
        peer: Option<String>,
        /// The batch size for the performance verifier downloader.
        #[clap(long)]
        performance_verifier_batch_size: Option<u16>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The batch size for the random verifier downloader.
        #[clap(long)]
        random_verifier_batch_size: Option<u16>,
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
        /// Skip building the autonomi binaries if they were built during a previous run of the deployer using the same
        /// --branch, --repo-owner and --name arguments.
        ///
        /// This is useful to re-run any failed deployments without rebuilding the binaries.
        #[arg(long, default_value_t = false)]
        skip_binary_build: bool,
        /// Sleep duration in seconds for downloader services.
        #[arg(long)]
        sleep_duration: Option<u16>,
        /// Set to start the delayed-verifier downloader on the VMs.
        #[clap(long)]
        start_delayed_verifier: bool,
        /// Set to start the performance-verifier downloader on the VMs.
        #[clap(long)]
        start_performance_verifier: bool,
        /// Set to start the random-verifier downloader on the VMs.
        #[clap(long)]
        start_random_verifier: bool,
    },
    /// Deploy a new static uploader environment.
    DeployStaticUploader {
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
        /// Set to disable metrics collection on all nodes.
        #[clap(long)]
        disable_metrics: bool,
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
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the network ID for the ant binary.
        ///
        /// This is used to ensure the client connects to the correct network.
        ///
        /// For a production deployment, use 1.
        ///
        /// For an alpha deployment, use 2.
        ///
        /// For a testnet deployment, use anything between 3 and 255.
        #[clap(long, verbatim_doc_comment)]
        network_id: u8,
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
        /// The batch size for the uploads.
        #[clap(long)]
        upload_batch_size: Option<u16>,
        /// The secret key for the wallet with the funds for uploading.
        #[arg(long, verbatim_doc_comment)]
        wallet_secret_key: String,
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
    /// Fetch scan repair results from all client VMs in an environment.
    FetchScanRepairResults {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Start all chunk trackers on all client VMs in an environment.
    StartChunkTrackers {
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
    /// Stop all chunk trackers on all client VMs in an environment.
    StopChunkTrackers {
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
        /// Set to start the download-verifier downloader on the VMs.
        #[clap(long)]
        start_download_verifier: bool,
        /// Set to start the performance-verifier downloader on the VMs.
        #[clap(long)]
        start_performance_verifier: bool,
        /// Set to start the random-verifier downloader on the VMs.
        #[clap(long)]
        start_random_verifier: bool,
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
        ClientsCommands::FetchScanRepairResults { name, provider } => {
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
                AnsiblePlaybook::FetchScanRepairResults,
                AnsibleInventoryType::Clients,
                Some(format!("{{ \"env_name\": \"{name}\" }}")),
            )?;

            println!("Scan repair results fetched successfully.");
            println!("Files saved to: scan-repair-results/{}/", name);
            Ok(())
        }
        ClientsCommands::Clean { name, provider } => {
            println!("Cleaning Client environment '{name}'...");
            let client_deployer = ClientsDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            client_deployer.clean().await?;
            println!("Client environment '{name}' cleaned");
            Ok(())
        }
        ClientsCommands::Deploy {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            chunk_tracker_data_addresses,
            chunk_tracker_services,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            expected_hash,
            expected_size,
            file_address,
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
            skip_binary_build,
            start_chunk_trackers,
            start_download_verifier,
            start_random_verifier,
            start_performance_verifier,
            start_uploaders,
            uploaders_count,
            upload_interval,
            upload_size,
            wallet_secret_key,
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

            if start_uploaders
                && funding_wallet_secret_key.is_none()
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                chunk_tracker_data_addresses,
                chunk_tracker_services,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
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
                network_contacts_url,
                network_id: Some(network_id),
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                run_chunk_trackers_provision: true,
                run_data_retrieval_provision: false,
                run_downloaders_provision: true,
                run_repair_files_provision: false,
                run_scan_repair_provision: false,
                run_uploaders_provision: true,
                scan_frequency: None,
                sleep_duration: None,
                sleep_interval: None,
                start_chunk_trackers,
                start_data_retrieval: false,
                start_delayed_verifier: start_download_verifier,
                start_performance_verifier,
                start_random_verifier,
                start_repair_service: false,
                start_uploaders,
                upload_batch_size: None,
                uploaders_count,
                upload_interval,
                upload_size: Some(upload_size),
                wallet_secret_keys: if wallet_secret_key.is_empty() {
                    None
                } else {
                    Some(wallet_secret_key)
                },
            };

            client_deployer.deploy(options).await?;

            println!("Client deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployChunkTrackers {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            chunk_tracker_data_addresses,
            chunk_tracker_services,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            forks,
            name,
            network_id,
            network_contacts_url,
            peer,
            provider,
            region,
            repo_owner,
            skip_binary_build,
            start_chunk_trackers,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                chunk_tracker_data_addresses,
                chunk_tracker_services,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address: None,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url,
                network_id: Some(network_id),
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                run_chunk_trackers_provision: true,
                run_data_retrieval_provision: false,
                run_downloaders_provision: false,
                run_repair_files_provision: false,
                run_scan_repair_provision: false,
                run_uploaders_provision: false,
                scan_frequency: None,
                sleep_duration: None,
                sleep_interval: None,
                start_chunk_trackers,
                start_data_retrieval: false,
                start_delayed_verifier: false,
                start_performance_verifier: false,
                start_random_verifier: false,
                start_repair_service: false,
                start_uploaders: false,
                upload_batch_size: None,
                uploaders_count: 0,
                upload_interval: 0,
                upload_size: None,
                wallet_secret_keys: None,
            };

            client_deployer.deploy(options).await?;

            println!("Chunk tracker deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployDataRetrieval {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            forks,
            name,
            network_id,
            network_contacts_url,
            provider,
            region,
            repo_owner,
            skip_binary_build,
            start_data_retrieval,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                chunk_tracker_data_addresses: vec![],
                chunk_tracker_services: 0,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address: None,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url,
                network_id: Some(network_id),
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer: None,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                run_chunk_trackers_provision: false,
                run_data_retrieval_provision: true,
                run_downloaders_provision: false,
                run_repair_files_provision: false,
                run_scan_repair_provision: false,
                run_uploaders_provision: false,
                scan_frequency: None,
                sleep_duration: None,
                sleep_interval: None,
                start_chunk_trackers: false,
                start_data_retrieval,
                start_delayed_verifier: false,
                start_performance_verifier: false,
                start_random_verifier: false,
                start_repair_service: false,
                start_uploaders: false,
                upload_batch_size: None,
                uploaders_count: 0,
                upload_interval: 0,
                upload_size: None,
                wallet_secret_keys: None,
            };

            client_deployer.deploy(options).await?;

            println!("Data retrieval deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployRepairFiles {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            forks,
            name,
            provider,
            region,
            repo_owner,
            service_count,
            skip_binary_build,
            start_repair_service,
            wallet_secret_key,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                network: EvmNetwork::ArbitrumOne,
                data_payments_address: None,
                payment_token_address: None,
                rpc_url: None,
            };

            let options = ClientsDeployOptions {
                binary_option,
                chunk_size,
                chunk_tracker_data_addresses: Vec::new(),
                chunk_tracker_services: 0,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address: None,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url: None,
                network_id: None,
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer: None,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: service_count.unwrap_or(1),
                run_chunk_trackers_provision: false,
                run_data_retrieval_provision: false,
                run_downloaders_provision: false,
                run_repair_files_provision: true,
                run_scan_repair_provision: false,
                run_uploaders_provision: false,
                scan_frequency: None,
                sleep_duration: None,
                sleep_interval: None,
                start_chunk_trackers: false,
                start_data_retrieval: false,
                start_delayed_verifier: false,
                start_performance_verifier: false,
                start_random_verifier: false,
                start_repair_service,
                start_uploaders: false,
                upload_batch_size: None,
                uploaders_count: 0,
                upload_interval: 0,
                upload_size: None,
                wallet_secret_keys: Some(vec![
                    wallet_secret_key;
                    client_vm_count.unwrap_or(1) as usize
                ]),
            };

            client_deployer.deploy(options).await?;

            println!("Repair files deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployScanRepair {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            forks,
            name,
            provider,
            region,
            repo_owner,
            scan_frequency,
            skip_binary_build,
            sleep_interval,
            start_service,
            wallet_secret_key,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                network: EvmNetwork::ArbitrumOne,
                data_payments_address: None,
                payment_token_address: None,
                rpc_url: None,
            };

            let options = ClientsDeployOptions {
                binary_option,
                chunk_size,
                chunk_tracker_data_addresses: Vec::new(),
                chunk_tracker_services: 0,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address: None,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url: None,
                network_id: None,
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer: None,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                run_chunk_trackers_provision: false,
                run_data_retrieval_provision: false,
                run_downloaders_provision: false,
                run_repair_files_provision: false,
                run_scan_repair_provision: true,
                run_uploaders_provision: false,
                scan_frequency,
                sleep_duration: None,
                sleep_interval,
                start_chunk_trackers: false,
                start_data_retrieval: false,
                start_delayed_verifier: false,
                start_performance_verifier: false,
                start_random_verifier: false,
                start_repair_service: start_service,
                start_uploaders: false,
                upload_batch_size: None,
                uploaders_count: 0,
                upload_interval: 0,
                upload_size: None,
                wallet_secret_keys: Some(vec![
                    wallet_secret_key;
                    client_vm_count.unwrap_or(1) as usize
                ]),
            };

            client_deployer.deploy(options).await?;

            println!("Scan repair deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployStaticDownloaders {
            ansible_verbose,
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            delayed_verifier_batch_size,
            delayed_verifier_quorum_value,
            disable_metrics,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            file_address,
            forks,
            name,
            network_id,
            network_contacts_url,
            peer,
            performance_verifier_batch_size,
            provider,
            random_verifier_batch_size,
            region,
            repo_owner,
            skip_binary_build,
            sleep_duration,
            start_delayed_verifier,
            start_performance_verifier,
            start_random_verifier,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                skip_binary_build,
            )
            .await?;

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
                chunk_tracker_data_addresses: Vec::new(),
                chunk_tracker_services: 1,
                client_env_variables,
                client_vm_count,
                client_vm_size: client_vm_size
                    .or_else(|| Some("s-8vcpu-32gb-640gb-intel".to_string())),
                current_inventory: inventory,
                delayed_verifier_batch_size,
                delayed_verifier_quorum_value,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url,
                network_id: Some(network_id),
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer,
                performance_verifier_batch_size,
                random_verifier_batch_size,
                repair_service_count: 0,
                run_chunk_trackers_provision: false,
                run_data_retrieval_provision: false,
                run_downloaders_provision: true,
                run_repair_files_provision: false,
                run_scan_repair_provision: false,
                run_uploaders_provision: false,
                scan_frequency: None,
                sleep_duration,
                sleep_interval: None,
                start_chunk_trackers: false,
                start_data_retrieval: false,
                start_delayed_verifier,
                start_performance_verifier,
                start_random_verifier,
                start_repair_service: false,
                start_uploaders: false,
                upload_batch_size: None,
                uploaders_count: 0,
                upload_interval: 10,
                upload_size: None,
                wallet_secret_keys: None,
            };

            client_deployer.deploy_static_downloaders(options).await?;

            println!("Static downloader deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::DeployStaticUploader {
            ant_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_metrics,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            name,
            network_id,
            network_contacts_url,
            peer,
            provider,
            region,
            repo_owner,
            upload_batch_size,
            wallet_secret_key,
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

            let binary_option = get_binary_option(
                branch,
                repo_owner,
                ant_version,
                None,
                None,
                None,
                false, // skip_binary_build - not exposed for static uploader
            )
            .await?;

            let mut builder = ClientsDeployBuilder::new();
            builder
                .deployment_type(environment_type.clone())
                .environment_name(&name)
                .provider(provider);
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
                chunk_tracker_data_addresses: Vec::new(),
                chunk_tracker_services: 1,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                current_inventory: inventory,
                delayed_verifier_batch_size: None,
                delayed_verifier_quorum_value: None,
                enable_metrics: !disable_metrics,
                environment_type,
                evm_details,
                expected_hash: None,
                expected_size: None,
                file_address: None,
                funding_wallet_secret_key: None,
                initial_gas: None,
                initial_tokens: None,
                max_archived_log_files: 1,
                max_log_files: 1,
                max_uploads: None,
                name: name.clone(),
                network_contacts_url,
                network_id: Some(network_id),
                output_inventory_dir_path: client_deployer.working_directory_path.join("inventory"),
                peer,
                performance_verifier_batch_size: None,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                run_chunk_trackers_provision: false,
                run_data_retrieval_provision: false,
                run_downloaders_provision: false,
                run_repair_files_provision: false,
                run_scan_repair_provision: false,
                run_uploaders_provision: true,
                scan_frequency: None,
                sleep_duration: None,
                sleep_interval: None,
                start_chunk_trackers: false,
                start_data_retrieval: false,
                start_delayed_verifier: false,
                start_performance_verifier: false,
                start_random_verifier: false,
                start_repair_service: false,
                start_uploaders: false,
                upload_batch_size,
                uploaders_count: 1,
                upload_interval: 10,
                upload_size: None,
                wallet_secret_keys: Some(vec![wallet_secret_key]),
            };

            client_deployer.deploy_static_uploader(options).await?;

            println!("Static uploader deployment for '{name}' completed successfully");
            Ok(())
        }
        ClientsCommands::StartChunkTrackers { name, provider } => {
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
                AnsiblePlaybook::StartChunkTrackers,
                AnsibleInventoryType::Clients,
                None,
            )?;
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
        ClientsCommands::StopChunkTrackers { name, provider } => {
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
                AnsiblePlaybook::StopChunkTrackers,
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
            funding_wallet_secret_key,
            gas_amount,
            infra_only,
            name,
            plan,
            provision_only,
            provider,
            start_download_verifier,
            start_random_verifier,
            start_performance_verifier,
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
                    ant_version: Some(autonomi_version),
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
                    funding_wallet_secret_key,
                    gas_amount,
                    max_archived_log_files: 1,
                    max_log_files: 1,
                    infra_only,
                    interval: Duration::from_millis(2000),
                    network_dashboard_branch: None,
                    node_env_variables: None,
                    plan,
                    provision_only,
                    public_rpc: false,
                    start_delayed_verifier: start_download_verifier,
                    start_random_verifier,
                    start_performance_verifier,
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
                        eprintln!("Failed to generate inventory on attempt {retries}: {e:?}");
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
