// Copyright (c) 2023, MaidSafe.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use alloy::primitives::{Address, U256};
use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{bail, eyre, OptionExt},
    Help, Result,
};
use dotenv::dotenv;
use evmlib::Network;
use semver::Version;
use sn_releases::{ReleaseType, SafeReleaseRepoActions};
use sn_testnet_deploy::{
    ansible::{extra_vars::ExtraVarsDocBuilder, inventory::AnsibleInventoryType, AnsiblePlaybook},
    bootstrap::BootstrapOptions,
    deploy::DeployOptions,
    error::Error,
    funding::FundingOptions,
    get_environment_details,
    inventory::{
        get_data_directory, DeploymentInventory, DeploymentInventoryService, VirtualMachine,
    },
    logstash::LogstashDeployBuilder,
    network_commands, notify_slack,
    setup::setup_dotenv_file,
    upscale::UpscaleOptions,
    BinaryOption, CloudProvider, EnvironmentType, EvmCustomTestnetData, EvmNetwork, LogFormat,
    NodeType, TestnetDeployBuilder, UpgradeOptions,
};
use std::{env, net::IpAddr};
use std::{str::FromStr, time::Duration};

#[derive(Parser, Debug)]
#[clap(name = "sn-testnet-deploy", version = env!("CARGO_PKG_VERSION"))]
struct Opt {
    #[command(subcommand)]
    command: Commands,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum Commands {
    /// Bootstrap a new network from an existing deployment.
    Bootstrap {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The branch of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this branch. It is typically used for testing
        /// changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        branch: Option<String>,
        /// The peer from an existing network that we can bootstrap from.
        #[arg(long)]
        bootstrap_peer: String,
        /// Specify the chunk size for the custom binaries using a 64-bit integer.
        ///
        /// This option only applies if the --branch and --repo-owner arguments are used.
        #[clap(long, value_parser = parse_chunk_size)]
        chunk_size: Option<u64>,
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
        /// Provide environment variables for the safenode service.
        ///
        /// This is useful to set the safenode's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        env_variables: Option<Vec<(String, String)>>,
        /// The address of the data payments contract.
        ///
        /// This argument only applies if the EVM network type is 'custom'.
        #[arg(long)]
        evm_data_payments_address: Option<String>,
        /// The private key of the wallet that deployed the contracts.
        ///
        /// This argument only applies if the EVM network type is 'custom'.
        #[arg(long)]
        evm_deployer_wallet_private_key: Option<String>,
        /// The EVM network to use.
        ///
        /// Valid values are "arbitrum-one", "arbitrum-sepolia", or "custom".
        #[clap(long, default_value_t = EvmNetwork::ArbitrumOne, value_parser = parse_evm_network)]
        evm_network_type: EvmNetwork,
        /// The address of the payment token contract.
        ///
        /// This argument only applies if the EVM network type is 'custom'.
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
        /// Optionally set the foundation public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        foundation_pk: Option<String>,
        /// Optionally set the genesis public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        genesis_pk: Option<String>,
        /// The interval between starting each node in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// Specify the logging format for the nodes.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// The maximum of archived log files to keep. After reaching this limit, the older files are deleted.
        #[clap(long, default_value = "5")]
        max_archived_log_files: u16,
        /// The maximum number of log files to keep. After reaching this limit, the older files are archived.
        #[clap(long, default_value = "10")]
        max_log_files: u16,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Optionally set the network royalties public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        network_royalties_pk: Option<String>,
        /// The number of safenode services to run on each VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_count: Option<u16>,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_vm_count: Option<u16>,
        /// Override the size of the node VMs.
        #[clap(long)]
        node_vm_size: Option<String>,
        /// Optionally set the payment forward public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        payment_forward_pk: Option<String>,
        /// The number of safenode services to be run behind a NAT on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        private_node_count: Option<u16>,
        /// The number of private node VMs to create.
        ///
        /// Each VM will run many safenode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        private_node_vm_count: Option<u16>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
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
        /// The rewards address for each of the safenode services.
        #[arg(long, required = true)]
        rewards_address: String,
        /// The features to enable on the safenode binary.
        ///
        /// If not provided, the default feature set specified for the safenode binary are used.
        ///
        /// The features argument is mutually exclusive with the --safenode-version argument.
        #[clap(long, verbatim_doc_comment)]
        safenode_features: Option<Vec<String>>,
        /// Supply a version number for the safenode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_version: Option<String>,
        /// Supply a version number for the safenode-manager binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_manager_version: Option<String>,
    },
    /// Clean a deployed testnet environment.
    Clean {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Configure a swapfile on all nodes in the environment.
    ConfigureSwapfile {
        /// Set to also configure swapfile on the bootstrap nodes.
        #[arg(long)]
        bootstrap: bool,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The size of the swapfile in GB.
        #[arg(short = 's', long)]
        size: u16,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Deploy a new testnet environment using the latest version of the safenode binary.
    Deploy {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The branch of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this branch. It is typically used for testing
        /// changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        branch: Option<String>,
        /// The number of safenode services to run on each bootstrap VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        bootstrap_node_count: Option<u16>,
        /// The number of bootstrap node VMs to create.
        ///
        /// Each VM will run many safenode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        bootstrap_node_vm_count: Option<u16>,
        /// Override the size of the bootstrap node VMs.
        #[clap(long)]
        bootstrap_node_vm_size: Option<String>,
        /// Specify the chunk size for the custom binaries using a 64-bit integer.
        ///
        /// This option only applies if the --branch and --repo-owner arguments are used.
        #[clap(long, value_parser = parse_chunk_size)]
        chunk_size: Option<u64>,
        /// If set to a non-zero value, the uploaders will also be accompanied by the specified
        /// number of downloaders.
        ///
        /// This will be the number on each uploader VM. So if the value here is 2 and there are
        /// 5 uploader VMs, there will be 10 downloaders across the 5 VMs.
        #[clap(long, default_value_t = 0)]
        downloaders_count: u16,
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
        /// Provide environment variables for the safenode service.
        ///
        /// This is useful to set the safenode's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        env_variables: Option<Vec<(String, String)>>,
        /// The EVM network type to use for the deployment.
        ///
        /// Possible values are 'arbitrum-one' or 'custom'.
        ///
        /// If not used, the default is 'arbitrum-one'.
        #[clap(long, default_value = "arbitrum-one", value_parser = parse_evm_network)]
        evm_network_type: EvmNetwork,
        /// Override the size of the EVM node VMs.
        #[clap(long)]
        evm_node_vm_size: Option<String>,
        /// Supply a version number to be used for the faucet binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner arguments.
        /// You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        faucet_version: Option<String>,
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// Optionally set the foundation public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        foundation_pk: Option<String>,
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// Optionally set the genesis public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        genesis_pk: Option<String>,
        /// The interval between starting each node in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// Specify the logging format for the nodes.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// The name of the Logstash stack to forward logs to.
        #[clap(long, default_value = "main")]
        logstash_stack_name: String,
        /// The maximum of archived log files to keep. After reaching this limit, the older files are deleted.
        #[clap(long, default_value = "5")]
        max_archived_log_files: u16,
        /// The maximum number of log files to keep. After reaching this limit, the older files are archived.
        #[clap(long, default_value = "10")]
        max_log_files: u16,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// Optionally set the network royalties public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        network_royalties_pk: Option<String>,
        /// The number of safenode services to run on each VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_count: Option<u16>,
        /// Override the size of the node VMs.
        #[clap(long)]
        node_vm_size: Option<String>,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_vm_count: Option<u16>,
        /// Optionally set the payment forward public key for a custom safenode binary.
        ///
        /// This argument only applies if the '--branch' and '--repo-owner' arguments are used.
        ///
        /// If one of the new keys is supplied, all must be supplied.
        #[arg(long)]
        payment_forward_pk: Option<String>,
        /// The number of safenode services to be run behind a NAT on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        private_node_count: Option<u16>,
        /// The number of private node VMs to create.
        ///
        /// Each VM will run many safenode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        private_node_vm_count: Option<u16>,
        /// Protocol version is used to partition the network and will not allow nodes with
        /// different protocol versions to join.
        ///
        /// If set to 'restricted', the branch name is used as the protocol version; otherwise the
        /// version is set to the value supplied.
        ///
        /// This argument is mutually exclusive with the --safenode-version argument.
        #[arg(long, verbatim_doc_comment)]
        protocol_version: Option<String>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// If set to true, the RPC of the node will be accessible remotely.
        ///
        /// By default, the safenode RPC is only accessible via the 'localhost' and is not exposed for
        /// security reasons.
        #[clap(long, default_value_t = false, verbatim_doc_comment)]
        public_rpc: bool,
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
        /// The rewards address for each of the safenode services.
        #[arg(long, required = true)]
        rewards_address: String,
        /// Supply a version number for the safe binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safe_version: Option<String>,
        /// The features to enable on the safenode binary.
        ///
        /// If not provided, the default feature set specified for the safenode binary are used.
        ///
        /// The features argument is mutually exclusive with the --safenode-version argument.
        #[clap(long, verbatim_doc_comment)]
        safenode_features: Option<Vec<String>>,
        /// Supply a version number for the safenode-manager binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_manager_version: Option<String>,
        /// Supply a version number for the safenode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_version: Option<String>,
        /// The desired number of uploaders per VM.
        #[clap(long, default_value_t = 1)]
        uploaders_count: u16,
        /// The number of uploader VMs to create.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        uploader_vm_count: Option<u16>,
        /// Override the size of the uploader VMs.
        #[clap(long)]
        uploader_vm_size: Option<String>,
    },
    /// Manage the faucet for an environment
    #[clap(name = "faucet", subcommand)]
    Faucet(FaucetCommands),
    /// Manage the funds in the network
    #[clap(name = "funds", subcommand)]
    Funds(FundsCommand),
    Inventory {
        /// If set to true, the inventory will be regenerated.
        ///
        /// This is useful if the testnet was created on another machine.
        #[clap(long, default_value_t = false)]
        force_regeneration: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    #[clap(name = "logs", subcommand)]
    Logs(LogCommands),
    #[clap(name = "logstash", subcommand)]
    Logstash(LogstashCommands),
    #[clap(name = "network", subcommand)]
    Network(NetworkCommands),
    /// Send a notification to Slack with testnet inventory details
    Notify {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Run 'terraform plan' for a given environment.
    ///
    /// Useful for reviewing infrastructure changes before deploying them.
    Plan {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    Setup {},
    /// Start all nodes in an environment.
    ///
    /// This can be useful if all nodes did not upgrade successfully.
    #[clap(name = "start")]
    Start {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will start nodes on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The interval between each node start in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to start the safenode services on. If not provided, the safenode services on
        /// all the node VMs will be started. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Get the status of all nodes in the environment.
    #[clap(name = "status")]
    Status {
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Start the Telegraf service on all machines in the environment.
    ///
    /// This may be necessary for performing upgrades.
    #[clap(name = "start-telegraf")]
    StartTelegraf {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will stop Telegraf on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to start the telegraf services on. If not provided, the telegraf services on
        /// all the node VMs will be started. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Stop all nodes in an environment.
    #[clap(name = "stop")]
    Stop {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will stop nodes on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The interval between each node stop in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to stop the safenode services on. If not provided, the safenode services on
        /// all the node VMs will be stopped. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Stop the Telegraf service on all machines in the environment.
    ///
    /// This may be necessary for performing upgrades.
    #[clap(name = "stop-telegraf")]
    StopTelegraf {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will stop Telegraf on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to stop the telegraf services on. If not provided, the telegraf services on
        /// all the node VMs will be stopped. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Upgrade the node binaries of a testnet environment to the latest version.
    Upgrade {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will run the upgrade against this particular subset of VMs.
        ///
        /// It can be useful to save time and run the upgrade against particular machines that were
        /// unreachable during the main run.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// Provide environment variables for the safenode service.
        ///
        /// These will override the values provided initially.
        ///
        /// This is useful to set safenode's log levels. Each variable should be comma separated
        /// without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Set to force the node manager to accept the safenode version provided.
        ///
        /// This can be used to downgrade safenode to a known good version.
        #[clap(long)]
        force: bool,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 2)]
        forks: usize,
        /// The interval between each node upgrade.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to upgrade the safenode services on. If not provided, the safenode services on
        /// all the node VMs will be upgraded. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        #[arg(long)]
        /// Optionally supply a version number for the safenode binary to upgrade to.
        ///
        /// If not provided, the latest version will be used. A lower version number can be
        /// specified to downgrade to a known good version.
        ///
        /// There should be no 'v' prefix.
        version: Option<String>,
    },
    /// Upgrade the safenode-manager binaries to a particular version.
    ///
    /// Simple mechanism that simply copies over the existing binary.
    #[clap(name = "upgrade-node-manager")]
    UpgradeNodeManager {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will upgrade the node manager on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to upgrade the safenode-manager services on. If not provided, the
        /// safenode-manager on all the node VMs will be upgraded.
        /// This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "bootstrap", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        #[arg(long)]
        /// The cloud provider of the environment.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Supply a version for the binary to be upgraded to.
        ///
        /// There should be no 'v' prefix.
        #[arg(short = 'v', long)]
        version: String,
    },
    /// Upgrade the node Telegraf configuration on an environment.
    #[clap(name = "upgrade-node-telegraf-config")]
    UpgradeNodeTelegrafConfig {
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Upgrade the uploader Telegraf configuration on an environment.
    #[clap(name = "upgrade-uploader-telegraf-config")]
    UpgradeUploaderTelegrafConfig {
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Manage uploaders for an environment
    #[clap(name = "uploaders", subcommand)]
    Uploaders(UploadersCommands),
    /// Upscale VMs and node services for an existing network.
    Upscale {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The desired number of auditor VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_auditor_vm_count: Option<u16>,
        /// The desired number of safenode services to be running on each bootstrap VM after the
        /// scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// Note: bootstrap VMs normally only use a single node service, so you probably want this
        /// value to be 1.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_bootstrap_node_count: Option<u16>,
        /// The desired number of bootstrap VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_bootstrap_node_vm_count: Option<u16>,
        /// The desired number of safenode services to be running on each node VM after the scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_node_count: Option<u16>,
        /// The desired number of node VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_node_vm_count: Option<u16>,
        /// The desired number of safenode services to be running behind a NAT on each private node VM after the
        /// scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_private_node_count: Option<u16>,
        /// The desired number of private node VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_private_node_vm_count: Option<u16>,
        /// The desired number of uploader VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_uploader_vm_count: Option<u16>,
        /// The desired number of uploaders to be running after the scale.
        ///
        /// If you want each uploader VM to run multiple uploader services, specify the total desired count.
        #[clap(long, verbatim_doc_comment)]
        desired_uploaders_count: Option<u16>,
        /// If set to a non-zero value, the uploaders will also be accompanied by the specified
        /// number of downloaders.
        ///
        /// This will be the number on each uploader VM. So if the value here is 2 and there are
        /// 5 uploader VMs, there will be 10 downloaders across the 5 VMs.
        #[clap(long, default_value_t = 0)]
        downloaders_count: u16,
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// Set to only use Terraform to upscale the VMs and not run Ansible.
        #[clap(long, default_value_t = false)]
        infra_only: bool,
        /// The interval between starting each node in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// The name of the existing network to upscale.
        #[arg(short = 'n', long, verbatim_doc_comment)]
        name: String,
        /// The maximum of archived log files to keep. After reaching this limit, the older files are deleted.
        #[clap(long, default_value = "5")]
        max_archived_log_files: u16,
        /// The maximum number of log files to keep. After reaching this limit, the older files are archived.
        #[clap(long, default_value = "10")]
        max_log_files: u16,
        /// Set to only run the Terraform plan rather than applying the changes.
        ///
        /// Can be useful to preview the upscale to make sure everything is ok and that no other
        /// changes slipped in.
        ///
        /// The plan will run and then the command will exit without doing anything else.
        #[clap(long, default_value_t = false)]
        plan: bool,
        /// The cloud provider for the network.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// If set to true, for new VMs the RPC of the node will be accessible remotely.
        ///
        /// By default, the safenode RPC is only accessible via the 'localhost' and is not exposed for
        /// security reasons.
        #[clap(long, default_value_t = false, verbatim_doc_comment)]
        public_rpc: bool,
        /// Supply a version number for the safe binary to be used for new uploader VMs.
        ///
        /// There should be no 'v' prefix.
        ///
        /// This argument is required when the uploader count is supplied.
        #[arg(long, verbatim_doc_comment)]
        safe_version: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum LogCommands {
    /// Removes all the rotated log files from the the node VMs.
    Cleanup {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Setup a cron job to perform the cleanup periodically.
        #[clap(long)]
        setup_cron: bool,
    },
    /// Retrieve the logs for a given environment by copying them from all the VMs.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Copy {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Should we copy the resource-usage.logs only
        #[arg(short = 'r', long)]
        resources_only: bool,
    },
    /// Retrieve the logs for a given environment from S3.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Get {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Reassemble retrieved logs from their parts.
    ///
    /// The logs must have already been retrieved using the 'get' command and be present at
    /// 'logs/<name>'.
    ///
    /// This will write the logs to 'logs/<name>-reassembled', relative to the current directory.
    ///
    /// The original logs are left intact so you can sync again if need be.
    Reassemble {
        /// The name of the environment for which logs have already been retrieved
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Run a ripgrep query through all the logs from all the VMs and copy the results.
    ///
    /// The results will be written to `logs/<name>/<vm>/rg-timestamp.log`
    Rg {
        /// The ripgrep arguments that are directly passed to ripgrep. The text to search for should be put inside
        /// single quotes. The dir to search for is set automatically, so do not provide one.
        ///
        /// Example command: `cargo run --release -- logs rg --name <name> --args "'ValidSpendRecordPutFromNetwork' -z -a"`
        #[arg(short = 'a', long, allow_hyphen_values(true))]
        args: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Remove the logs from a given environment from the bucket on S3.
    Rm {
        /// The name of the environment for which logs have already been retrieved
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Rsync the logs from all the VMs for a given environment.
    /// Rerunning the same command will sync only the changed log files without copying everything from the beginning.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Rsync {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Should we copy the resource-usage.logs only
        #[arg(short = 'r', long)]
        resources_only: bool,
        /// Optionally only sync the logs for the VMs that contain the following string.
        #[arg(long)]
        vm_filter: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum LogstashCommands {
    /// Clean a deployed Logstash environment.
    Clean {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Deploy the Logstash infrastructure to support log forwarding to S3.
    Deploy {
        /// The name of the Logstash environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider to provision on
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The number of VMs to create.
        ///
        /// Use this to horizontally scale Logstash if need be.
        #[clap(long, default_value = "1")]
        vm_count: u16,
    },
}

// Administer or perform activities on a deployed network.
#[derive(Subcommand, Debug)]
enum NetworkCommands {
    /// Restart nodes in the testnet to simulate the churn of nodes.
    #[clap(name = "churn", subcommand)]
    ChurnCommands(ChurnCommands),
    /// Modifies the log levels for all the safenode services through RPC requests.
    UpdateNodeLogLevel {
        /// The number of nodes to update concurrently.
        #[clap(long, short = 'c', default_value_t = 10)]
        concurrent_updates: usize,
        /// Change the log level of the safenode. This accepts a comma-separated list of log levels for different modules
        /// or specific keywords like "all" or "v".
        ///
        /// Example: --level libp2p=DEBUG,tokio=INFO,all,sn_client=ERROR
        #[clap(name = "level", long)]
        log_level: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum ChurnCommands {
    /// Churn nodes at fixed intervals.
    FixedInterval {
        /// The number of time each node in the network is restarted.
        #[clap(long, default_value_t = 1)]
        churn_cycles: usize,
        /// The number of nodes to restart concurrently per VM.
        #[clap(long, short = 'c', default_value_t = 2)]
        concurrent_churns: usize,
        /// The interval between each node churn.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?)}, default_value = "60")]
        interval: Duration,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Whether to retain the same PeerId on restart.
        #[clap(long, default_value_t = false)]
        retain_peer_id: bool,
    },
    /// Churn nodes at random intervals.
    RandomInterval {
        /// Number of nodes to restart in the given time frame.
        #[clap(long, default_value_t = 10)]
        churn_count: usize,
        /// The number of time each node in the network is restarted.
        #[clap(long, default_value_t = 1)]
        churn_cycles: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Whether to retain the same PeerId on restart.
        #[clap(long, default_value_t = false)]
        retain_peer_id: bool,
        /// The time frame in which the churn_count nodes are restarted.
        /// Nodes are restarted at a rate of churn_count/time_frame with random delays between each restart.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?)}, default_value = "600")]
        time_frame: Duration,
    },
}

#[derive(Subcommand, Debug)]
enum UploadersCommands {
    /// Start all uploaders for an environment
    Start {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Stop all uploaders for an environment.
    Stop {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Upgrade the uploaders for a given environment.
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
    /// Upscale uploaders for an existing network.
    Upscale {
        /// Supply a version number for the autonomi binary to be used for new uploader VMs.
        ///
        /// There should be no 'v' prefix.
        #[arg(long, verbatim_doc_comment)]
        autonomi_version: String,
        /// The desired number of uploader VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_uploader_vm_count: Option<u16>,
        /// The desired number of uploaders to be running after the scale.
        ///
        /// If you want each uploader VM to run multiple uploader services, specify the total desired count.
        #[clap(long, verbatim_doc_comment)]
        desired_uploaders_count: Option<u16>,
        /// If set to a non-zero value, the uploaders will also be accompanied by the specified
        /// number of downloaders.
        ///
        /// This will be the number on each uploader VM. So if the value here is 2 and there are
        /// 5 uploader VMs, there will be 10 downloaders across the 5 VMs.
        #[clap(long, default_value_t = 0)]
        downloaders_count: u16,
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The amount of gas tokens to transfer to each uploader.
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

#[derive(Subcommand, Debug)]
enum FaucetCommands {
    /// Fund the uploaders from the faucet
    ///
    /// This command requires the faucet to be running, so run the 'faucet start' command first.
    FundUploaders {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Set to a non-zero value to fund each uploader wallet multiple times.
        ///
        /// The faucet will distribute one token on each playbook run.
        #[clap(long, default_value_t = 0)]
        repeat: u8,
    },
    /// Start the faucet for the environment
    Start {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Stop the faucet for the environment
    Stop {
        /// The name of the environment
        #[arg(long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
}

#[derive(Subcommand, Debug)]
enum FundsCommand {
    /// Deposit tokens and gas from the provided funding wallet secret key to all the uploaders
    Deposit {
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The number of gas to transfer, in U256
        ///
        /// 1 ETH = 1_000_000_000_000_000_000. Defaults to 0.1 ETH
        #[arg(long)]
        gas_to_transfer: Option<U256>,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The number of tokens to transfer, in U256
        ///
        /// 1 Token = 1_000_000_000_000_000_000. Defaults to 1 token.
        #[arg(long)]
        tokens_to_transfer: Option<U256>,
    },
    /// Drain all the tokens and gas from the uploaders to the funding wallet.
    Drain {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The address of the wallet that will receive all the tokens and gas.
        ///
        /// This argument is optional, the funding wallet address from the S3 environment file will be used by default.
        #[clap(long)]
        to_address: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    dotenv().ok();
    env_logger::init();

    let opt = Opt::parse();
    match opt.command {
        Commands::Bootstrap {
            ansible_verbose,
            bootstrap_peer,
            branch,
            environment_type,
            env_variables,
            evm_data_payments_address,
            evm_deployer_wallet_private_key,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            forks,
            foundation_pk,
            genesis_pk,
            interval,
            log_format,
            name,
            network_royalties_pk,
            node_count,
            node_vm_count,
            node_vm_size,
            max_archived_log_files,
            max_log_files,
            payment_forward_pk,
            private_node_count,
            private_node_vm_count,
            provider,
            repo_owner,
            rewards_address,
            safenode_features,
            safenode_version,
            safenode_manager_version,
            chunk_size,
        } => {
            let evm_custom_testnet_data = match evm_network_type {
                EvmNetwork::Custom => {
                    if evm_data_payments_address.is_none()
                        || evm_payment_token_address.is_none()
                        || evm_rpc_url.is_none()
                    {
                        return Err(eyre!(
                            "When using a custom EVM network, you must supply evm-data-payments-address, evm-payment-token-address, and evm-rpc-url"
                        ));
                    }
                    Some(EvmCustomTestnetData {
                        data_payments_address: evm_data_payments_address.unwrap(),
                        deployer_wallet_private_key: evm_deployer_wallet_private_key
                            .unwrap_or_default(),
                        payment_token_address: evm_payment_token_address.unwrap(),
                        rpc_url: evm_rpc_url.unwrap(),
                    })
                }
                _ => {
                    if evm_data_payments_address.is_some()
                        || evm_payment_token_address.is_some()
                        || evm_rpc_url.is_some()
                        || evm_deployer_wallet_private_key.is_some()
                    {
                        return Err(eyre!(
                            "EVM custom network parameters can only be used with evm-network-type=custom"
                        ));
                    }
                    None
                }
            };

            let network_keys = validate_and_get_pks(
                foundation_pk,
                genesis_pk,
                network_royalties_pk,
                payment_forward_pk,
            )?;

            let binary_option = get_binary_option(
                branch,
                None,
                repo_owner,
                None,
                None,
                safenode_version,
                safenode_manager_version,
                safenode_features,
                network_keys,
            )
            .await?;

            let mut builder = TestnetDeployBuilder::default();
            builder
                .ansible_verbose_mode(ansible_verbose)
                .deployment_type(environment_type.clone())
                .environment_name(&name)
                .provider(provider);
            if let Some(forks) = forks {
                builder.ansible_forks(forks);
            }
            let testnet_deployer = builder.build()?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service
                .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
                .await?;

            match testnet_deployer.init().await {
                Ok(_) => {}
                Err(e @ Error::LogsForPreviousTestnetExist(_)) => {
                    return Err(eyre!(e)
                        .wrap_err(format!(
                            "Logs already exist for a previous testnet with the \
                                    name '{name}'"
                        ))
                        .suggestion(
                            "If you wish to keep them, retrieve the logs with the 'logs get' \
                                command, then remove them with 'logs rm'. If you don't need them, \
                                simply run 'logs rm'. Then you can proceed with deploying your \
                                new testnet.",
                        ));
                }
                Err(e) => {
                    return Err(eyre!(e));
                }
            }

            testnet_deployer
                .bootstrap(&BootstrapOptions {
                    binary_option,
                    bootstrap_peer,
                    environment_type: environment_type.clone(),
                    env_variables,
                    evm_network: evm_network_type,
                    evm_custom_testnet_data,
                    interval,
                    log_format,
                    name: name.clone(),
                    node_count: node_count.unwrap_or(environment_type.get_default_node_count()),
                    max_archived_log_files,
                    max_log_files,
                    node_vm_size,
                    output_inventory_dir_path: inventory_service
                        .working_directory_path
                        .join("ansible")
                        .join("inventory"),
                    private_node_vm_count,
                    private_node_count: private_node_count
                        .unwrap_or(environment_type.get_default_private_node_count()),
                    node_vm_count,
                    rewards_address,
                    chunk_size,
                })
                .await?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let new_inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            new_inventory.print_report()?;
            new_inventory.save()?;
            Ok(())
        }
        Commands::Clean { name, provider } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.clean().await?;
            Ok(())
        }
        Commands::Deploy {
            ansible_verbose,
            bootstrap_node_count,
            bootstrap_node_vm_count,
            bootstrap_node_vm_size,
            branch,
            chunk_size,
            downloaders_count,
            env_variables,
            environment_type,
            evm_network_type,
            evm_node_vm_size,
            faucet_version,
            forks,
            foundation_pk,
            funding_wallet_secret_key,
            genesis_pk,
            interval,
            log_format,
            logstash_stack_name,
            max_archived_log_files,
            max_log_files,
            name,
            network_contacts_file_name,
            network_royalties_pk,
            node_count,
            node_vm_count,
            node_vm_size,
            payment_forward_pk,
            private_node_count,
            private_node_vm_count,
            protocol_version,
            provider,
            public_rpc,
            repo_owner,
            rewards_address,
            safe_version,
            safenode_features,
            safenode_manager_version,
            safenode_version,
            uploader_vm_count,
            uploader_vm_size,
            uploaders_count,
        } => {
            let network_keys = validate_and_get_pks(
                foundation_pk,
                genesis_pk,
                network_royalties_pk,
                payment_forward_pk,
            )?;

            if funding_wallet_secret_key.is_some() && evm_network_type == EvmNetwork::Custom {
                return Err(eyre!(
                    "Wallet secret key only applies to Arbitrum or Sepolia networks"
                ));
            } else if funding_wallet_secret_key.is_none() && evm_network_type != EvmNetwork::Custom
            {
                return Err(eyre!(
                    "Wallet secret key is required for Arbitrum or Sepolia networks"
                ));
            }

            let binary_option = get_binary_option(
                branch,
                protocol_version,
                repo_owner,
                faucet_version,
                safe_version,
                safenode_version,
                safenode_manager_version,
                safenode_features,
                network_keys,
            )
            .await?;

            let mut builder = TestnetDeployBuilder::default();
            builder
                .ansible_verbose_mode(ansible_verbose)
                .deployment_type(environment_type.clone())
                .environment_name(&name)
                .provider(provider);
            if let Some(forks) = forks {
                builder.ansible_forks(forks);
            }
            let testnet_deployer = builder.build()?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
                .await?;

            match testnet_deployer.init().await {
                Ok(_) => {}
                Err(e @ Error::LogsForPreviousTestnetExist(_)) => {
                    return Err(eyre!(e)
                        .wrap_err(format!(
                            "Logs already exist for a previous testnet with the \
                                    name '{name}'"
                        ))
                        .suggestion(
                            "If you wish to keep them, retrieve the logs with the 'logs get' \
                                command, then remove them with 'logs rm'. If you don't need them, \
                                simply run 'logs rm'. Then you can proceed with deploying your \
                                new testnet.",
                        ));
                }
                Err(e) => {
                    return Err(eyre!(e));
                }
            }

            let logstash_details = {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                let stack_hosts = logstash_deploy
                    .get_stack_hosts(&logstash_stack_name)
                    .await?;
                if stack_hosts.is_empty() {
                    None
                } else {
                    Some((logstash_stack_name, stack_hosts))
                }
            };

            testnet_deployer
                .deploy(&DeployOptions {
                    binary_option: binary_option.clone(),
                    bootstrap_node_count: bootstrap_node_count
                        .unwrap_or(environment_type.get_default_bootstrap_node_count()),
                    bootstrap_node_vm_count,
                    chunk_size,
                    current_inventory: inventory,
                    downloaders_count,
                    environment_type: environment_type.clone(),
                    env_variables,
                    evm_network: evm_network_type,
                    evm_node_vm_size,
                    funding_wallet_secret_key,
                    interval,
                    log_format,
                    logstash_details,
                    name: name.clone(),
                    node_count: node_count.unwrap_or(environment_type.get_default_node_count()),
                    node_vm_count,
                    max_archived_log_files,
                    max_log_files,
                    output_inventory_dir_path: inventory_service
                        .working_directory_path
                        .join("ansible")
                        .join("inventory"),
                    private_node_vm_count,
                    private_node_count: private_node_count
                        .unwrap_or(environment_type.get_default_private_node_count()),
                    public_rpc,
                    uploaders_count,
                    uploader_vm_count,
                    rewards_address,
                    node_vm_size,
                    bootstrap_node_vm_size,
                    uploader_vm_size,
                })
                .await?;

            let max_retries = 3;
            let mut retries = 0;
            let inventory = loop {
                match inventory_service
                    .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
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

            inventory.print_report()?;
            inventory.save()?;

            inventory_service
                .upload_network_contacts(&inventory, network_contacts_file_name)
                .await?;

            Ok(())
        }
        Commands::Faucet(uploaders_cmd) => match uploaders_cmd {
            FaucetCommands::FundUploaders {
                name,
                provider,
                repeat,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .ansible_forks(1)
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
                let inventory = inventory_service
                    .generate_or_retrieve_inventory(&name, true, None)
                    .await?;

                let ansible_runner = testnet_deployer.ansible_provisioner.ansible_runner;

                let playbook_runs = repeat + 1;
                for _ in 0..playbook_runs {
                    ansible_runner.run_playbook(
                        AnsiblePlaybook::FundUploaders,
                        AnsibleInventoryType::Uploaders,
                        Some(build_fund_faucet_extra_vars_doc(
                            &inventory.get_genesis_ip().ok_or_else(||
                                eyre!("Genesis node not found. Most likely this is a bootstrap deployment."))?,
                            &inventory.genesis_multiaddr.clone().ok_or_else(||
                                eyre!("Genesis node not found. Most likely this is a bootstrap deployment."))?,
                        )?),
                    )?;
                }

                Ok(())
            }
            FaucetCommands::Start { name, provider } => {
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
                    AnsiblePlaybook::StartFaucet,
                    AnsibleInventoryType::Genesis,
                    None,
                )?;
                Ok(())
            }
            FaucetCommands::Stop { name, provider } => {
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
                    AnsiblePlaybook::StopFaucet,
                    AnsibleInventoryType::Genesis,
                    None,
                )?;
                Ok(())
            }
        },
        Commands::Funds(funds_cmd) => match funds_cmd {
            FundsCommand::Deposit {
                funding_wallet_secret_key,
                gas_to_transfer,
                name,
                provider,
                tokens_to_transfer,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                let inventory_services = DeploymentInventoryService::from(&testnet_deployer);
                inventory_services
                    .generate_or_retrieve_inventory(&name, true, None)
                    .await?;

                let environment_details =
                    get_environment_details(&name, &inventory_services.s3_repository).await?;

                if funding_wallet_secret_key.is_some()
                    && environment_details.evm_network == EvmNetwork::Custom
                {
                    return Err(eyre!(
                        "Wallet secret key only applies to Arbitrum or Sepolia networks"
                    ));
                }

                let options = FundingOptions {
                    custom_evm_testnet_data: environment_details.evm_testnet_data,
                    evm_network: environment_details.evm_network,
                    funding_wallet_secret_key,
                    uploaders_count: None,
                    token_amount: tokens_to_transfer,
                    gas_amount: gas_to_transfer,
                };
                testnet_deployer
                    .ansible_provisioner
                    .deposit_funds_to_uploaders(&options)
                    .await?;

                Ok(())
            }
            FundsCommand::Drain {
                name,
                provider,
                to_address,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;

                let inventory_services = DeploymentInventoryService::from(&testnet_deployer);
                inventory_services
                    .generate_or_retrieve_inventory(&name, true, None)
                    .await?;

                let environment_details =
                    get_environment_details(&name, &inventory_services.s3_repository).await?;

                let to_address = if let Some(to_address) = to_address {
                    Address::from_str(&to_address)?
                } else if let Some(to_address) = environment_details.funding_wallet_address {
                    Address::from_str(&to_address)?
                } else {
                    return Err(eyre!(
                        "No to-address was provided and no funding wallet address was found in the environment details"
                    ));
                };

                let network = match environment_details.evm_network {
                    EvmNetwork::ArbitrumOne => Network::ArbitrumOne,
                    EvmNetwork::ArbitrumSepolia => Network::ArbitrumSepolia,
                    EvmNetwork::Custom => {
                        let custom_evm_details =
                            environment_details.evm_testnet_data.ok_or_else(|| {
                                eyre!("Custom EVM details not found in the environment details")
                            })?;
                        Network::new_custom(
                            &custom_evm_details.rpc_url,
                            &custom_evm_details.payment_token_address,
                            &custom_evm_details.data_payments_address,
                        )
                    }
                };

                testnet_deployer
                    .ansible_provisioner
                    .drain_funds_from_uploaders(to_address, network)
                    .await?;

                Ok(())
            }
        },
        Commands::Inventory {
            force_regeneration,
            name,
            network_contacts_file_name,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, force_regeneration, None)
                .await?;
            inventory.print_report()?;
            inventory.save()?;

            inventory_service
                .upload_network_contacts(&inventory, network_contacts_file_name)
                .await?;

            Ok(())
        }
        Commands::Logs(log_cmd) => match log_cmd {
            LogCommands::Cleanup {
                name,
                provider,
                setup_cron,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deployer.init().await?;
                let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
                inventory_service.setup_environment_inventory(&name)?;

                testnet_deployer.cleanup_node_logs(setup_cron)?;
                Ok(())
            }
            LogCommands::Copy {
                name,
                provider,
                resources_only,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deployer.init().await?;
                let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
                inventory_service.setup_environment_inventory(&name)?;

                testnet_deployer.copy_logs(&name, resources_only)?;
                Ok(())
            }
            LogCommands::Get { name } => {
                sn_testnet_deploy::logs::get_logs(&name).await?;
                Ok(())
            }
            LogCommands::Reassemble { name } => {
                sn_testnet_deploy::logs::reassemble_logs(&name)?;
                Ok(())
            }
            LogCommands::Rg {
                args,
                name,
                provider,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deployer.init().await?;
                let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
                inventory_service.setup_environment_inventory(&name)?;

                testnet_deployer.ripgrep_logs(&name, &args)?;
                Ok(())
            }

            LogCommands::Rm { name } => {
                sn_testnet_deploy::logs::rm_logs(&name).await?;
                Ok(())
            }
            LogCommands::Rsync {
                name,
                provider,
                resources_only,
                vm_filter,
            } => {
                let testnet_deployer = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deployer.init().await?;
                let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
                inventory_service.setup_environment_inventory(&name)?;

                testnet_deployer.rsync_logs(&name, resources_only, vm_filter)?;
                Ok(())
            }
        },
        Commands::Logstash(logstash_cmd) => match logstash_cmd {
            LogstashCommands::Clean { name, provider } => {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .provider(provider)
                    .build()?;
                logstash_deploy.clean(&name)?;
                Ok(())
            }
            LogstashCommands::Deploy {
                name,
                provider,
                vm_count,
            } => {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .provider(provider)
                    .build()?;
                logstash_deploy.init(&name).await?;
                logstash_deploy.deploy(&name, vm_count)?;
                Ok(())
            }
        },
        Commands::Network(NetworkCommands::ChurnCommands(churn_cmds)) => {
            let (name, provider) = match &churn_cmds {
                ChurnCommands::FixedInterval { name, provider, .. } => (name, provider),
                ChurnCommands::RandomInterval { name, provider, .. } => (name, provider),
            };
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(1)
                .environment_name(name)
                .provider(*provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(name, true, None)
                .await?;

            match churn_cmds {
                ChurnCommands::FixedInterval {
                    churn_cycles,
                    concurrent_churns,
                    interval,
                    retain_peer_id,
                    ..
                } => {
                    network_commands::perform_fixed_interval_network_churn(
                        inventory,
                        interval,
                        concurrent_churns,
                        retain_peer_id,
                        churn_cycles,
                    )
                    .await?;
                }
                ChurnCommands::RandomInterval {
                    churn_count,
                    churn_cycles,
                    retain_peer_id,
                    time_frame,
                    ..
                } => {
                    network_commands::perform_random_interval_network_churn(
                        inventory,
                        time_frame,
                        churn_count,
                        retain_peer_id,
                        churn_cycles,
                    )
                    .await?;
                }
            }
            Ok(())
        }
        Commands::Network(NetworkCommands::UpdateNodeLogLevel {
            concurrent_updates,
            log_level,
            name,
        }) => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;
            network_commands::update_node_log_levels(inventory, log_level, concurrent_updates)
                .await?;

            Ok(())
        }
        Commands::Notify { name } => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;
            notify_slack(inventory).await?;
            Ok(())
        }
        Commands::Plan { name, provider } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            testnet_deployer.init().await?;
            testnet_deployer.plan(None, inventory.environment_details.environment_type)?;
            Ok(())
        }
        Commands::Setup {} => {
            setup_dotenv_file()?;
            Ok(())
        }
        Commands::Start {
            custom_inventory,
            forks,
            interval,
            name,
            node_type,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            testnet_deployer.start(interval, node_type, custom_inventory)?;

            Ok(())
        }
        Commands::StartTelegraf {
            custom_inventory,
            forks,
            name,
            node_type,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            testnet_deployer.start_telegraf(node_type, custom_inventory)?;

            Ok(())
        }
        Commands::Status {
            forks,
            name,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            testnet_deployer.status()?;
            Ok(())
        }
        Commands::Stop {
            custom_inventory,
            forks,
            interval,
            name,
            node_type,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            testnet_deployer.stop(interval, node_type, custom_inventory)?;

            Ok(())
        }
        Commands::StopTelegraf {
            custom_inventory,
            forks,
            name,
            node_type,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            testnet_deployer.stop_telegraf(node_type, custom_inventory)?;

            Ok(())
        }
        Commands::ConfigureSwapfile {
            bootstrap,
            name,
            provider,
            size,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let ansible_runner = testnet_deployer.ansible_provisioner.ansible_runner;
            ansible_runner.run_playbook(
                AnsiblePlaybook::ConfigureSwapfile,
                AnsibleInventoryType::Nodes,
                Some(build_swapfile_extra_vars_doc(size)?),
            )?;

            if bootstrap {
                ansible_runner.run_playbook(
                    AnsiblePlaybook::ConfigureSwapfile,
                    AnsibleInventoryType::BootstrapNodes,
                    Some(build_swapfile_extra_vars_doc(size)?),
                )?;
            }

            Ok(())
        }
        Commands::Upgrade {
            ansible_verbose,
            custom_inventory,
            env_variables,
            force,
            forks,
            interval,
            name,
            node_type,
            provider,
            version,
        } => {
            // The upgrade intentionally uses a small value for `forks`, but this is far too slow
            // for retrieving the inventory from a large deployment. Therefore, we will use 50
            // forks for the initial run to retrieve the inventory, then recreate the deployer
            // using the smaller fork value.
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(50)
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .ansible_verbose_mode(ansible_verbose)
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.upgrade(UpgradeOptions {
                ansible_verbose,
                custom_inventory,
                env_variables,
                force,
                forks,
                interval,
                name: name.clone(),
                node_type,
                provider,
                version,
            })?;

            // Recreate the deployer with an increased number of forks for retrieving the status.
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(50)
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.status()?;

            Ok(())
        }
        Commands::UpgradeNodeManager {
            custom_inventory,
            name,
            node_type,
            provider,
            version,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(50)
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            let custom_inventory = if let Some(custom_inventory) = custom_inventory {
                let custom_vms = get_custom_inventory(&inventory, &custom_inventory)?;
                Some(custom_vms)
            } else {
                None
            };

            testnet_deployer.upgrade_node_manager(version.parse()?, node_type, custom_inventory)?;
            Ok(())
        }
        Commands::UpgradeNodeTelegrafConfig {
            forks,
            name,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            testnet_deployer.upgrade_node_telegraf(&name)?;

            Ok(())
        }
        Commands::UpgradeUploaderTelegrafConfig {
            forks,
            name,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(forks)
                .environment_name(&name)
                .provider(provider)
                .build()?;

            // This is required in the case where the command runs in a remote environment, where
            // there won't be an existing inventory, which is required to retrieve the node
            // registry files used to determine the status.
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;
            if inventory.is_empty() {
                return Err(eyre!("The {name} environment does not exist"));
            }

            testnet_deployer.upgrade_uploader_telegraf(&name)?;

            Ok(())
        }
        Commands::Uploaders(uploaders_cmd) => match uploaders_cmd {
            UploadersCommands::Start { name, provider } => {
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
                    AnsibleInventoryType::Uploaders,
                    None,
                )?;
                Ok(())
            }
            UploadersCommands::Stop { name, provider } => {
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
                    AnsibleInventoryType::Uploaders,
                    None,
                )?;
                Ok(())
            }
            UploadersCommands::Upgrade {
                name,
                provider,
                version,
            } => {
                let version = get_version_from_option(version, &ReleaseType::Autonomi).await?;

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
                extra_vars.add_variable("autonomi_version", &version.to_string());
                ansible_runner.run_playbook(
                    AnsiblePlaybook::UpgradeUploaders,
                    AnsibleInventoryType::Uploaders,
                    Some(extra_vars.build()),
                )?;

                Ok(())
            }
            UploadersCommands::Upscale {
                autonomi_version,
                desired_uploader_vm_count,
                desired_uploaders_count,
                downloaders_count,
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

                println!("Upscaling uploaders...");
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
                    .upscale_uploaders(&UpscaleOptions {
                        ansible_verbose: false,
                        current_inventory: inventory,
                        desired_auditor_vm_count: None,
                        desired_bootstrap_node_count: None,
                        desired_bootstrap_node_vm_count: None,
                        desired_node_count: None,
                        desired_node_vm_count: None,
                        desired_private_node_count: None,
                        desired_private_node_vm_count: None,
                        desired_uploader_vm_count,
                        desired_uploaders_count,
                        downloaders_count,
                        funding_wallet_secret_key,
                        gas_amount,
                        max_archived_log_files: 1,
                        max_log_files: 1,
                        infra_only,
                        interval: Duration::from_millis(2000),
                        plan,
                        provision_only,
                        public_rpc: false,
                        safe_version: Some(autonomi_version),
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

                inventory.print_report()?;
                inventory.save()?;

                Ok(())
            }
        },
        Commands::Upscale {
            ansible_verbose,
            desired_auditor_vm_count,
            desired_bootstrap_node_count,
            desired_bootstrap_node_vm_count,
            desired_node_count,
            desired_node_vm_count,
            desired_private_node_count,
            desired_private_node_vm_count,
            desired_uploader_vm_count,
            desired_uploaders_count,
            downloaders_count,
            funding_wallet_secret_key,
            infra_only,
            interval,
            name,
            max_archived_log_files,
            max_log_files,
            plan,
            provider,
            public_rpc,
            safe_version,
        } => {
            if desired_uploader_vm_count.is_some() && safe_version.is_none() {
                return Err(eyre!("The --safe-version argument is required when --desired-uploader-vm-count is used"));
            }

            println!("Upscaling deployment...");
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_verbose_mode(ansible_verbose)
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;

            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            testnet_deployer
                .upscale(&UpscaleOptions {
                    ansible_verbose,
                    current_inventory: inventory,
                    desired_auditor_vm_count,
                    desired_bootstrap_node_count,
                    desired_bootstrap_node_vm_count,
                    desired_node_count,
                    desired_node_vm_count,
                    desired_private_node_count,
                    desired_private_node_vm_count,
                    desired_uploader_vm_count,
                    desired_uploaders_count,
                    downloaders_count,
                    funding_wallet_secret_key,
                    gas_amount: None,
                    interval,
                    max_archived_log_files,
                    max_log_files,
                    infra_only,
                    plan,
                    provision_only: false,
                    public_rpc,
                    safe_version,
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

            inventory.print_report()?;
            inventory.save()?;

            Ok(())
        }
    }
}

/// Get the binary option for the deployment.
///
/// Versioned binaries are preferred first, since building from source adds significant time to the
/// deployment. There are two options here. If no version arguments were supplied, the latest
/// versions will be used. Otherwise, the specified versions will be used, and if any were not
/// specified, the latest version will be used in its place.
///
/// The second option is to build from source, which is useful for testing changes from forks.
///
/// The usage of arguments are also validated here.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
async fn get_binary_option(
    branch: Option<String>,
    protocol_version: Option<String>,
    repo_owner: Option<String>,
    faucet_version: Option<String>,
    safe_version: Option<String>,
    safenode_version: Option<String>,
    safenode_manager_version: Option<String>,
    safenode_features: Option<Vec<String>>,
    network_keys: Option<(String, String, String, String)>,
) -> Result<BinaryOption> {
    let mut use_versions = true;

    let branch_specified = branch.is_some() || repo_owner.is_some();
    let versions_specified = faucet_version.is_some()
        || safenode_version.is_some()
        || safenode_manager_version.is_some();
    if branch_specified && versions_specified {
        return Err(
            eyre!("Version numbers and branches cannot be supplied at the same time").suggestion(
                "Please choose whether you want to use version numbers or build the binaries",
            ),
        );
    }

    if versions_specified {
        if safenode_features.is_some() {
            return Err(eyre!(
                "The --safenode-features argument only applies if we are building binaries"
            ));
        }
        if protocol_version.is_some() {
            return Err(eyre!(
                "The --protocol-version argument only applies if we are building binaries"
            ));
        }
    }

    if branch_specified {
        if let (Some(_), None) | (None, Some(_)) = (&repo_owner, &branch) {
            return Err(eyre!(
                "The --branch and --repo-owner arguments must be supplied together"
            ));
        }
        use_versions = false;
    }

    let binary_option = if use_versions {
        print_with_banner("Binaries will be supplied from pre-built versions");

        let safe_version = get_version_from_option(safe_version, &ReleaseType::Autonomi).await?;
        let safenode_version =
            get_version_from_option(safenode_version, &ReleaseType::Safenode).await?;
        let safenode_manager_version =
            get_version_from_option(safenode_manager_version, &ReleaseType::SafenodeManager)
                .await?;
        BinaryOption::Versioned {
            safe_version: Some(safe_version),
            safenode_version,
            safenode_manager_version,
        }
    } else {
        // Unwraps are justified here because it's already been asserted that both must have
        // values.
        let repo_owner = repo_owner.unwrap();
        let branch = branch.unwrap();

        print_with_banner(&format!(
            "Binaries will be built from {}/{}",
            repo_owner, branch
        ));

        if let Some(ref network_keys) = network_keys {
            println!("Using custom network keys:");
            println!("Foundation PK: {}", network_keys.0);
            println!("Genesis PK: {}", network_keys.1);
            println!("Network Royalties PK: {}", network_keys.2);
            println!("Payment Forward PK: {}", network_keys.3);
        }

        let url = format!("https://github.com/{repo_owner}/safe_network/tree/{branch}",);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            bail!("The provided branch or owner does not exist: {url:?}");
        }
        BinaryOption::BuildFromSource {
            repo_owner,
            branch,
            safenode_features: safenode_features.map(|list| list.join(",")),
            protocol_version,
            network_keys,
        }
    };

    Ok(binary_option)
}

fn print_with_banner(s: &str) {
    let banner = "=".repeat(s.len());
    println!("{}\n{}\n{}", banner, s, banner);
}

pub fn parse_provider(val: &str) -> Result<CloudProvider> {
    match val {
        "aws" => Ok(CloudProvider::Aws),
        "digital-ocean" => Ok(CloudProvider::DigitalOcean),
        _ => Err(eyre!(
            "The only supported providers are 'aws' or 'digital-ocean'"
        )),
    }
}

pub fn parse_deployment_type(val: &str) -> Result<EnvironmentType> {
    match val {
        "development" => Ok(EnvironmentType::Development),
        "production" => Ok(EnvironmentType::Production),
        "staging" => Ok(EnvironmentType::Staging),
        _ => Err(eyre!(
            "Supported deployment types are 'development', 'production' or 'staging'."
        )),
    }
}

// Since delimiter is on, we get element of the csv and not the entire csv.
fn parse_environment_variables(env_var: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = env_var.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(eyre!(
            "Environment variable must be in the format KEY=VALUE or KEY=INNER_KEY=VALUE.\nMultiple key-value pairs can be given with a comma between them."
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

async fn get_version_from_option(
    version: Option<String>,
    release_type: &ReleaseType,
) -> Result<Version> {
    let release_repo = <dyn SafeReleaseRepoActions>::default_config();
    let version = if let Some(version) = version {
        println!("Using {version} for {release_type}");
        version
    } else {
        println!("Getting latest version for {release_type}...");
        let version = release_repo
            .get_latest_version(release_type)
            .await?
            .to_string();
        println!("Using {version} for {release_type}");
        version
    };
    Ok(version.parse()?)
}

fn get_custom_inventory(
    inventory: &DeploymentInventory,
    vm_list: &[String],
) -> Result<Vec<VirtualMachine>> {
    let mut custom_vms = Vec::new();
    for vm_name in vm_list.iter() {
        let vm_list = inventory.vm_list();
        let vm = vm_list
            .iter()
            .find(|vm| vm_name == &vm.name)
            .ok_or_eyre(format!(
                "{vm_name} is not in the inventory for this environment",
            ))?;
        custom_vms.push(vm.clone());
    }
    Ok(custom_vms)
}

fn build_fund_faucet_extra_vars_doc(
    genesis_ip: &IpAddr,
    genesis_multiaddr: &str,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("genesis_addr", &genesis_ip.to_string());
    extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
    Ok(extra_vars.build())
}

fn build_swapfile_extra_vars_doc(size: u16) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("swapfile_size", &format!("{size}G"));
    Ok(extra_vars.build())
}

fn parse_chunk_size(val: &str) -> Result<u64> {
    let size = val.parse::<u64>()?;
    if size == 0 {
        Err(eyre!("chunk_size must be a positive integer"))
    } else {
        Ok(size)
    }
}

fn validate_and_get_pks(
    foundation_pk: Option<String>,
    genesis_pk: Option<String>,
    network_royalties_pk: Option<String>,
    payment_forward_pk: Option<String>,
) -> Result<Option<(String, String, String, String)>> {
    let all_pks_supplied = foundation_pk.is_some()
        && genesis_pk.is_some()
        && network_royalties_pk.is_some()
        && payment_forward_pk.is_some();
    let any_pk_supplied = foundation_pk.is_some()
        || genesis_pk.is_some()
        || network_royalties_pk.is_some()
        || payment_forward_pk.is_some();

    if any_pk_supplied && !all_pks_supplied {
        return Err(eyre!(
            "The network keys are a set. They must be supplied together."
        ));
    }

    if all_pks_supplied {
        Ok(Some((
            foundation_pk.unwrap(),
            genesis_pk.unwrap(),
            network_royalties_pk.unwrap(),
            payment_forward_pk.unwrap(),
        )))
    } else {
        Ok(None)
    }
}

fn parse_evm_network(s: &str) -> Result<EvmNetwork, String> {
    match s.to_lowercase().as_str() {
        "arbitrum-one" => Ok(EvmNetwork::ArbitrumOne),
        "arbitrum-sepolia" => Ok(EvmNetwork::ArbitrumSepolia),
        "custom" => Ok(EvmNetwork::Custom),
        _ => Err(format!("Invalid EVM network type: {}", s)),
    }
}
