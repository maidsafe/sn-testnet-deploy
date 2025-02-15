// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod deployments;
pub mod funds;
pub mod logs;
pub mod misc;
pub mod network;
pub mod nodes;
pub mod telegraf;
pub mod upgrade;
pub mod uploaders;

use crate::cmd::{
    funds::FundsCommand, logs::LogCommands, network::NetworkCommands, uploaders::UploadersCommands,
};
use alloy::primitives::U256;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use clap::Subcommand;
use color_eyre::{
    eyre::{bail, eyre, OptionExt},
    Help, Result,
};
use log::debug;
use semver::Version;
use sn_testnet_deploy::{
    inventory::{DeploymentInventory, VirtualMachine},
    BinaryOption, CloudProvider, EnvironmentType, EvmNetwork, LogFormat, NodeType,
};
use std::time::Duration;

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Bootstrap a new network from an existing deployment.
    Bootstrap {
        /// Supply a version number for the antctl binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        antctl_version: Option<String>,
        /// The features to enable on the antnode binary.
        ///
        /// If not provided, the default feature set specified for the antnode binary are used.
        ///
        /// The features argument is mutually exclusive with the --antnode-version argument.
        #[clap(long, verbatim_doc_comment)]
        antnode_features: Option<Vec<String>>,
        /// Supply a version number for the antnode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        antnode_version: Option<String>,
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
        /// The network contacts URL to bootstrap from.
        ///
        /// Either this or the `bootstrap-peer` argument must be provided.
        bootstrap_network_contacts_url: Option<String>,
        /// The peer from an existing network that we can bootstrap from.
        ///
        /// Either this or the `bootstrap-network-contacts-url` argument must be provided.
        #[arg(long)]
        bootstrap_peer: Option<String>,
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
        /// Provide environment variables for the antnode service.
        ///
        /// This is useful to set the antnode's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        env_variables: Option<Vec<(String, String)>>,
        /// The address of the data payments contract.
        ///
        /// This argument must match the same contract address used in the existing network.
        #[arg(long)]
        evm_data_payments_address: Option<String>,
        /// The EVM network to use.
        ///
        /// Valid values are "arbitrum-one", "arbitrum-sepolia", or "custom".
        #[clap(long, default_value_t = EvmNetwork::ArbitrumOne, value_parser = parse_evm_network)]
        evm_network_type: EvmNetwork,
        /// The address of the payment token contract.
        ///
        /// This argument must match the same contract address used in the existing network.
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
        /// The number of antnode services to be run behind a Full Cone NAT Gateway on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        full_cone_private_node_count: Option<u16>,
        /// The number of private node VMs to create. The private nodes will be behind a Full Cone NAT Gateway.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        full_cone_private_node_vm_count: Option<u16>,
        /// The size of the volumes to attach to each private node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        full_cone_private_node_volume_size: Option<u16>,
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
        /// Specify the network ID to use for the node services. This is used to partition the network and will not allow
        /// nodes with different network IDs to join.
        ///
        /// By default, the network ID is set to 1, which represents the mainnet.
        #[clap(long, verbatim_doc_comment)]
        network_id: Option<u8>,
        /// The number of antnode services to run on each VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_count: Option<u16>,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_vm_count: Option<u16>,
        /// Override the size of the node VMs.
        #[clap(long)]
        node_vm_size: Option<String>,
        /// The size of the volumes to attach to each node VM. This argument will set the size of all the 7 attached
        /// volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_volume_size: Option<u16>,
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
        /// The rewards address for each of the antnode services.
        #[arg(long, required = true)]
        rewards_address: String,
        /// The number of antnode services to be run behind a Symmetric NAT Gateway on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        symmetric_private_node_count: Option<u16>,
        /// The number of private node VMs to create. The private nodes will be behind a Symmetric NAT Gateway.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        symmetric_private_node_vm_count: Option<u16>,
        /// The size of the volumes to attach to each private node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        symmetric_private_node_volume_size: Option<u16>,
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
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The size of the swapfile in GB.
        #[arg(short = 's', long)]
        size: u16,
        /// Set to also configure swapfile on the PeerCache nodes.
        #[arg(long)]
        peer_cache: bool,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Deploy a new testnet environment using the latest version of the antnode binary.
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
        /// Supply a version number for the antctl binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        antctl_version: Option<String>,
        /// The features to enable on the antnode binary.
        ///
        /// If not provided, the default feature set specified for the antnode binary are used.
        ///
        /// The features argument is mutually exclusive with the --antnode-version argument.
        #[clap(long, verbatim_doc_comment)]
        antnode_features: Option<Vec<String>>,
        /// Supply a version number for the antnode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        antnode_version: Option<String>,
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
        /// The number of antnode services to run on each Peer Cache VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        peer_cache_node_count: Option<u16>,
        /// The number of Peer Cache node VMs to create.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        peer_cache_node_vm_count: Option<u16>,
        /// Override the size of the Peer Cache node VMs.
        #[clap(long)]
        peer_cache_node_vm_size: Option<String>,
        /// The size of the volumes to attach to each Peer Cache node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        peer_cache_node_volume_size: Option<u16>,
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
        /// Provide environment variables for the antnode service.
        ///
        /// This is useful to set the antnode's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        env_variables: Option<Vec<(String, String)>>,
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
        /// Override the size of the EVM node VMs.
        #[clap(long)]
        evm_node_vm_size: Option<String>,
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
        /// Override the size of the Full Cone NAT gateway VM.
        #[clap(long)]
        full_cone_nat_gateway_vm_size: Option<String>,
        /// The number of antnode services to be run behind a Full Cone NAT Gateway on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        full_cone_private_node_count: Option<u16>,
        /// The number of private node VMs to create. The private nodes will be behind a Full Cone NAT Gateway.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        full_cone_private_node_vm_count: Option<u16>,
        /// The size of the volumes to attach to each private node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        full_cone_private_node_volume_size: Option<u16>,
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The size of the volumes to attach to each genesis node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        genesis_node_volume_size: Option<u16>,
        /// The amount of gas to initially transfer to each uploader, in U256
        ///
        /// 1 ETH = 1_000_000_000_000_000_000. Defaults to 0.1 ETH
        #[arg(long)]
        initial_gas: Option<U256>,
        /// The amount of tokens to initially transfer to each uploader, in U256
        ///
        /// 1 Token = 1_000_000_000_000_000_000. Defaults to 100 token.
        #[arg(long)]
        initial_tokens: Option<U256>,
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
        /// Specify the network ID to use for the node services. This is used to partition the network and will not allow
        /// nodes with different network IDs to join.
        ///
        /// By default, the network ID is set to 1, which represents the mainnet.
        #[clap(long, verbatim_doc_comment)]
        network_id: Option<u8>,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// The number of antnode services to run on each VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_count: Option<u16>,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_vm_count: Option<u16>,
        /// Override the size of the node VMs.
        #[clap(long)]
        node_vm_size: Option<String>,
        /// The size of the volumes to attach to each node VM. This argument will set the size of all the 7 attached
        /// volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_volume_size: Option<u16>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// If set to true, the RPC of the node will be accessible remotely.
        ///
        /// By default, the antnode RPC is only accessible via the 'localhost' and is not exposed for
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
        /// The rewards address for each of the antnode services.
        #[arg(long, required = true)]
        rewards_address: String,
        /// Override the size of the Symmetric NAT gateway VM.
        #[clap(long)]
        symmetric_nat_gateway_vm_size: Option<String>,
        /// The number of antnode services to be run behind a Symmetric NAT Gateway on each private node VM.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long, verbatim_doc_comment)]
        symmetric_private_node_count: Option<u16>,
        /// The number of private node VMs to create. The private nodes will be behind a Symmetric NAT Gateway.
        ///
        /// Each VM will run many antnode services.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        #[clap(long, verbatim_doc_comment)]
        symmetric_private_node_vm_count: Option<u16>,
        /// The size of the volumes to attach to each private node VM. This argument will set the size of all the
        /// 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        symmetric_private_node_volume_size: Option<u16>,
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
        /// Set to only deploy up to the genesis node.
        ///
        /// This will provision all infrastructure but only deploy and start the genesis node.
        #[clap(long, default_value_t = false)]
        to_genesis: bool,
    },
    ExtendVolumeSize {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The new size of the volumes attached to each private node VM that is behind a Full Cone NAT Gateway.
        /// This argument will scale up the size of all the 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        full_cone_private_node_volume_size: Option<u16>,
        /// The new size of the volumes attached to each genesis node VM. This argument will scale up the size of all
        /// the 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        genesis_node_volume_size: Option<u16>,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The new size of the volumes attached to each node VM. This argument will scale up the size of all
        /// the 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        node_volume_size: Option<u16>,
        /// The new size of the volumes attached to each Peer Cache node VM. This argument will scale up the size of all
        /// the 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        peer_cache_node_volume_size: Option<u16>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The new size of the volumes attached to each private node VM that is behind a Symmetric NAT Gateway.
        /// This argument will scale up the size of all the 7 attached volumes.
        ///
        /// If the argument is not used, the value will be determined by the 'environment-type'
        /// argument.
        #[clap(long)]
        symmetric_private_node_volume_size: Option<u16>,
    },
    /// Manage the funds in the network
    #[clap(name = "funds", subcommand)]
    Funds(FundsCommand),
    Inventory {
        /// If set to true, the inventory will be regenerated.
        ///
        /// This is useful if the testnet was created on another machine.
        #[clap(long, default_value_t = false)]
        force_regeneration: bool,
        /// If set to true, all non-local listener addresses will be printed for each peer.
        #[clap(long, default_value_t = false)]
        full: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// If set to true, only print the Peer Cache webservers
        #[clap(long, default_value_t = false)]
        peer_cache: bool,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    #[clap(name = "logs", subcommand)]
    Logs(LogCommands),
    #[clap(name = "network", subcommand)]
    Network(NetworkCommands),
    /// Send a notification to Slack with testnet inventory details
    Notify {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
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
        /// Specify the type of node VM to start the antnode services on. If not provided, the antnode services on
        /// all the node VMs will be started. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
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
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
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
        /// Delay in seconds before stopping nodes.
        ///
        /// This can be useful when there is one node per machine.
        #[clap(long)]
        delay: Option<u64>,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The interval between each node stop in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        interval: Duration,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to stop the antnode services on. If not provided, the antnode services on
        /// all the node VMs will be stopped. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The service names to stop.
        #[clap(long)]
        service_name: Option<Vec<String>>,
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
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
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
        /// Provide environment variables for the antnode service.
        ///
        /// These will override the values provided initially.
        ///
        /// This is useful to set antnode's log levels. Each variable should be comma separated
        /// without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Set to force the node manager to accept the antnode version provided.
        ///
        /// This can be used to downgrade antnode to a known good version.
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
        /// Specify the type of node VM to upgrade the antnode services on. If not provided, the antnode services on
        /// all the node VMs will be upgraded. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// Delay before an upgrade starts.
        ///
        /// Useful for upgrading Peer Cache nodes when there is one node per machine.
        #[clap(long)]
        pre_upgrade_delay: Option<u64>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        #[arg(long)]
        /// Optionally supply a version number for the antnode binary to upgrade to.
        ///
        /// If not provided, the latest version will be used. A lower version number can be
        /// specified to downgrade to a known good version.
        ///
        /// There should be no 'v' prefix.
        version: Option<String>,
    },
    /// Upgrade antctl binaries to a particular version.
    ///
    /// Simple mechanism that copies over the existing binary.
    #[clap(name = "upgrade-antctl")]
    UpgradeAntctl {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will upgrade antctl on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of VM to run the upgrade on.
        ///
        /// If not provided, the upgrade will take place on all VMs.
        ///
        /// This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
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
        /// Supply a version number for the antctl binary.
        ///
        /// There should be no 'v' prefix.
        #[arg(long, verbatim_doc_comment)]
        antctl_version: Option<String>,
        /// Supply a version number for the antnode binary.
        ///
        /// There should be no 'v' prefix.
        #[arg(long, verbatim_doc_comment)]
        antnode_version: Option<String>,
        /// Supply a version number for the safe binary to be used for new uploader VMs.
        ///
        /// There should be no 'v' prefix.
        ///
        /// This argument is required when the uploader count is supplied.
        #[arg(long, verbatim_doc_comment)]
        ant_version: Option<String>,
        /// The name of a branch from which custom binaries were built.
        ///
        /// This only applies if the original deployment also used a custom branch. The upscale will
        /// then use the same binaries that were built in the original deployment.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument. It is mutually
        /// exclusive with the version arguments.
        #[arg(long, verbatim_doc_comment)]
        branch: Option<String>,
        /// The desired number of antnode services to be running behind a Full Cone NAT on each private node VM after
        /// the scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_full_cone_private_node_count: Option<u16>,
        /// The desired number of private node VMs to be running behind a Full Cone NAT after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_full_cone_private_node_vm_count: Option<u16>,
        /// The desired number of antnode services to be running on each node VM after the scale.
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
        /// The desired number of antnode services to be running on each Peer Cache VM after the
        /// scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_peer_cache_node_count: Option<u16>,
        /// The desired number of Peer Cache VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_peer_cache_node_vm_count: Option<u16>,
        /// The desired number of antnode services to be running behind a Symmetric NAT on each private node VM after
        /// the scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_symmetric_private_node_count: Option<u16>,
        /// The desired number of private node VMs to be running behind a Symmetric NAT after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the
        /// value, not 10 as a delta.
        ///
        /// This option is not applicable to a bootstrap deployment.
        #[clap(long, verbatim_doc_comment)]
        desired_symmetric_private_node_vm_count: Option<u16>,
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
        /// The maximum of archived log files to keep. After reaching this limit, the older files are deleted.
        #[clap(long, default_value = "5")]
        max_archived_log_files: u16,
        /// The maximum number of log files to keep. After reaching this limit, the older files are archived.
        #[clap(long, default_value = "10")]
        max_log_files: u16,
        /// The name of the existing network to upscale.
        #[arg(short = 'n', long, verbatim_doc_comment)]
        name: String,
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
        /// By default, the antnode RPC is only accessible via the 'localhost' and is not exposed for
        /// security reasons.
        #[clap(long, default_value_t = false, verbatim_doc_comment)]
        public_rpc: bool,
        /// The repo owner of a branch from which custom binaries were built.
        ///
        /// This only applies if the original deployment also used a custom branch. The upscale will
        /// then use the same binaries that were built in the original deployment.
        ///
        /// This argument must be used in conjunction with the --branch argument. It is mutually
        /// exclusive with the version arguments.
        #[arg(long, verbatim_doc_comment)]
        repo_owner: Option<String>,
    },
    /// Update the peer multiaddr in the node registry.
    ///
    /// This will then cause the service definitions to be updated when an upgrade is performed.
    UpdatePeer {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will update the peer on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Specify the type of node VM to update the peer on. If not provided, the peer will be updated on
        /// all the node VMs. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The new peer multiaddr to use.
        #[arg(long)]
        peer: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Reset nodes to a specified count.
    ///
    /// This will stop all nodes, clear their data, and start the specified number of nodes.
    #[clap(name = "reset-to-n-nodes")]
    ResetToNNodes {
        /// Provide a list of VM names to use as a custom inventory.
        ///
        /// This will reset nodes on a particular subset of VMs.
        #[clap(name = "custom-inventory", long, use_value_delimiter = true)]
        custom_inventory: Option<Vec<String>>,
        /// The EVM network to use.
        ///
        /// Valid values are "arbitrum-one", "arbitrum-sepolia", or "custom".
        #[clap(long, value_parser = parse_evm_network)]
        evm_network_type: EvmNetwork,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 50)]
        forks: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The number of nodes to run after reset.
        #[arg(long)]
        node_count: u16,
        /// Specify the type of node VM to reset the nodes on. If not provided, the nodes on
        /// all the node VMs will be reset. This is mutually exclusive with the '--custom-inventory' argument.
        ///
        /// Valid values are "peer-cache", "genesis", "generic" and "private".
        #[arg(long, conflicts_with = "custom-inventory")]
        node_type: Option<NodeType>,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The interval between starting each node in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        start_interval: Duration,
        /// The interval between stopping each node in milliseconds.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_millis)?)}, default_value = "2000")]
        stop_interval: Duration,
        /// Supply a version number for the antnode binary.
        ///
        /// If not provided, the latest version will be used.
        #[arg(long)]
        version: Option<String>,
    },
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
#[allow(clippy::too_many_arguments)]
async fn get_binary_option(
    branch: Option<String>,
    repo_owner: Option<String>,
    ant_version: Option<String>,
    antnode_version: Option<String>,
    antctl_version: Option<String>,
    antnode_features: Option<Vec<String>>,
) -> Result<BinaryOption> {
    let mut use_versions = true;

    let branch_specified = branch.is_some() || repo_owner.is_some();
    let versions_specified = antnode_version.is_some() || antctl_version.is_some();
    if branch_specified && versions_specified {
        return Err(
            eyre!("Version numbers and branches cannot be supplied at the same time").suggestion(
                "Please choose whether you want to use version numbers or build the binaries",
            ),
        );
    }

    if versions_specified && antnode_features.is_some() {
        return Err(eyre!(
            "The --antnode-features argument only applies if we are building binaries"
        ));
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

        let ant_version = get_version_from_option(ant_version, &ReleaseType::Ant).await?;
        let antnode_version =
            get_version_from_option(antnode_version, &ReleaseType::AntNode).await?;
        let antctl_version = get_version_from_option(antctl_version, &ReleaseType::AntCtl).await?;
        BinaryOption::Versioned {
            ant_version: Some(ant_version),
            antnode_version,
            antctl_version,
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

        let url = format!("https://github.com/{repo_owner}/autonomi/tree/{branch}",);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            bail!("The provided branch or owner does not exist: {url:?}");
        }
        BinaryOption::BuildFromSource {
            repo_owner,
            branch,
            antnode_features: antnode_features.map(|list| list.join(",")),
        }
    };

    Ok(binary_option)
}

pub fn get_custom_inventory(
    inventory: &DeploymentInventory,
    vm_list: &[String],
) -> Result<Vec<VirtualMachine>> {
    debug!("Attempting to use a custom inventory: {vm_list:?}");
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

    debug!("Retrieved custom inventory:");
    for vm in &custom_vms {
        debug!("  {} - {}", vm.name, vm.public_ip_addr);
    }
    Ok(custom_vms)
}

pub async fn get_version_from_option(
    version: Option<String>,
    release_type: &ReleaseType,
) -> Result<Version> {
    let release_repo = <dyn AntReleaseRepoActions>::default_config();
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

pub fn parse_chunk_size(val: &str) -> Result<u64> {
    let size = val.parse::<u64>()?;
    if size == 0 {
        Err(eyre!("chunk_size must be a positive integer"))
    } else {
        Ok(size)
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
pub fn parse_environment_variables(env_var: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = env_var.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(eyre!(
            "Environment variable must be in the format KEY=VALUE or KEY=INNER_KEY=VALUE.\nMultiple key-value pairs can be given with a comma between them."
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

pub fn parse_evm_network(s: &str) -> Result<EvmNetwork, String> {
    match s.to_lowercase().as_str() {
        "anvil" => Ok(EvmNetwork::Anvil),
        "arbitrum-one" => Ok(EvmNetwork::ArbitrumOne),
        "arbitrum-sepolia" => Ok(EvmNetwork::ArbitrumSepolia),
        "custom" => Ok(EvmNetwork::Custom),
        _ => Err(format!("Invalid EVM network type: {}", s)),
    }
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

fn print_with_banner(s: &str) {
    let banner = "=".repeat(s.len());
    println!("{}\n{}\n{}", banner, s, banner);
}
