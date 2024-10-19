// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod bootstrap;
pub mod deploy;
pub mod digital_ocean;
pub mod error;
pub mod inventory;
pub mod logs;
pub mod logstash;
pub mod network_commands;
pub mod rpc_client;
pub mod s3;
pub mod safe;
pub mod setup;
pub mod ssh;
pub mod terraform;
pub mod upscale;

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
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    time::{Duration, Instant},
};
use tar::Archive;
use tokio::time::{sleep, Duration as TokioDuration};

const ANSIBLE_DEFAULT_FORKS: usize = 50;

#[derive(Clone, Debug)]
pub struct InfraRunOptions {
    pub bootstrap_node_vm_count: Option<u16>,
    pub enable_build_vm: bool,
    pub evm_node_count: Option<u16>,
    pub genesis_vm_count: Option<u16>,
    pub name: String,
    pub node_vm_count: Option<u16>,
    pub private_node_vm_count: Option<u16>,
    pub tfvars_filename: String,
    pub uploader_vm_count: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DeploymentType {
    /// The deployment has been bootstrapped from an existing network.
    Bootstrap,
    /// The deployment is a new network.
    #[default]
    New,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmCustomTestnetData {
    pub data_payments_address: String,
    pub deployer_wallet_private_key: String,
    pub payment_token_address: String,
    pub rpc_url: String,
}

impl std::fmt::Display for DeploymentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentType::Bootstrap => write!(f, "bootstrap"),
            DeploymentType::New => write!(f, "new"),
        }
    }
}

impl std::str::FromStr for DeploymentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bootstrap" => Ok(DeploymentType::Bootstrap),
            "new" => Ok(DeploymentType::New),
            _ => Err(format!("Invalid deployment type: {}", s)),
        }
    }
}

#[derive(Debug)]
pub enum NodeType {
    Bootstrap,
    Normal,
    Private,
}

impl NodeType {
    pub fn telegraph_role(&self) -> &'static str {
        match self {
            NodeType::Bootstrap => "BOOTSTRAP_NODE",
            NodeType::Normal => "GENERIC_NODE",
            NodeType::Private => "NAT_RANDOMIZED_NODE",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Serialize, Deserialize, PartialEq)]
pub enum EvmNetwork {
    #[default]
    ArbitrumOne,
    ArbitrumSepolia,
    Custom,
}

impl std::fmt::Display for EvmNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvmNetwork::ArbitrumOne => write!(f, "evm-arbitrum-one"),
            EvmNetwork::Custom => write!(f, "evm-custom"),
            EvmNetwork::ArbitrumSepolia => write!(f, "evm-arbitrum-sepolia"),
        }
    }
}

impl std::str::FromStr for EvmNetwork {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "arbitrum-one" => Ok(EvmNetwork::ArbitrumOne),
            "custom" => Ok(EvmNetwork::Custom),
            "arbitrum-sepolia" => Ok(EvmNetwork::ArbitrumSepolia),
            _ => Err(format!("Invalid EVM network type: {}", s)),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvironmentDetails {
    pub deployment_type: DeploymentType,
    pub environment_type: EnvironmentType,
    pub evm_network: EvmNetwork,
    pub evm_testnet_data: Option<EvmCustomTestnetData>,
    pub rewards_address: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum EnvironmentType {
    #[default]
    Development,
    Production,
    Staging,
}

impl EnvironmentType {
    pub fn get_tfvars_filename(&self) -> String {
        match self {
            EnvironmentType::Development => "dev.tfvars".to_string(),
            EnvironmentType::Production => "production.tfvars".to_string(),
            EnvironmentType::Staging => "staging.tfvars".to_string(),
        }
    }

    pub fn get_default_bootstrap_node_count(&self) -> u16 {
        match self {
            EnvironmentType::Development => 1,
            EnvironmentType::Production => 1,
            EnvironmentType::Staging => 1,
        }
    }

    pub fn get_default_node_count(&self) -> u16 {
        match self {
            EnvironmentType::Development => 25,
            EnvironmentType::Production => 25,
            EnvironmentType::Staging => 25,
        }
    }

    pub fn get_default_private_node_count(&self) -> u16 {
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

pub struct DeployOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub bootstrap_node_vm_count: u16,
    pub current_inventory: DeploymentInventory,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: u16,
    pub public_rpc: bool,
    pub uploader_vm_count: u16,
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
        branch: String,
        network_keys: Option<(String, String, String, String)>,
        protocol_version: Option<String>,
        repo_owner: String,
        /// A comma-separated list that will be passed to the `--features` argument.
        safenode_features: Option<String>,
    },
    /// Pre-built, versioned binaries will be fetched from S3.
    Versioned {
        faucet_version: Option<Version>,
        safe_version: Option<Version>,
        safenode_version: Version,
        safenode_manager_version: Version,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub custom_inventory: Option<Vec<VirtualMachine>>,
    pub env_variables: Option<Vec<(String, String)>>,
    pub faucet_version: Option<String>,
    pub force_faucet: bool,
    pub force_safenode: bool,
    pub forks: usize,
    pub interval: Duration,
    pub name: String,
    pub provider: CloudProvider,
    pub safenode_version: Option<String>,
}

impl UpgradeOptions {
    pub fn get_ansible_vars(&self) -> String {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("interval", &self.interval.as_millis().to_string());
        if let Some(env_variables) = &self.env_variables {
            extra_vars.add_env_variable_list("env_variables", env_variables.clone());
        }
        if self.force_faucet {
            extra_vars.add_variable("force_faucet", &self.force_faucet.to_string());
        }
        if let Some(version) = &self.faucet_version {
            extra_vars.add_variable("faucet_version", version);
        }
        if self.force_safenode {
            extra_vars.add_variable("force_safenode", &self.force_safenode.to_string());
        }
        if let Some(version) = &self.safenode_version {
            extra_vars.add_variable("safenode_version", version);
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

    pub async fn plan(
        &self,
        vars: Option<Vec<(String, String)>>,
        environment_type: EnvironmentType,
    ) -> Result<()> {
        println!("Selecting {} workspace...", self.environment_name);
        self.terraform_runner
            .workspace_select(&self.environment_name)?;
        self.terraform_runner
            .plan(vars, Some(environment_type.get_tfvars_filename()))?;
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        self.ansible_provisioner.start_nodes().await?;
        Ok(())
    }

    /// Get the status of all nodes in a network.
    ///
    /// First, a playbook runs `safenode-manager status` against all the machines, to get the
    /// current state of all the nodes. Then all the node registry files are retrieved and
    /// deserialized to a `NodeRegistry`, allowing us to output the status of each node on each VM.
    pub async fn status(&self) -> Result<()> {
        self.ansible_provisioner.status().await?;

        let bootstrap_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::BootstrapNodes)?;
        let generic_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Nodes)?;
        let private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::PrivateNodes)?;
        let genesis_node_registry = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Genesis)?
            .clone();

        bootstrap_node_registries.print();
        generic_node_registries.print();
        private_node_registries.print();
        genesis_node_registry.print();

        Ok(())
    }

    pub async fn cleanup_node_logs(&self, setup_cron: bool) -> Result<()> {
        self.ansible_provisioner
            .cleanup_node_logs(setup_cron)
            .await?;
        Ok(())
    }

    pub async fn start_telegraf(
        &self,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner
            .start_telegraf(&self.environment_name, custom_inventory)
            .await?;
        Ok(())
    }

    pub async fn stop_telegraf(&self, custom_inventory: Option<Vec<VirtualMachine>>) -> Result<()> {
        self.ansible_provisioner
            .stop_telegraf(&self.environment_name, custom_inventory)
            .await?;
        Ok(())
    }

    pub async fn upgrade(&self, options: UpgradeOptions) -> Result<()> {
        self.ansible_provisioner.upgrade_nodes(&options).await?;
        Ok(())
    }

    pub async fn upgrade_node_manager(
        &self,
        version: Version,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        self.ansible_provisioner
            .upgrade_node_manager(&self.environment_name, &version, custom_inventory)
            .await?;
        Ok(())
    }

    pub async fn upgrade_node_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_provisioner.upgrade_node_telegraf(name).await?;
        Ok(())
    }

    pub async fn upgrade_uploader_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_provisioner
            .upgrade_uploader_telegraf(name)
            .await?;
        Ok(())
    }

    pub async fn clean(&self) -> Result<()> {
        let environment_type =
            get_environment_details(&self.environment_name, &self.s3_repository).await?;
        do_clean(
            &self.environment_name,
            Some(environment_type.environment_type),
            self.working_directory_path.clone(),
            &self.terraform_runner,
            None,
        )?;
        self.s3_repository
            .delete_object("sn-environment-type", &self.environment_name)
            .await?;
        Ok(())
    }

    async fn create_or_update_infra(&self, options: &InfraRunOptions) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;

        let mut args = Vec::new();

        if let Some(genesis_vm_count) = options.genesis_vm_count {
            args.push(("genesis_vm_count".to_string(), genesis_vm_count.to_string()));
        }

        if let Some(bootstrap_node_vm_count) = options.bootstrap_node_vm_count {
            args.push((
                "bootstrap_node_vm_count".to_string(),
                bootstrap_node_vm_count.to_string(),
            ));
        }
        if let Some(node_vm_count) = options.node_vm_count {
            args.push(("node_vm_count".to_string(), node_vm_count.to_string()));
        }
        if let Some(private_node_vm_count) = options.private_node_vm_count {
            args.push((
                "private_node_vm_count".to_string(),
                private_node_vm_count.to_string(),
            ));
            args.push((
                "setup_nat_gateway".to_string(),
                (private_node_vm_count > 0).to_string(),
            ));
        }

        if let Some(evm_node_count) = options.evm_node_count {
            args.push((
                "evm_node_vm_count".to_string(),
                evm_node_count.to_string(),
            ));
        }

        if let Some(uploader_vm_count) = options.uploader_vm_count {
            args.push((
                "uploader_vm_count".to_string(),
                uploader_vm_count.to_string(),
            ));
        }
        args.push((
            "use_custom_bin".to_string(),
            options.enable_build_vm.to_string(),
        ));

        println!("Running terraform apply...");
        self.terraform_runner
            .apply(args, Some(options.tfvars_filename.clone()))?;
        print_duration(start.elapsed());
        Ok(())
    }
}

///
/// Shared Helpers
///

pub async fn get_genesis_multiaddr(
    ansible_runner: &AnsibleRunner,
    ssh_client: &SshClient,
) -> Result<(String, IpAddr)> {
    let genesis_inventory = ansible_runner
        .get_inventory(AnsibleInventoryType::Genesis, true)
        .await?;
    let genesis_ip = genesis_inventory[0].public_ip_addr;

    let multiaddr =
        ssh_client
        .run_command(
            &genesis_ip,
            "root",
            // fetch the first multiaddr if genesis is true and which does not contain the localhost addr.
            "jq -r '.nodes[] | select(.genesis == true) | .listen_addr[] | select(contains(\"127.0.0.1\") | not)' /var/safenode-manager/node_registry.json | head -n 1",
            false,
        )?.first()
        .cloned()
        .ok_or_else(|| Error::GenesisListenAddress)?;

    // The genesis_ip is obviously inside the multiaddr, but it's just being returned as a
    // separate item for convenience.
    Ok((multiaddr, genesis_ip))
}

pub async fn get_evm_testnet_data(
    ansible_runner: &AnsibleRunner,
    ssh_client: &SshClient,
) -> Result<EvmCustomTestnetData> {
    let evm_inventory = ansible_runner
        .get_inventory(AnsibleInventoryType::EvmNodes, true)
        .await?;
    if evm_inventory.is_empty() {
        return Err(Error::EvmNodeNotFound);
    }

    let evm_ip = evm_inventory[0].public_ip_addr;
    let csv_file_path = "/home/safe/.local/share/safe/evm_testnet_data.csv";

    const MAX_ATTEMPTS: u8 = 5;
    const RETRY_DELAY: TokioDuration = TokioDuration::from_secs(5);

    for attempt in 1..=MAX_ATTEMPTS {
        match ssh_client.run_command(&evm_ip, "safe", &format!("cat {}", csv_file_path), false) {
            Ok(output) => {
                if let Some(csv_contents) = output.first() {
                    let parts: Vec<&str> = csv_contents.split(',').collect();
                    if parts.len() != 4 {
                        return Err(Error::EvmTestnetDataParsingError(
                            "Expected 4 fields in the CSV".to_string(),
                        ));
                    }

                    let evm_testnet_data = EvmCustomTestnetData {
                        rpc_url: parts[0].trim().to_string(),
                        payment_token_address: parts[1].trim().to_string(),
                        data_payments_address: parts[2].trim().to_string(),
                        deployer_wallet_private_key: parts[3].trim().to_string(),
                    };
                    return Ok(evm_testnet_data);
                }
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(e);
                }
                println!(
                    "Attempt {} failed to read EVM testnet data. Retrying in {} seconds...",
                    attempt,
                    RETRY_DELAY.as_secs()
                );
            }
        }
        sleep(RETRY_DELAY).await;
    }

    Err(Error::EvmTestnetDataNotFound)
}

pub async fn get_multiaddr(
    ansible_runner: &AnsibleRunner,
    ssh_client: &SshClient,
) -> Result<(String, IpAddr)> {
    let node_inventory = ansible_runner
        .get_inventory(AnsibleInventoryType::Nodes, true)
        .await?;
    let node_ip = node_inventory[0].public_ip_addr;

    let multiaddr =
        ssh_client
        .run_command(
            &node_ip,
            "root",
            // fetch the first multiaddr which does not contain the localhost addr.
            "jq -r '.nodes[] | .listen_addr[] | select(contains(\"127.0.0.1\") | not)' /var/safenode-manager/node_registry.json | head -n 1",
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
    let archive_file_name = archive_bucket_path.split('/').last().unwrap();
    let archive_dest_path = dest_path.join(archive_file_name);
    s3_repository
        .download_object(bucket_name, archive_bucket_path, &archive_dest_path)
        .await?;
    extract_archive(&archive_dest_path, dest_path).await?;
    Ok(())
}

pub async fn extract_archive(archive_path: &Path, dest_path: &Path) -> Result<()> {
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

pub fn do_clean(
    name: &str,
    environment_type: Option<EnvironmentType>,
    working_directory_path: PathBuf,
    terraform_runner: &TerraformRunner,
    inventory_types: Option<Vec<AnsibleInventoryType>>,
) -> Result<()> {
    terraform_runner.init()?;
    let workspaces = terraform_runner.workspace_list()?;
    if !workspaces.contains(&name.to_string()) {
        return Err(Error::EnvironmentDoesNotExist(name.to_string()));
    }
    terraform_runner.workspace_select(name)?;
    println!("Selected {name} workspace");

    terraform_runner.destroy(environment_type.map(|et| et.get_tfvars_filename()))?;

    // The 'dev' workspace is one we always expect to exist, for admin purposes.
    // You can't delete a workspace while it is selected, so we select 'dev' before we delete
    // the current workspace.
    terraform_runner.workspace_select("dev")?;
    terraform_runner.workspace_delete(name)?;
    println!("Deleted {name} workspace");

    cleanup_environment_inventory(
        name,
        &working_directory_path.join("ansible").join("inventory"),
        inventory_types,
    )?;

    println!("Deleted Ansible inventory for {name}");
    Ok(())
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
            message.push_str(&format!("Repo owner: {}\n", repo_owner));
            message.push_str(&format!("Branch: {}\n", branch));
        }
        BinaryOption::Versioned {
            ref faucet_version,
            ref safe_version,
            ref safenode_version,
            ref safenode_manager_version,
            ..
        } => {
            message.push_str("*Version Details*\n");
            message.push_str(&format!(
                "faucet version: {}\n",
                faucet_version
                    .as_ref()
                    .map_or("None".to_string(), |v| v.to_string())
            ));
            message.push_str(&format!(
                "safe version: {}\n",
                safe_version
                    .as_ref()
                    .map_or("None".to_string(), |v| v.to_string())
            ));
            message.push_str(&format!("safenode version: {}\n", safenode_version));
            message.push_str(&format!(
                "safenode-manager version: {}\n",
                safenode_manager_version
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
        message.push_str(&format!("{}: {}\n", addr, file_name))
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
    debug!("Time taken: {} minutes and {} seconds", minutes, seconds);
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
    match s3_repository
        .download_object("sn-environment-type", environment_name, temp_file.path())
        .await
    {
        Ok(_) => {
            let content = std::fs::read_to_string(temp_file.path())?;
            match serde_json::from_str(&content) {
                Ok(environment_details) => Ok(environment_details),
                Err(_) => {
                    let lines: Vec<&str> = content.lines().collect();

                    let environment_type = if lines.is_empty() {
                        None
                    } else {
                        Some(lines[0].trim())
                    };

                    let deployment_type = lines.get(1).map(|s| s.trim());

                    let environment_type = match environment_type.and_then(|s| s.parse().ok()) {
                        Some(e) => {
                            debug!("Parsed the environment type from the S3 file");
                            e
                        }
                        None => {
                            debug!("Could not parse the environment type from the S3 file");
                            debug!(
                                "Falling back to using the environment name ({environment_name})"
                            );
                            if environment_name.starts_with("DEV") {
                                debug!("Using Development as the environment type");
                                EnvironmentType::Development
                            } else if environment_name.starts_with("STG") {
                                debug!("Using Staging as the environment type");
                                EnvironmentType::Staging
                            } else if environment_name.starts_with("PROD") {
                                debug!("Using Production as the environment type");
                                EnvironmentType::Production
                            } else {
                                return Err(Error::EnvironmentNameFromStringError(
                                    environment_name.to_string(),
                                ));
                            }
                        }
                    };

                    let deployment_type = match deployment_type.and_then(|s| s.parse().ok()) {
                        Some(d) => d,
                        None => {
                            debug!("Could not parse the deployment type from the S3 file");
                            debug!("Using New as the default");
                            DeploymentType::New
                        }
                    };

                    Ok(EnvironmentDetails {
                        environment_type,
                        deployment_type,
                        evm_network: EvmNetwork::ArbitrumOne,
                        rewards_address: "".to_string(),
                        evm_testnet_data: None,
                    })
                }
            }
        }
        Err(_) => Err(Error::EnvironmentDetailsNotFound(
            environment_name.to_string(),
        )),
    }
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
