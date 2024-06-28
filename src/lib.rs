// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod deploy;
pub mod digital_ocean;
pub mod error;
pub mod inventory;
pub mod logs;
pub mod logstash;
pub mod manage_test_data;
pub mod network_commands;
pub mod rpc_client;
pub mod s3;
pub mod safe;
pub mod setup;
pub mod ssh;
pub mod terraform;

use crate::{
    ansible::{AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner, ExtraVarsDocBuilder},
    error::{Error, Result},
    inventory::DeploymentInventory,
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
    io::{BufRead, BufReader, BufWriter},
    net::IpAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tar::Archive;

const ANSIBLE_DEFAULT_FORKS: usize = 50;

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
        repo_owner: String,
        branch: String,
        /// A comma-separated list that will be passed to the `--features` argument.
        safenode_features: Option<String>,
        protocol_version: Option<String>,
    },
    /// Pre-built, versioned binaries will be fetched from S3.
    Versioned {
        faucet_version: Version,
        safenode_version: Version,
        safenode_manager_version: Version,
        sn_auditor_version: Version,
    },
}

#[derive(Debug, Clone)]
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
    pub env_variables: Option<Vec<(String, String)>>,
    pub faucet_version: Option<String>,
    pub force_faucet: bool,
    pub force_safenode: bool,
    pub forks: usize,
    pub name: String,
    pub provider: CloudProvider,
    pub safenode_version: Option<String>,
}

impl UpgradeOptions {
    pub fn get_ansible_vars(&self) -> String {
        let mut extra_vars = ExtraVarsDocBuilder::default();
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
        let provider = self
            .provider
            .as_ref()
            .unwrap_or(&CloudProvider::DigitalOcean);
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
            provider.clone(),
            &state_bucket_name,
        )?;
        let ansible_runner = AnsibleRunner::new(
            self.ansible_forks.unwrap_or(ANSIBLE_DEFAULT_FORKS),
            self.ansible_verbose_mode,
            &self.environment_name,
            provider.clone(),
            ssh_secret_key_path.clone(),
            vault_password_path,
            working_directory_path.join("ansible"),
        )?;
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
            ansible_runner,
            provider.clone(),
            &self.environment_name,
            rpc_client,
            S3Repository {},
            SshClient::new(ssh_secret_key_path),
            terraform_runner,
            working_directory_path,
        )?;

        Ok(testnet)
    }
}

#[derive(Clone)]
pub struct TestnetDeployer {
    pub ansible_runner: AnsibleRunner,
    pub cloud_provider: CloudProvider,
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
        ansible_runner: AnsibleRunner,
        cloud_provider: CloudProvider,
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
            ansible_runner,
            cloud_provider,
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
            let mut permissions = std::fs::metadata(&rpc_client_path)?.permissions();
            permissions.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&rpc_client_path, permissions)?;
        }

        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartNodes,
            AnsibleInventoryType::Genesis,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartNodes,
            AnsibleInventoryType::Nodes,
            None,
        )?;

        Ok(())
    }

    pub async fn upgrade(&self, options: UpgradeOptions) -> Result<()> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&options.name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(options.name.to_string()));
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodes,
            AnsibleInventoryType::Nodes,
            Some(options.get_ansible_vars()),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodes,
            AnsibleInventoryType::Genesis,
            Some(options.get_ansible_vars()),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeFaucet,
            AnsibleInventoryType::Genesis,
            Some(options.get_ansible_vars()),
        )?;

        Ok(())
    }

    pub async fn upgrade_node_manager(&self, name: &str, version: Version) -> Result<()> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("version", &version.to_string());
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeManager,
            AnsibleInventoryType::Genesis,
            Some(extra_vars.build()),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeManager,
            AnsibleInventoryType::Nodes,
            Some(extra_vars.build()),
        )?;

        Ok(())
    }

    pub async fn clean(&self) -> Result<()> {
        do_clean(
            &self.environment_name,
            self.working_directory_path.clone(),
            &self.terraform_runner,
            vec![
                "build".to_string(),
                "genesis".to_string(),
                "node".to_string(),
            ],
        )
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
    let genesis_ip = genesis_inventory[0].1;

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
        return Err(Error::ExternalCommandRunFailed(binary_path.to_string()));
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
    working_directory_path: PathBuf,
    terraform_runner: &TerraformRunner,
    inventory_types: Vec<String>,
) -> Result<()> {
    terraform_runner.init()?;
    let workspaces = terraform_runner.workspace_list()?;
    if !workspaces.contains(&name.to_string()) {
        return Err(Error::EnvironmentDoesNotExist(name.to_string()));
    }
    terraform_runner.workspace_select(name)?;
    println!("Selected {name} workspace");
    terraform_runner.destroy()?;
    // The 'dev' workspace is one we always expect to exist, for admin purposes.
    // You can't delete a workspace while it is selected, so we select 'dev' before we delete
    // the current workspace.
    terraform_runner.workspace_select("dev")?;
    terraform_runner.workspace_delete(name)?;
    println!("Deleted {name} workspace");

    for inventory_type in inventory_types.iter() {
        let inventory_file_path = working_directory_path
            .join("ansible")
            .join("inventory")
            .join(format!(
                ".{}_{}_inventory_digital_ocean.yml",
                name, inventory_type
            ));
        if inventory_file_path.exists() {
            debug!("Removing inventory file at {inventory_file_path:#?}");
            std::fs::remove_file(inventory_file_path)?;
        }
    }
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
    message.push_str(&format!("Node count: {}\n", inventory.peers.len()));
    message.push_str(&format!("Faucet address: {:?}\n", inventory.faucet_address));
    match inventory.binary_option {
        BinaryOption::BuildFromSource {
            repo_owner, branch, ..
        } => {
            message.push_str("*Branch Details*\n");
            message.push_str(&format!("Repo owner: {}\n", repo_owner));
            message.push_str(&format!("Branch: {}\n", branch));
        }
        BinaryOption::Versioned {
            faucet_version,
            safenode_version,
            safenode_manager_version,
            sn_auditor_version,
        } => {
            message.push_str("*Version Details*\n");
            message.push_str(&format!("faucet version: {}\n", faucet_version));
            message.push_str(&format!("safenode version: {}\n", safenode_version));
            message.push_str(&format!(
                "safenode-manager version: {}\n",
                safenode_manager_version
            ));
            message.push_str(&format!("sn_auditor version: {}\n", sn_auditor_version));
        }
    }

    message.push_str("*Sample Peers*\n");
    message.push_str("```\n");
    for peer in inventory.peers.iter().take(20) {
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
