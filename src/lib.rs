// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod bootstrap;
pub mod clients;
pub mod deploy;
pub mod digital_ocean;
pub mod error;
pub mod funding;
pub mod infra;
pub mod inventory;
pub mod logs;
pub mod reserved_ip;
pub mod rpc_client;
pub mod s3;
pub mod safe;
pub mod setup;
pub mod ssh;
pub mod symlinked_antnode;
pub mod terraform;
pub mod upscale;

pub use symlinked_antnode::SymlinkedAntnodeDeployer;

const STORAGE_REQUIRED_PER_NODE: u16 = 7;

use crate::{
    ansible::{
        extra_vars::ExtraVarsDocBuilder,
        inventory::{cleanup_environment_inventory, AnsibleInventoryType},
        provisioning::AnsibleProvisioner,
        AnsibleRunner,
    },
    error::{Error, Result},
    inventory::{DeploymentInventory, VirtualMachine},
    rpc_client::RpcClient,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
};
use ant_service_management::ServiceStatus;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use infra::{build_terraform_args, InfraRunOptions};
use log::{debug, trace};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    time::Duration,
};
use tar::Archive;

const ANSIBLE_DEFAULT_FORKS: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DeploymentType {
    /// The deployment has been bootstrapped from an existing network.
    Bootstrap,
    /// Client deployment.
    Client,
    /// The deployment is a new network.
    #[default]
    New,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnvilNodeData {
    pub data_payments_address: String,
    pub deployer_wallet_private_key: String,
    pub merkle_payments_address: String,
    pub payment_token_address: String,
    pub rpc_url: String,
}

impl std::fmt::Display for DeploymentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentType::Bootstrap => write!(f, "bootstrap"),
            DeploymentType::Client => write!(f, "clients"),
            DeploymentType::New => write!(f, "new"),
        }
    }
}

impl std::str::FromStr for DeploymentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bootstrap" => Ok(DeploymentType::Bootstrap),
            "clients" => Ok(DeploymentType::Client),
            "new" => Ok(DeploymentType::New),
            _ => Err(format!("Invalid deployment type: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum NodeType {
    FullConePrivateNode,
    PortRestrictedConePrivateNode,
    Generic,
    Genesis,
    PeerCache,
    SymmetricPrivateNode,
    Upnp,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::FullConePrivateNode => write!(f, "full-cone-private"),
            NodeType::PortRestrictedConePrivateNode => write!(f, "port-restricted-cone-private"),
            NodeType::Generic => write!(f, "generic"),
            NodeType::Genesis => write!(f, "genesis"),
            NodeType::PeerCache => write!(f, "peer-cache"),
            NodeType::SymmetricPrivateNode => write!(f, "symmetric-private"),
            NodeType::Upnp => write!(f, "upnp"),
        }
    }
}

impl std::str::FromStr for NodeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full-cone-private" => Ok(NodeType::FullConePrivateNode),
            "port-restricted-cone-private" => Ok(NodeType::PortRestrictedConePrivateNode),
            "generic" => Ok(NodeType::Generic),
            "genesis" => Ok(NodeType::Genesis),
            "peer-cache" => Ok(NodeType::PeerCache),
            "symmetric-private" => Ok(NodeType::SymmetricPrivateNode),
            "upnp" => Ok(NodeType::Upnp),
            _ => Err(format!("Invalid node type: {s}")),
        }
    }
}

impl NodeType {
    pub fn telegraf_role(&self) -> &'static str {
        match self {
            NodeType::FullConePrivateNode => "NAT_STATIC_FULL_CONE_NODE",
            NodeType::PortRestrictedConePrivateNode => "PORT_RESTRICTED_CONE_NODE",
            NodeType::Generic => "GENERIC_NODE",
            NodeType::Genesis => "GENESIS_NODE",
            NodeType::PeerCache => "PEER_CACHE_NODE",
            NodeType::SymmetricPrivateNode => "NAT_RANDOMIZED_NODE",
            NodeType::Upnp => "UPNP_NODE",
        }
    }

    pub fn to_ansible_inventory_type(&self) -> AnsibleInventoryType {
        match self {
            NodeType::FullConePrivateNode => AnsibleInventoryType::FullConePrivateNodes,
            NodeType::PortRestrictedConePrivateNode => {
                AnsibleInventoryType::PortRestrictedConePrivateNodes
            }
            NodeType::Generic => AnsibleInventoryType::Nodes,
            NodeType::Genesis => AnsibleInventoryType::Genesis,
            NodeType::PeerCache => AnsibleInventoryType::PeerCacheNodes,
            NodeType::SymmetricPrivateNode => AnsibleInventoryType::SymmetricPrivateNodes,
            NodeType::Upnp => AnsibleInventoryType::Upnp,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Serialize, Deserialize, PartialEq)]
pub enum EvmNetwork {
    #[default]
    Anvil,
    ArbitrumOne,
    ArbitrumSepoliaTest,
    Custom,
}

impl std::fmt::Display for EvmNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvmNetwork::Anvil => write!(f, "evm-custom"),
            EvmNetwork::ArbitrumOne => write!(f, "evm-arbitrum-one"),
            EvmNetwork::ArbitrumSepoliaTest => write!(f, "evm-arbitrum-sepolia-test"),
            EvmNetwork::Custom => write!(f, "evm-custom"),
        }
    }
}

impl std::str::FromStr for EvmNetwork {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anvil" => Ok(EvmNetwork::Anvil),
            "arbitrum-one" => Ok(EvmNetwork::ArbitrumOne),
            "arbitrum-sepolia-test" => Ok(EvmNetwork::ArbitrumSepoliaTest),
            "custom" => Ok(EvmNetwork::Custom),
            _ => Err(format!("Invalid EVM network type: {s}")),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvmDetails {
    pub network: EvmNetwork,
    pub data_payments_address: Option<String>,
    pub merkle_payments_address: Option<String>,
    pub payment_token_address: Option<String>,
    pub rpc_url: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvironmentDetails {
    pub deployment_type: DeploymentType,
    pub environment_type: EnvironmentType,
    pub evm_details: EvmDetails,
    pub funding_wallet_address: Option<String>,
    pub network_id: Option<u8>,
    pub region: String,
    pub rewards_address: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum EnvironmentType {
    #[default]
    Development,
    Production,
    Staging,
}

impl EnvironmentType {
    pub fn get_tfvars_filenames(&self, name: &str, region: &str) -> Vec<String> {
        match self {
            EnvironmentType::Development => vec![
                "dev.tfvars".to_string(),
                format!("dev-images-{region}.tfvars", region = region),
            ],
            EnvironmentType::Staging => vec![
                "staging.tfvars".to_string(),
                format!("staging-images-{region}.tfvars", region = region),
            ],
            EnvironmentType::Production => {
                vec![
                    format!("{name}.tfvars", name = name),
                    format!("production-images-{region}.tfvars", region = region),
                ]
            }
        }
    }

    pub fn get_tfvars_filenames_with_fallback(
        &self,
        name: &str,
        region: &str,
        terraform_dir: &Path,
    ) -> Vec<String> {
        match self {
            EnvironmentType::Development | EnvironmentType::Staging => {
                self.get_tfvars_filenames(name, region)
            }
            EnvironmentType::Production => {
                let named_tfvars = format!("{name}.tfvars");
                let tfvars_file = if terraform_dir.join(&named_tfvars).exists() {
                    named_tfvars
                } else {
                    "production.tfvars".to_string()
                };
                vec![tfvars_file, format!("production-images-{region}.tfvars")]
            }
        }
    }

    pub fn get_default_peer_cache_node_count(&self) -> u16 {
        match self {
            EnvironmentType::Development => 5,
            EnvironmentType::Production => 5,
            EnvironmentType::Staging => 5,
        }
    }

    pub fn get_default_node_count(&self) -> u16 {
        match self {
            EnvironmentType::Development => 25,
            EnvironmentType::Production => 25,
            EnvironmentType::Staging => 25,
        }
    }

    pub fn get_default_symmetric_private_node_count(&self) -> u16 {
        self.get_default_node_count()
    }

    pub fn get_default_full_cone_private_node_count(&self) -> u16 {
        self.get_default_node_count()
    }
    pub fn get_default_upnp_private_node_count(&self) -> u16 {
        self.get_default_node_count()
    }
}

impl std::fmt::Display for EnvironmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentType::Development => write!(f, "development"),
            EnvironmentType::Production => write!(f, "production"),
            EnvironmentType::Staging => write!(f, "staging"),
        }
    }
}

impl FromStr for EnvironmentType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "development" => Ok(EnvironmentType::Development),
            "production" => Ok(EnvironmentType::Production),
            "staging" => Ok(EnvironmentType::Staging),
            _ => Err(Error::EnvironmentNameFromStringError(s.to_string())),
        }
    }
}

/// Specify the binary option for the deployment.
///
/// There are several binaries involved in the deployment:
/// * safenode
/// * safenode_rpc_client
/// * faucet
/// * safe
///
/// The `safe` binary is only used for smoke testing the deployment, although we don't really do
/// that at the moment.
///
/// The options are to build from source, or supply a pre-built, versioned binary, which will be
/// fetched from S3. Building from source adds significant time to the deployment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BinaryOption {
    /// Binaries will be built from source.
    BuildFromSource {
        /// A comma-separated list that will be passed to the `--features` argument.
        antnode_features: Option<String>,
        branch: String,
        repo_owner: String,
        /// Skip building the binaries, if they were already built during the previous run using the same
        /// branch, repo owner and testnet name.
        skip_binary_build: bool,
    },
    /// Pre-built, versioned binaries will be fetched from S3.
    Versioned {
        ant_version: Option<Version>,
        antctl_version: Option<Version>,
        antnode_version: Option<Version>,
    },
}

impl BinaryOption {
    pub fn should_provision_build_machine(&self) -> bool {
        match self {
            BinaryOption::BuildFromSource {
                skip_binary_build, ..
            } => !skip_binary_build,
            BinaryOption::Versioned { .. } => false,
        }
    }

    pub fn print(&self) {
        match self {
            BinaryOption::BuildFromSource {
                antnode_features,
                branch,
                repo_owner,
                skip_binary_build: _,
            } => {
                println!("Source configuration:");
                println!("  Repository owner: {repo_owner}");
                println!("  Branch: {branch}");
                if let Some(features) = antnode_features {
                    println!("  Antnode features: {features}");
                }
            }
            BinaryOption::Versioned {
                ant_version,
                antctl_version,
                antnode_version,
            } => {
                println!("Versioned binaries configuration:");
                if let Some(version) = ant_version {
                    println!("  ant version: {version}");
                }
                if let Some(version) = antctl_version {
                    println!("  antctl version: {version}");
                }
                if let Some(version) = antnode_version {
                    println!("  antnode version: {version}");
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CloudProvider {
    Aws,
    DigitalOcean,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProvider::Aws => write!(f, "aws"),
            CloudProvider::DigitalOcean => write!(f, "digital-ocean"),
        }
    }
}

impl CloudProvider {
    pub fn get_ssh_user(&self) -> String {
        match self {
            CloudProvider::Aws => "ubuntu".to_string(),
            CloudProvider::DigitalOcean => "root".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LogFormat {
    Default,
    Json,
}

impl LogFormat {
    pub fn parse_from_str(val: &str) -> Result<Self> {
        match val {
            "default" => Ok(LogFormat::Default),
            "json" => Ok(LogFormat::Json),
            _ => Err(Error::LoggingConfiguration(
                "The only valid values for this argument are \"default\" or \"json\"".to_string(),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::Default => "default",
            LogFormat::Json => "json",
        }
    }
}

#[derive(Clone)]
pub struct UpgradeOptions {
    pub ansible_verbose: bool,
    pub branch: Option<String>,
    pub custom_inventory: Option<Vec<VirtualMachine>>,
    pub env_variables: Option<Vec<(String, String)>>,
    pub force: bool,
    pub forks: usize,
    pub interval: Duration,
    pub name: String,
    pub node_type: Option<NodeType>,
    pub pre_upgrade_delay: Option<u64>,
    pub provider: CloudProvider,
    pub repo_owner: Option<String>,
    pub version: Option<String>,
}

impl UpgradeOptions {
    pub fn get_ansible_vars(&self) -> String {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("interval", &self.interval.as_millis().to_string());
        if let Some(env_variables) = &self.env_variables {
            extra_vars.add_env_variable_list("env_variables", env_variables.clone());
        }
        if self.force {
            extra_vars.add_variable("force", &self.force.to_string());
        }
        if let Some(version) = &self.version {
            extra_vars.add_variable("antnode_version", version);
        }
        if let Some(pre_upgrade_delay) = &self.pre_upgrade_delay {
            extra_vars.add_variable("pre_upgrade_delay", &pre_upgrade_delay.to_string());
        }

        if let (Some(repo_owner), Some(branch)) = (&self.repo_owner, &self.branch) {
            let binary_option = BinaryOption::BuildFromSource {
                antnode_features: None,
                branch: branch.clone(),
                repo_owner: repo_owner.clone(),
                skip_binary_build: true,
            };
            extra_vars.add_node_url_or_version(&self.name, &binary_option);
        }

        extra_vars.build()
    }
}

#[derive(Default)]
pub struct TestnetDeployBuilder {
    ansible_forks: Option<usize>,
    ansible_verbose_mode: bool,
    deployment_type: EnvironmentType,
    environment_name: String,
    provider: Option<CloudProvider>,
    region: Option<String>,
    ssh_secret_key_path: Option<PathBuf>,
    state_bucket_name: Option<String>,
    terraform_binary_path: Option<PathBuf>,
    vault_password_path: Option<PathBuf>,
    working_directory_path: Option<PathBuf>,
}

impl TestnetDeployBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn ansible_verbose_mode(&mut self, ansible_verbose_mode: bool) -> &mut Self {
        self.ansible_verbose_mode = ansible_verbose_mode;
        self
    }

    pub fn ansible_forks(&mut self, ansible_forks: usize) -> &mut Self {
        self.ansible_forks = Some(ansible_forks);
        self
    }

    pub fn deployment_type(&mut self, deployment_type: EnvironmentType) -> &mut Self {
        self.deployment_type = deployment_type;
        self
    }

    pub fn environment_name(&mut self, name: &str) -> &mut Self {
        self.environment_name = name.to_string();
        self
    }

    pub fn provider(&mut self, provider: CloudProvider) -> &mut Self {
        self.provider = Some(provider);
        self
    }

    pub fn state_bucket_name(&mut self, state_bucket_name: String) -> &mut Self {
        self.state_bucket_name = Some(state_bucket_name);
        self
    }

    pub fn terraform_binary_path(&mut self, terraform_binary_path: PathBuf) -> &mut Self {
        self.terraform_binary_path = Some(terraform_binary_path);
        self
    }

    pub fn working_directory(&mut self, working_directory_path: PathBuf) -> &mut Self {
        self.working_directory_path = Some(working_directory_path);
        self
    }

    pub fn ssh_secret_key_path(&mut self, ssh_secret_key_path: PathBuf) -> &mut Self {
        self.ssh_secret_key_path = Some(ssh_secret_key_path);
        self
    }

    pub fn vault_password_path(&mut self, vault_password_path: PathBuf) -> &mut Self {
        self.vault_password_path = Some(vault_password_path);
        self
    }

    pub fn region(&mut self, region: String) -> &mut Self {
        self.region = Some(region);
        self
    }

    pub fn build(&self) -> Result<TestnetDeployer> {
        let provider = self.provider.unwrap_or(CloudProvider::DigitalOcean);
        match provider {
            CloudProvider::DigitalOcean => {
                let digital_ocean_pat = std::env::var("DO_PAT").map_err(|_| {
                    Error::CloudProviderCredentialsNotSupplied("DO_PAT".to_string())
                })?;
                // The DO_PAT variable is not actually read by either Terraform or Ansible.
                // Each tool uses a different variable, so instead we set each of those variables
                // to the value of DO_PAT. This means the user only needs to set one variable.
                std::env::set_var("DIGITALOCEAN_TOKEN", digital_ocean_pat.clone());
                std::env::set_var("DO_API_TOKEN", digital_ocean_pat);
            }
            _ => {
                return Err(Error::CloudProviderNotSupported(provider.to_string()));
            }
        }

        let state_bucket_name = match self.state_bucket_name {
            Some(ref bucket_name) => bucket_name.clone(),
            None => std::env::var("TERRAFORM_STATE_BUCKET_NAME")?,
        };

        let default_terraform_bin_path = PathBuf::from("terraform");
        let terraform_binary_path = self
            .terraform_binary_path
            .as_ref()
            .unwrap_or(&default_terraform_bin_path);

        let working_directory_path = match self.working_directory_path {
            Some(ref work_dir_path) => work_dir_path.clone(),
            None => std::env::current_dir()?.join("resources"),
        };

        let ssh_secret_key_path = match self.ssh_secret_key_path {
            Some(ref ssh_sk_path) => ssh_sk_path.clone(),
            None => PathBuf::from(std::env::var("SSH_KEY_PATH")?),
        };

        let vault_password_path = match self.vault_password_path {
            Some(ref vault_pw_path) => vault_pw_path.clone(),
            None => PathBuf::from(std::env::var("ANSIBLE_VAULT_PASSWORD_PATH")?),
        };

        let region = match self.region {
            Some(ref region) => region.clone(),
            None => "lon1".to_string(),
        };

        let terraform_runner = TerraformRunner::new(
            terraform_binary_path.to_path_buf(),
            working_directory_path
                .join("terraform")
                .join("testnet")
                .join(provider.to_string()),
            provider,
            &state_bucket_name,
        )?;
        let ansible_runner = AnsibleRunner::new(
            self.ansible_forks.unwrap_or(ANSIBLE_DEFAULT_FORKS),
            self.ansible_verbose_mode,
            &self.environment_name,
            provider,
            ssh_secret_key_path.clone(),
            vault_password_path,
            working_directory_path.join("ansible"),
        )?;
        let ssh_client = SshClient::new(ssh_secret_key_path);
        let ansible_provisioner =
            AnsibleProvisioner::new(ansible_runner, provider, ssh_client.clone());
        let rpc_client = RpcClient::new(
            PathBuf::from("/usr/local/bin/safenode_rpc_client"),
            working_directory_path.clone(),
        );

        // Remove any `safe` binary from a previous deployment. Otherwise you can end up with
        // mismatched binaries.
        let safe_path = working_directory_path.join("safe");
        if safe_path.exists() {
            std::fs::remove_file(safe_path)?;
        }

        let testnet = TestnetDeployer::new(
            ansible_provisioner,
            provider,
            self.deployment_type.clone(),
            &self.environment_name,
            rpc_client,
            S3Repository {},
            ssh_client,
            terraform_runner,
            working_directory_path,
            region,
        )?;

        Ok(testnet)
    }
}

#[derive(Clone)]
pub struct TestnetDeployer {
    pub ansible_provisioner: AnsibleProvisioner,
    pub cloud_provider: CloudProvider,
    pub deployment_type: EnvironmentType,
    pub environment_name: String,
    pub inventory_file_path: PathBuf,
    pub region: String,
    pub rpc_client: RpcClient,
    pub s3_repository: S3Repository,
    pub ssh_client: SshClient,
    pub terraform_runner: TerraformRunner,
    pub working_directory_path: PathBuf,
}

impl TestnetDeployer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ansible_provisioner: AnsibleProvisioner,
        cloud_provider: CloudProvider,
        deployment_type: EnvironmentType,
        environment_name: &str,
        rpc_client: RpcClient,
        s3_repository: S3Repository,
        ssh_client: SshClient,
        terraform_runner: TerraformRunner,
        working_directory_path: PathBuf,
        region: String,
    ) -> Result<TestnetDeployer> {
        if environment_name.is_empty() {
            return Err(Error::EnvironmentNameRequired);
        }
        let inventory_file_path = working_directory_path
            .join("ansible")
            .join("inventory")
            .join("dev_inventory_digital_ocean.yml");
        Ok(TestnetDeployer {
            ansible_provisioner,
            cloud_provider,
            deployment_type,
            environment_name: environment_name.to_string(),
            inventory_file_path,
            region,
            rpc_client,
            ssh_client,
            s3_repository,
            terraform_runner,
            working_directory_path,
        })
    }

    pub async fn init(&self) -> Result<()> {
        if self
            .s3_repository
            .folder_exists(
                "sn-testnet",
                &format!("testnet-logs/{}", self.environment_name),
            )
            .await?
        {
            return Err(Error::LogsForPreviousTestnetExist(
                self.environment_name.clone(),
            ));
        }

        self.terraform_runner.init()?;
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&self.environment_name) {
            self.terraform_runner
                .workspace_new(&self.environment_name)?;
        } else {
            println!("Workspace {} already exists", self.environment_name);
        }

        let rpc_client_path = self.working_directory_path.join("safenode_rpc_client");
        if !rpc_client_path.is_file() {
            println!("Downloading the rpc client for safenode...");
            let archive_name = "safenode_rpc_client-latest-x86_64-unknown-linux-musl.tar.gz";
            get_and_extract_archive_from_s3(
                &self.s3_repository,
                "sn-node-rpc-client",
                archive_name,
                &self.working_directory_path,
            )
            .await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&rpc_client_path)?.permissions();
                permissions.set_mode(0o755); // rwxr-xr-x
                std::fs::set_permissions(&rpc_client_path, permissions)?;
            }
        }

        Ok(())
    }

    pub fn plan(&self, options: &InfraRunOptions) -> Result<()> {
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;

        let args = build_terraform_args(options)?;

        self.terraform_runner
            .plan(Some(args), options.tfvars_filenames.clone())?;
        Ok(())
    }

    pub fn start(
        &self,
        interval: Duration,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.start_nodes(
            &self.environment_name,
            interval,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    pub fn apply_delete_node_records_cron(
        &self,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.apply_delete_node_records_cron(
            &self.environment_name,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    pub fn reset(
        &self,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.reset_nodes(
            &self.environment_name,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    /// Get the status of all nodes in a network.
    ///
    /// First, a playbook runs `safenode-manager status` against all the machines, to get the
    /// current state of all the nodes. Then all the node registry files are retrieved and
    /// deserialized to a `NodeRegistry`, allowing us to output the status of each node on each VM.
    pub async fn status(&self) -> Result<()> {
        self.ansible_provisioner.status()?;

        let peer_cache_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::PeerCacheNodes)
            .await?;
        let generic_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Nodes)
            .await?;
        let symmetric_private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::SymmetricPrivateNodes)
            .await?;
        let full_cone_private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::FullConePrivateNodes)
            .await?;
        let upnp_private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Upnp)
            .await?;
        let port_restricted_cone_private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::PortRestrictedConePrivateNodes)
            .await?;
        let genesis_node_registry = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Genesis)
            .await?
            .clone();

        peer_cache_node_registries.print().await;
        generic_node_registries.print().await;
        symmetric_private_node_registries.print().await;
        full_cone_private_node_registries.print().await;
        upnp_private_node_registries.print().await;
        genesis_node_registry.print().await;

        let all_registries = [
            &peer_cache_node_registries,
            &generic_node_registries,
            &symmetric_private_node_registries,
            &full_cone_private_node_registries,
            &upnp_private_node_registries,
            &genesis_node_registry,
        ];

        let mut total_nodes = 0;
        let mut running_nodes = 0;
        let mut stopped_nodes = 0;
        let mut added_nodes = 0;
        let mut removed_nodes = 0;

        for (_, registry) in all_registries
            .iter()
            .flat_map(|r| r.retrieved_registries.iter())
        {
            for node in registry.nodes.read().await.iter() {
                total_nodes += 1;
                match node.read().await.status {
                    ServiceStatus::Running => running_nodes += 1,
                    ServiceStatus::Stopped => stopped_nodes += 1,
                    ServiceStatus::Added => added_nodes += 1,
                    ServiceStatus::Removed => removed_nodes += 1,
                }
            }
        }

        let peer_cache_hosts = peer_cache_node_registries.retrieved_registries.len();
        let generic_hosts = generic_node_registries.retrieved_registries.len();
        let symmetric_private_hosts = symmetric_private_node_registries.retrieved_registries.len();
        let full_cone_private_hosts = full_cone_private_node_registries.retrieved_registries.len();
        let upnp_private_hosts = upnp_private_node_registries.retrieved_registries.len();
        let port_restricted_cone_private_hosts = port_restricted_cone_private_node_registries
            .retrieved_registries
            .len();

        let peer_cache_nodes = peer_cache_node_registries.get_node_count().await;
        let generic_nodes = generic_node_registries.get_node_count().await;
        let symmetric_private_nodes = symmetric_private_node_registries.get_node_count().await;
        let full_cone_private_nodes = full_cone_private_node_registries.get_node_count().await;
        let upnp_private_nodes = upnp_private_node_registries.get_node_count().await;
        let port_restricted_cone_private_nodes = port_restricted_cone_private_node_registries
            .get_node_count()
            .await;

        println!("-------");
        println!("Summary");
        println!("-------");
        println!(
            "Total peer cache nodes ({}x{}): {}",
            peer_cache_hosts,
            if peer_cache_hosts > 0 {
                peer_cache_nodes / peer_cache_hosts
            } else {
                0
            },
            peer_cache_nodes
        );
        println!(
            "Total generic nodes ({}x{}): {}",
            generic_hosts,
            if generic_hosts > 0 {
                generic_nodes / generic_hosts
            } else {
                0
            },
            generic_nodes
        );
        println!(
            "Total symmetric private nodes ({}x{}): {}",
            symmetric_private_hosts,
            if symmetric_private_hosts > 0 {
                symmetric_private_nodes / symmetric_private_hosts
            } else {
                0
            },
            symmetric_private_nodes
        );
        println!(
            "Total full cone private nodes ({}x{}): {}",
            full_cone_private_hosts,
            if full_cone_private_hosts > 0 {
                full_cone_private_nodes / full_cone_private_hosts
            } else {
                0
            },
            full_cone_private_nodes
        );
        println!(
            "Total UPnP private nodes ({}x{}): {}",
            upnp_private_hosts,
            if upnp_private_hosts > 0 {
                upnp_private_nodes / upnp_private_hosts
            } else {
                0
            },
            upnp_private_nodes
        );
        println!(
            "Total port restricted cone private nodes ({}x{}): {}",
            port_restricted_cone_private_hosts,
            if port_restricted_cone_private_hosts > 0 {
                port_restricted_cone_private_nodes / port_restricted_cone_private_hosts
            } else {
                0
            },
            port_restricted_cone_private_nodes
        );
        println!("Total nodes: {total_nodes}");
        println!("Running nodes: {running_nodes}");
        println!("Stopped nodes: {stopped_nodes}");
        println!("Added nodes: {added_nodes}");
        println!("Removed nodes: {removed_nodes}");

        Ok(())
    }

    pub fn cleanup_node_logs(&self, setup_cron: bool) -> Result<()> {
        self.ansible_provisioner.cleanup_node_logs(setup_cron)?;
        Ok(())
    }

    pub fn start_telegraf(
        &self,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.start_telegraf(
            &self.environment_name,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    pub fn stop(
        &self,
        interval: Duration,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
        delay: Option<u64>,
        service_names: Option<Vec<String>>,
    ) -> Result<()> {
        self.ansible_provisioner.stop_nodes(
            &self.environment_name,
            interval,
            node_type,
            custom_inventory,
            delay,
            service_names,
        )?;
        Ok(())
    }

    pub fn stop_telegraf(
        &self,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.stop_telegraf(
            &self.environment_name,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    pub fn upgrade(&self, options: UpgradeOptions) -> Result<()> {
        self.ansible_provisioner.upgrade_nodes(&options)?;
        Ok(())
    }

    pub fn upgrade_antctl(
        &self,
        version: Version,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner.upgrade_antctl(
            &self.environment_name,
            &version,
            node_type,
            custom_inventory,
        )?;
        Ok(())
    }

    pub fn upgrade_geoip_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_provisioner.upgrade_geoip_telegraf(name)?;
        Ok(())
    }

    pub fn upgrade_node_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_provisioner.upgrade_node_telegraf(name)?;
        Ok(())
    }

    pub fn upgrade_client_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_provisioner.upgrade_client_telegraf(name)?;
        Ok(())
    }

    pub async fn clean(&self) -> Result<()> {
        let environment_details =
            get_environment_details(&self.environment_name, &self.s3_repository)
                .await
                .inspect_err(|err| {
                    println!("Failed to get environment details: {err}. Continuing cleanup...");
                })
                .ok();
        if let Some(environment_details) = &environment_details {
            funding::drain_funds(&self.ansible_provisioner, environment_details).await?;
        }

        self.destroy_infra(environment_details).await?;

        cleanup_environment_inventory(
            &self.environment_name,
            &self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
            None,
        )?;

        println!("Deleted Ansible inventory for {}", self.environment_name);

        if let Err(err) = self
            .s3_repository
            .delete_object("sn-environment-type", &self.environment_name)
            .await
        {
            println!("Failed to delete environment type: {err}. Continuing cleanup...");
        }
        Ok(())
    }

    async fn destroy_infra(&self, environment_details: Option<EnvironmentDetails>) -> Result<()> {
        infra::select_workspace(&self.terraform_runner, &self.environment_name)?;

        let options = InfraRunOptions::generate_existing(
            &self.environment_name,
            &self.region,
            &self.terraform_runner,
            environment_details.as_ref(),
        )
        .await?;

        let args = build_terraform_args(&options)?;
        let tfvars_filenames = if let Some(environment_details) = &environment_details {
            environment_details
                .environment_type
                .get_tfvars_filenames(&self.environment_name, &self.region)
        } else {
            vec![]
        };

        self.terraform_runner
            .destroy(Some(args), Some(tfvars_filenames))?;

        infra::delete_workspace(&self.terraform_runner, &self.environment_name)?;

        Ok(())
    }
}

//
// Shared Helpers
//

pub fn get_genesis_multiaddr(
    ansible_runner: &AnsibleRunner,
    ssh_client: &SshClient,
) -> Result<Option<(String, IpAddr)>> {
    let genesis_inventory = ansible_runner.get_inventory(AnsibleInventoryType::Genesis, true)?;
    if genesis_inventory.is_empty() {
        return Ok(None);
    }
    let genesis_ip = genesis_inventory[0].public_ip_addr;

    // It's possible for the genesis host to be altered from its original state where a node was
    // started with the `--first` flag.
    // First attempt: try to find node with first=true
    let multiaddr = ssh_client
        .run_command(
            &genesis_ip,
            "root",
            "jq -r '.nodes[] | select(.initial_peers_config.first == true) | .listen_addr[] | select(contains(\"127.0.0.1\") | not) | select(contains(\"quic-v1\"))' /var/antctl/node_registry.json | head -n 1",
            false,
        )
        .map(|output| output.first().cloned())
        .unwrap_or_else(|err| {
            log::error!("Failed to find first node with quic-v1 protocol: {err:?}");
            None
        });

    // Second attempt: if first attempt failed, see if any node is available.
    let multiaddr = match multiaddr {
        Some(addr) => addr,
        None => ssh_client
            .run_command(
                &genesis_ip,
                "root",
                "jq -r '.nodes[] | .listen_addr[] | select(contains(\"127.0.0.1\") | not) | select(contains(\"quic-v1\"))' /var/antctl/node_registry.json | head -n 1",
                false,
            )?
            .first()
            .cloned()
            .ok_or_else(|| Error::GenesisListenAddress)?,
    };

    Ok(Some((multiaddr, genesis_ip)))
}

pub fn get_anvil_node_data_hardcoded(ansible_runner: &AnsibleRunner) -> Result<AnvilNodeData> {
    let evm_inventory = ansible_runner.get_inventory(AnsibleInventoryType::EvmNodes, true)?;
    if evm_inventory.is_empty() {
        return Err(Error::EvmNodeNotFound);
    }
    let evm_ip = evm_inventory[0].public_ip_addr;

    Ok(AnvilNodeData {
        data_payments_address: "0x8464135c8F25Da09e49BC8782676a84730C318bC".to_string(),
        deployer_wallet_private_key:
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
        merkle_payments_address: "0x663F3ad617193148711d28f5334eE4Ed07016602".to_string(),
        payment_token_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string(),
        rpc_url: format!("http://{evm_ip}:61611"),
    })
}

pub fn get_multiaddr(
    ansible_runner: &AnsibleRunner,
    ssh_client: &SshClient,
) -> Result<(String, IpAddr)> {
    let node_inventory = ansible_runner.get_inventory(AnsibleInventoryType::Nodes, true)?;
    // For upscaling a bootstrap deployment, we'd need to select one of the nodes that's already
    // provisioned. So just try the first one.
    let node_ip = node_inventory
        .iter()
        .find(|vm| vm.name.ends_with("-node-1"))
        .ok_or_else(|| Error::NodeAddressNotFound)?
        .public_ip_addr;

    debug!("Getting multiaddr from node {node_ip}");

    let multiaddr =
        ssh_client
        .run_command(
            &node_ip,
            "root",
            // fetch the first multiaddr which does not contain the localhost addr.
            "jq -r '.nodes[] | .listen_addr[] | select(contains(\"127.0.0.1\") | not)' /var/antctl/node_registry.json | head -n 1",
            false,
        )?.first()
        .cloned()
        .ok_or_else(|| Error::NodeAddressNotFound)?;

    // The node_ip is obviously inside the multiaddr, but it's just being returned as a
    // separate item for convenience.
    Ok((multiaddr, node_ip))
}

pub async fn get_and_extract_archive_from_s3(
    s3_repository: &S3Repository,
    bucket_name: &str,
    archive_bucket_path: &str,
    dest_path: &Path,
) -> Result<()> {
    // In this case, not using unwrap leads to having to provide a very trivial error variant that
    // doesn't seem very valuable.
    let archive_file_name = archive_bucket_path.split('/').next_back().unwrap();
    let archive_dest_path = dest_path.join(archive_file_name);
    s3_repository
        .download_object(bucket_name, archive_bucket_path, &archive_dest_path)
        .await?;
    extract_archive(&archive_dest_path, dest_path)?;
    Ok(())
}

pub fn extract_archive(archive_path: &Path, dest_path: &Path) -> Result<()> {
    let archive_file = File::open(archive_path)?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    let entries = archive.entries()?;
    for entry_result in entries {
        let mut entry = entry_result?;
        let extract_path = dest_path.join(entry.path()?);
        if entry.header().entry_type() == tar::EntryType::Directory {
            std::fs::create_dir_all(extract_path)?;
            continue;
        }
        let mut file = BufWriter::new(File::create(extract_path)?);
        std::io::copy(&mut entry, &mut file)?;
    }
    std::fs::remove_file(archive_path)?;
    Ok(())
}

pub fn run_external_command(
    binary_path: PathBuf,
    working_directory_path: PathBuf,
    args: Vec<String>,
    suppress_stdout: bool,
    suppress_stderr: bool,
) -> Result<Vec<String>> {
    let mut command = Command::new(binary_path.clone());
    for arg in &args {
        command.arg(arg);
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.current_dir(working_directory_path.clone());
    debug!("Running {binary_path:#?} with args {args:#?}");
    debug!("Working directory set to {working_directory_path:#?}");

    let mut child = command.spawn()?;
    let mut output_lines = Vec::new();

    if let Some(ref mut stdout) = child.stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line?;
            if !suppress_stdout {
                println!("{line}");
            }
            output_lines.push(line);
        }
    }

    if let Some(ref mut stderr) = child.stderr {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line?;
            if !suppress_stderr {
                eprintln!("{line}");
            }
            output_lines.push(line);
        }
    }

    let output = child.wait()?;
    if !output.success() {
        // Using `unwrap` here avoids introducing another error variant, which seems excessive.
        let binary_path = binary_path.to_str().unwrap();
        return Err(Error::ExternalCommandRunFailed {
            binary: binary_path.to_string(),
            exit_status: output,
        });
    }

    Ok(output_lines)
}

pub fn is_binary_on_path(binary_name: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let mut full_path = PathBuf::from(dir);
            full_path.push(binary_name);
            if full_path.exists() {
                return true;
            }
        }
    }
    false
}

pub fn get_wallet_directory() -> Result<PathBuf> {
    Ok(dirs_next::data_dir()
        .ok_or_else(|| Error::CouldNotRetrieveDataDirectory)?
        .join("safe")
        .join("client")
        .join("wallet"))
}

pub async fn notify_slack(inventory: DeploymentInventory) -> Result<()> {
    let webhook_url =
        std::env::var("SLACK_WEBHOOK_URL").map_err(|_| Error::SlackWebhookUrlNotSupplied)?;

    let mut message = String::new();
    message.push_str("*Testnet Details*\n");
    message.push_str(&format!("Name: {}\n", inventory.name));
    message.push_str(&format!("Node count: {}\n", inventory.peers().len()));
    message.push_str(&format!("Faucet address: {:?}\n", inventory.faucet_address));
    match inventory.binary_option {
        BinaryOption::BuildFromSource {
            ref repo_owner,
            ref branch,
            ..
        } => {
            message.push_str("*Branch Details*\n");
            message.push_str(&format!("Repo owner: {repo_owner}\n"));
            message.push_str(&format!("Branch: {branch}\n"));
        }
        BinaryOption::Versioned {
            ant_version: ref safe_version,
            antnode_version: ref safenode_version,
            antctl_version: ref safenode_manager_version,
            ..
        } => {
            message.push_str("*Version Details*\n");
            message.push_str(&format!(
                "ant version: {}\n",
                safe_version
                    .as_ref()
                    .map_or("None".to_string(), |v| v.to_string())
            ));
            message.push_str(&format!(
                "safenode version: {}\n",
                safenode_version
                    .as_ref()
                    .map_or("None".to_string(), |v| v.to_string())
            ));
            message.push_str(&format!(
                "antctl version: {}\n",
                safenode_manager_version
                    .as_ref()
                    .map_or("None".to_string(), |v| v.to_string())
            ));
        }
    }

    message.push_str("*Sample Peers*\n");
    message.push_str("```\n");
    for peer in inventory.peers().iter().take(20) {
        message.push_str(&format!("{peer}\n"));
    }
    message.push_str("```\n");
    message.push_str("*Available Files*\n");
    message.push_str("```\n");
    for (addr, file_name) in inventory.uploaded_files.iter() {
        message.push_str(&format!("{addr}: {file_name}\n"))
    }
    message.push_str("```\n");

    let payload = json!({
        "text": message,
    });
    reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await?;
    println!("{message}");
    println!("Posted notification to Slack");
    Ok(())
}

fn print_duration(duration: Duration) {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    debug!("Time taken: {minutes} minutes and {seconds} seconds");
}

pub fn get_progress_bar(length: u64) -> Result<ProgressBar> {
    let progress_bar = ProgressBar::new(length);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")?
            .progress_chars("#>-"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    Ok(progress_bar)
}

pub async fn get_environment_details(
    environment_name: &str,
    s3_repository: &S3Repository,
) -> Result<EnvironmentDetails> {
    let temp_file = tempfile::NamedTempFile::new()?;

    let max_retries = 3;
    let mut retries = 0;
    let env_details = loop {
        debug!("Downloading the environment details file for {environment_name} from S3");
        match s3_repository
            .download_object("sn-environment-type", environment_name, temp_file.path())
            .await
        {
            Ok(_) => {
                debug!("Downloaded the environment details file for {environment_name} from S3");
                let content = match std::fs::read_to_string(temp_file.path()) {
                    Ok(content) => content,
                    Err(err) => {
                        log::error!("Could not read the environment details file: {err:?}");
                        if retries < max_retries {
                            debug!("Retrying to read the environment details file");
                            retries += 1;
                            continue;
                        } else {
                            return Err(Error::EnvironmentDetailsNotFound(
                                environment_name.to_string(),
                            ));
                        }
                    }
                };
                trace!("Content of the environment details file: {content}");

                match serde_json::from_str(&content) {
                    Ok(environment_details) => break environment_details,
                    Err(err) => {
                        log::error!("Could not parse the environment details file: {err:?}");
                        if retries < max_retries {
                            debug!("Retrying to parse the environment details file");
                            retries += 1;
                            continue;
                        } else {
                            return Err(Error::EnvironmentDetailsNotFound(
                                environment_name.to_string(),
                            ));
                        }
                    }
                }
            }
            Err(err) => {
                log::error!(
                    "Could not download the environment details file for {environment_name} from S3: {err:?}"
                );
                if retries < max_retries {
                    retries += 1;
                    continue;
                } else {
                    return Err(Error::EnvironmentDetailsNotFound(
                        environment_name.to_string(),
                    ));
                }
            }
        }
    };

    debug!("Fetched environment details: {env_details:?}");

    Ok(env_details)
}

pub async fn write_environment_details(
    s3_repository: &S3Repository,
    environment_name: &str,
    environment_details: &EnvironmentDetails,
) -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let path = temp_dir.path().to_path_buf().join(environment_name);
    let mut file = File::create(&path)?;
    let json = serde_json::to_string(environment_details)?;
    file.write_all(json.as_bytes())?;
    s3_repository
        .upload_file("sn-environment-type", &path, true)
        .await?;
    Ok(())
}

pub fn calculate_size_per_attached_volume(node_count: u16) -> u16 {
    if node_count == 0 {
        return 0;
    }
    let total_volume_required = node_count * STORAGE_REQUIRED_PER_NODE;

    // 7 attached volumes per VM
    (total_volume_required as f64 / 7.0).ceil() as u16
}

pub fn get_bootstrap_cache_url(ip_addr: &IpAddr) -> String {
    format!("http://{ip_addr}/bootstrap_cache.json")
}
