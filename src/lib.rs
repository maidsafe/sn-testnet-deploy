// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod digital_ocean;
pub mod error;
pub mod logs;
pub mod logstash;
pub mod manage_test_data;
pub mod rpc_client;
pub mod s3;
pub mod safe;
pub mod setup;
pub mod ssh;
pub mod terraform;

#[cfg(test)]
mod tests;

use crate::ansible::{AnsibleRunner, AnsibleRunnerInterface};
use crate::error::{Error, Result};
use crate::rpc_client::{RpcClient, RpcClientInterface};
use crate::s3::{S3Repository, S3RepositoryInterface};
use crate::ssh::{SshClient, SshClientInterface};
use crate::terraform::{TerraformRunner, TerraformRunnerInterface};
use flate2::read::GzDecoder;
use log::debug;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tar::Archive;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub name: String,
    pub version_info: Option<(String, String)>,
    pub branch_info: Option<(String, String)>,
    pub vm_list: Vec<(String, String)>,
    pub node_count: u16,
    pub ssh_user: String,
    pub genesis_multiaddr: String,
    pub peers: Vec<String>,
    pub faucet_address: String,
    pub uploaded_files: Vec<(String, String)>,
}

impl DeploymentInventory {
    pub fn save(&self, file_path: &PathBuf) -> Result<()> {
        let serialized_data = serde_json::to_string_pretty(self)?;
        let mut file = File::create(file_path)?;
        file.write_all(serialized_data.as_bytes())?;
        Ok(())
    }

    pub fn read(file_path: &PathBuf) -> Result<Self> {
        let data = std::fs::read_to_string(file_path)?;
        let deserialized_data: DeploymentInventory = serde_json::from_str(&data)?;
        Ok(deserialized_data)
    }

    pub fn add_uploaded_files(&mut self, uploaded_files: Vec<(String, String)>) {
        self.uploaded_files.extend_from_slice(&uploaded_files);
    }

    pub fn get_random_peer(&self) -> String {
        let mut rng = rand::thread_rng();
        let i = rng.gen_range(0..self.peers.len());
        let random_peer = &self.peers[i];
        random_peer.to_string()
    }

    pub fn print_report(&self) {
        println!("**************************************");
        println!("*                                    *");
        println!("*          Inventory Report          *");
        println!("*                                    *");
        println!("**************************************");

        println!("Name: {}", self.name);
        if let Some((repo_owner, branch)) = &self.branch_info {
            println!("Branch Details");
            println!("==============");
            println!("Repo owner: {}", repo_owner);
            println!("Branch name: {}", branch);
        } else if let Some((safenode_version, safe_version)) = &self.version_info {
            println!("Version Details");
            println!("===============");
            println!("safenode version: {}", safenode_version);
            println!("safe version: {}", safe_version);
        }

        for vm in self.vm_list.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("SSH user: {}", self.ssh_user);
        println!("Sample Peers");
        println!("============");
        for peer in self.peers.iter() {
            println!("{peer}");
        }
        println!("Genesis multiaddr: {}", self.genesis_multiaddr);
        println!("Faucet address: {}", self.faucet_address);
        println!("Check the faucet:");
        println!(
            "safe --peer {} wallet get-faucet {}",
            self.genesis_multiaddr, self.faucet_address
        );

        if !self.uploaded_files.is_empty() {
            println!("Uploaded files:");
            for file in self.uploaded_files.iter() {
                println!("{}: {}", file.0, file.1);
            }
        }
    }
}

#[derive(Default)]
pub struct TestnetDeployBuilder {
    provider: Option<CloudProvider>,
    state_bucket_name: Option<String>,
    terraform_binary_path: Option<PathBuf>,
    working_directory_path: Option<PathBuf>,
    ssh_secret_key_path: Option<PathBuf>,
    vault_password_path: Option<PathBuf>,
}

impl TestnetDeployBuilder {
    pub fn new() -> Self {
        Default::default()
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

    pub fn build(&self) -> Result<TestnetDeploy> {
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
            working_directory_path.join("ansible"),
            provider.clone(),
            ssh_secret_key_path.clone(),
            vault_password_path,
        );
        let rpc_client = RpcClient::new(
            PathBuf::from("./safenode_rpc_client"),
            working_directory_path.clone(),
        );

        // Remove any `safe` binary from a previous deployment. Otherwise you can end up with
        // mismatched binaries.
        let safe_path = working_directory_path.join("safe");
        if safe_path.exists() {
            std::fs::remove_file(safe_path)?;
        }

        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            Box::new(ansible_runner),
            Box::new(rpc_client),
            Box::new(SshClient::new(ssh_secret_key_path)),
            working_directory_path,
            provider.clone(),
            Box::new(S3Repository {}),
        );

        Ok(testnet)
    }
}

pub struct TestnetDeploy {
    pub terraform_runner: Box<dyn TerraformRunnerInterface>,
    pub ansible_runner: Box<dyn AnsibleRunnerInterface>,
    pub rpc_client: Box<dyn RpcClientInterface>,
    pub ssh_client: Box<dyn SshClientInterface>,
    pub working_directory_path: PathBuf,
    pub cloud_provider: CloudProvider,
    pub s3_repository: Box<dyn S3RepositoryInterface>,
    pub inventory_file_path: PathBuf,
}

impl TestnetDeploy {
    pub fn new(
        terraform_runner: Box<dyn TerraformRunnerInterface>,
        ansible_runner: Box<dyn AnsibleRunnerInterface>,
        rpc_client: Box<dyn RpcClientInterface>,
        ssh_client: Box<dyn SshClientInterface>,
        working_directory_path: PathBuf,
        cloud_provider: CloudProvider,
        s3_repository: Box<dyn S3RepositoryInterface>,
    ) -> TestnetDeploy {
        let inventory_file_path = working_directory_path
            .join("ansible")
            .join("inventory")
            .join("dev_inventory_digital_ocean.yml");
        TestnetDeploy {
            terraform_runner,
            ansible_runner,
            rpc_client,
            ssh_client,
            working_directory_path,
            cloud_provider,
            s3_repository,
            inventory_file_path,
        }
    }

    pub async fn init(&self, name: &str) -> Result<()> {
        if self
            .s3_repository
            .folder_exists("sn-testnet", &format!("testnet-logs/{name}"))
            .await?
        {
            return Err(Error::LogsForPreviousTestnetExist(name.to_string()));
        }

        self.terraform_runner.init()?;
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&name.to_string()) {
            self.terraform_runner.workspace_new(name)?;
        } else {
            println!("Workspace {name} already exists")
        }

        let rpc_client_path = self.working_directory_path.join("safenode_rpc_client");
        if !rpc_client_path.is_file() {
            println!("Downloading the rpc client for safenode...");
            let archive_name = "rpc_client-latest-x86_64-unknown-linux-musl.tar.gz";
            get_and_extract_archive_from_s3(
                &*self.s3_repository,
                "sn-testnet",
                archive_name,
                &self.working_directory_path,
            )
            .await?;
            let mut permissions = std::fs::metadata(&rpc_client_path)?.permissions();
            permissions.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&rpc_client_path, permissions)?;
        }

        let inventory_files = ["build", "genesis", "node"];
        for inventory_type in inventory_files.iter() {
            let src_path = self.inventory_file_path.clone();
            let dest_path = self
                .working_directory_path
                .join("ansible")
                .join("inventory")
                .join(format!(
                    ".{}_{}_inventory_digital_ocean.yml",
                    name, inventory_type
                ));
            if dest_path.is_file() {
                // In this case 'init' has already been called before and the value has been
                // replaced, so just move on.
                continue;
            }

            let mut contents = std::fs::read_to_string(&src_path)?;
            contents = contents.replace("env_value", name);
            contents = contents.replace("type_value", inventory_type);
            std::fs::write(&dest_path, contents)?;
            debug!("Created inventory file at {dest_path:#?}");
        }

        Ok(())
    }

    pub async fn create_infra(
        &self,
        name: &str,
        vm_count: u16,
        enable_build_vm: bool,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {name} workspace...");
        self.terraform_runner.workspace_select(name)?;
        let args = vec![
            ("node_count".to_string(), vm_count.to_string()),
            ("use_custom_bin".to_string(), enable_build_vm.to_string()),
        ];
        println!("Running terraform apply...");
        self.terraform_runner.apply(args)?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn build_safe_network_binaries(
        &self,
        name: &str,
        custom_branch_details: Option<(String, String)>,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for build VM...");
        let build_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml")),
        )?;
        let build_ip = build_inventory[0].1.clone();
        self.ssh_client
            .wait_for_ssh_availability(&build_ip, &self.cloud_provider.get_ssh_user())?;

        println!("Running ansible against build VM...");
        let extra_vars = self.build_binaries_extra_vars_doc(name, custom_branch_details)?;
        self.ansible_runner.run_playbook(
            PathBuf::from("build.yml"),
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml")),
            self.cloud_provider.get_ssh_user(),
            Some(extra_vars),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_genesis_node(
        &self,
        name: &str,
        logstash_details: (&str, &[SocketAddr]),
        custom_branch_details: Option<(String, String)>,
        safenode_version: Option<String>,
    ) -> Result<()> {
        let start = Instant::now();
        let genesis_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
        )?;
        let genesis_ip = genesis_inventory[0].1.clone();
        self.ssh_client
            .wait_for_ssh_availability(&genesis_ip, &self.cloud_provider.get_ssh_user())?;
        println!("Running ansible against genesis node...");
        self.ansible_runner.run_playbook(
            PathBuf::from("genesis_node.yml"),
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
            self.cloud_provider.get_ssh_user(),
            Some(self.build_node_extra_vars_doc(
                name,
                None,
                None,
                custom_branch_details,
                logstash_details,
                safenode_version,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_faucet(
        &self,
        name: &str,
        genesis_multiaddr: &str,
        custom_branch_details: Option<(String, String)>,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy faucet...");
        self.ansible_runner.run_playbook(
            PathBuf::from("faucet.yml"),
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
            self.cloud_provider.get_ssh_user(),
            Some(self.build_faucet_extra_vars_doc(
                name,
                genesis_multiaddr,
                custom_branch_details,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_remaining_nodes(
        &self,
        name: &str,
        logstash_details: (&str, &[SocketAddr]),
        genesis_multiaddr: String,
        node_instance_count: u16,
        custom_branch_details: Option<(String, String)>,
        safenode_version: Option<String>,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against remaining nodes...");
        self.ansible_runner.run_playbook(
            PathBuf::from("nodes.yml"),
            PathBuf::from("inventory").join(format!(".{name}_node_inventory_digital_ocean.yml")),
            self.cloud_provider.get_ssh_user(),
            Some(self.build_node_extra_vars_doc(
                name,
                Some(genesis_multiaddr),
                Some(node_instance_count),
                custom_branch_details,
                logstash_details,
                safenode_version,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn get_genesis_multiaddr(&self, name: &str) -> Result<(String, String)> {
        let genesis_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
        )?;
        let genesis_ip = genesis_inventory[0].1.clone();
        let node_info = self
            .rpc_client
            .get_info(format!("{}:12001", genesis_ip).parse()?)?;
        let multiaddr = format!("/ip4/{}/tcp/12000/p2p/{}", genesis_ip, node_info.peer_id);
        // The genesis_ip is obviously inside the multiaddr, but it's just being returned as a
        // separate item for convenience.
        Ok((multiaddr, genesis_ip))
    }

    pub async fn deploy(
        &self,
        name: &str,
        logstash_details: (&str, &[SocketAddr]),
        vm_count: u16,
        node_instance_count: u16,
        custom_branch_details: Option<(String, String)>,
        custom_version_details: Option<(String, String)>,
    ) -> Result<()> {
        let safenode_version = custom_version_details.as_ref().map(|x| x.0.clone());
        self.create_infra(name, vm_count, true).await?;
        self.build_safe_network_binaries(name, custom_branch_details.clone())
            .await?;
        self.provision_genesis_node(
            name,
            logstash_details,
            custom_branch_details.clone(),
            safenode_version.clone(),
        )
        .await?;
        let (multiaddr, genesis_ip) = self.get_genesis_multiaddr(name).await?;
        println!("Obtained multiaddr for genesis node: {multiaddr}");
        self.provision_faucet(name, &multiaddr, custom_branch_details.clone())
            .await?;
        self.provision_remaining_nodes(
            name,
            logstash_details,
            multiaddr,
            node_instance_count,
            custom_branch_details.clone(),
            safenode_version,
        )
        .await?;
        // For reasons not known, the faucet service needs to be 'nudged' with a restart.
        // It seems to work fine after this.
        self.ssh_client.run_command(
            &genesis_ip,
            &self.cloud_provider.get_ssh_user(),
            "systemctl restart faucet",
        )?;
        self.list_inventory(
            name,
            true,
            custom_branch_details,
            custom_version_details,
            Some(node_instance_count),
        )
        .await?;
        Ok(())
    }

    /// Print the inventory for the deployment.
    ///
    /// It will be cached for the purposes of quick display later and also for use with the test
    /// data upload.
    ///
    /// The `force_regeneration` flag is used when the `deploy` command runs, to make sure that a
    /// new inventory is generated, because it's possible that an old one with the same deployment
    /// name has been cached.
    pub async fn list_inventory(
        &self,
        name: &str,
        force_regeneration: bool,
        custom_branch_info: Option<(String, String)>,
        version_info: Option<(String, String)>,
        node_instance_count: Option<u16>,
    ) -> Result<()> {
        let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
        if inventory_path.exists() && !force_regeneration {
            let inventory = DeploymentInventory::read(&inventory_path)?;
            inventory.print_report();
            return Ok(());
        }

        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        // The ansible runner will have its working directory set to this location. We need the
        // same here to test the inventory paths, which are relative to the `ansible` directory.
        let ansible_dir_path = self.working_directory_path.join("ansible");
        std::env::set_current_dir(ansible_dir_path.clone())?;

        // Somehow it might be possible that the workspace wasn't cleared out, but the environment
        // was actually torn down and the generated inventory files were deleted. If the files
        // don't exist, we can reasonably consider the environment non-existent.
        let genesis_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml"));
        let build_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml"));
        let remaining_nodes_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_node_inventory_digital_ocean.yml"));
        if !genesis_inventory_path.exists()
            || !build_inventory_path.exists()
            || !remaining_nodes_inventory_path.exists()
        {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let genesis_inventory = self.ansible_runner.inventory_list(genesis_inventory_path)?;
        let build_inventory = self.ansible_runner.inventory_list(build_inventory_path)?;
        let remaining_nodes_inventory = self
            .ansible_runner
            .inventory_list(remaining_nodes_inventory_path)?;

        // It also seems to be possible for a workspace and inventory files to still exist, but
        // there to be no inventory items returned. Perhaps someone deleted the VMs manually. We
        // only need to test the genesis node in this case, since that must always exist.
        if genesis_inventory.is_empty() {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let mut vm_list = Vec::new();
        vm_list.push((
            genesis_inventory[0].0.clone(),
            genesis_inventory[0].1.clone(),
        ));
        vm_list.push((build_inventory[0].0.clone(), build_inventory[0].1.clone()));
        for entry in remaining_nodes_inventory.iter() {
            vm_list.push((entry.0.clone(), entry.1.clone()));
        }
        let (genesis_multiaddr, genesis_ip) = self.get_genesis_multiaddr(name).await?;

        // The scripts are relative to the `resources` directory, so we need to change the current
        // working directory back to that location first.
        std::env::set_current_dir(self.working_directory_path.clone())?;
        let mut peers = Vec::new();
        println!("Retrieving sample peers. This can take several minutes.");
        for (_, ip_address) in remaining_nodes_inventory {
            let output = self.ssh_client.run_script(
                &ip_address,
                "safe",
                PathBuf::from("scripts").join("get_peer_multiaddr.sh"),
                true,
            )?;
            for line in output.iter() {
                peers.push(line.to_string());
            }
        }

        // The VM list includes the genesis node and the build machine, hence the subtraction of 2
        // from the total VM count. After that, add one node for genesis, since this machine only
        // runs a single node.
        let mut node_count = (vm_list.len() - 2) as u16 * node_instance_count.unwrap_or(0);
        node_count += 1;
        let inventory = DeploymentInventory {
            name: name.to_string(),
            branch_info: custom_branch_info,
            version_info,
            vm_list,
            node_count,
            ssh_user: self.cloud_provider.get_ssh_user(),
            genesis_multiaddr,
            peers,
            faucet_address: format!("{}:8000", genesis_ip),
            uploaded_files: Vec::new(),
        };
        inventory.print_report();
        inventory.save(&inventory_path)?;

        Ok(())
    }

    pub async fn clean(&self, name: &str) -> Result<()> {
        do_clean(
            name,
            self.working_directory_path.clone(),
            &*self.terraform_runner,
            vec![
                "build".to_string(),
                "genesis".to_string(),
                "node".to_string(),
            ],
        )
    }

    ///
    /// Private Helpers
    ///
    fn build_node_extra_vars_doc(
        &self,
        name: &str,
        genesis_multiaddr: Option<String>,
        node_instance_count: Option<u16>,
        custom_branch_details: Option<(String, String)>,
        logstash_details: (&str, &[SocketAddr]),
        safenode_version: Option<String>,
    ) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");
        Self::add_value(
            &mut extra_vars,
            "provider",
            &self.cloud_provider.to_string(),
        );
        Self::add_value(&mut extra_vars, "testnet_name", name);
        if genesis_multiaddr.is_some() {
            Self::add_value(
                &mut extra_vars,
                "genesis_multiaddr",
                &genesis_multiaddr.ok_or_else(|| Error::GenesisMultiAddrNotSupplied)?,
            );
        }
        if node_instance_count.is_some() {
            Self::add_value(
                &mut extra_vars,
                "node_instance_count",
                &node_instance_count.unwrap_or(20).to_string(),
            );
        }
        if let Some((repo_owner, branch)) = custom_branch_details {
            Self::add_value(
                &mut extra_vars,
                "node_archive_url",
                &format!(
                    "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/safenode-{}-x86_64-unknown-linux-musl.tar.gz",
                    repo_owner,
                    branch,
                    name),
            );
        }
        if let Some(version) = safenode_version {
            Self::add_value(
                &mut extra_vars,
                "node_archive_url",
                &format!(
                    "https://github.com/maidsafe/safe_network/releases/download/sn_node-v{version}/safenode-{version}-x86_64-unknown-linux-musl.tar.gz",
                ),
            );
        }

        let (logstash_stack_name, logstash_hosts) = logstash_details;
        Self::add_value(&mut extra_vars, "logstash_stack_name", logstash_stack_name);
        extra_vars.push_str("\"logstash_hosts\": [");
        for host in logstash_hosts.iter() {
            extra_vars.push_str(&format!("\"{}\", ", host));
        }
        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str("] }");
        Ok(extra_vars)
    }

    fn build_faucet_extra_vars_doc(
        &self,
        name: &str,
        genesis_multiaddr: &str,
        custom_branch_details: Option<(String, String)>,
    ) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");
        Self::add_value(
            &mut extra_vars,
            "provider",
            &self.cloud_provider.to_string(),
        );
        Self::add_value(&mut extra_vars, "testnet_name", name);
        Self::add_value(&mut extra_vars, "genesis_multiaddr", genesis_multiaddr);
        if let Some((repo_owner, branch)) = custom_branch_details {
            Self::add_value(&mut extra_vars, "branch", &branch);
            Self::add_value(&mut extra_vars, "org", &repo_owner);
        }

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");
        Ok(extra_vars)
    }

    fn build_binaries_extra_vars_doc(
        &self,
        name: &str,
        custom_branch_details: Option<(String, String)>,
    ) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");

        if let Some((repo_owner, branch)) = custom_branch_details {
            Self::add_value(&mut extra_vars, "custom_bin", "true");
            Self::add_value(&mut extra_vars, "branch", &branch);
            Self::add_value(&mut extra_vars, "org", &repo_owner);
        } else {
            Self::add_value(&mut extra_vars, "custom_bin", "false");
        }
        Self::add_value(&mut extra_vars, "testnet_name", name);

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");

        Ok(extra_vars)
    }

    fn add_value(document: &mut String, name: &str, value: &str) {
        document.push_str(&format!("\"{name}\": \"{value}\", "))
    }
}

///
/// Shared Helpers
///
pub async fn get_and_extract_archive_from_s3(
    s3_repository: &dyn S3RepositoryInterface,
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
    suppress_output: bool,
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
            if !suppress_output {
                println!("{}", &line);
            }
            output_lines.push(line);
        }
    }

    if let Some(ref mut stderr) = child.stderr {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line?;
            if !suppress_output {
                println!("{}", &line);
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
    terraform_runner: &dyn TerraformRunnerInterface,
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

pub fn get_data_directory() -> Result<PathBuf> {
    let path = dirs_next::data_dir()
        .ok_or_else(|| Error::CouldNotRetrieveDataDirectory)?
        .join("safe")
        .join("testnet-deploy");
    if !path.exists() {
        std::fs::create_dir_all(path.clone())?;
    }
    Ok(path)
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
    message.push_str(&format!("Node count: {}\n", inventory.node_count));
    message.push_str(&format!("Faucet address: {}\n", inventory.faucet_address));
    if let Some((repo_owner, branch)) = inventory.branch_info {
        message.push_str("*Branch Details*\n");
        message.push_str(&format!("Repo owner: {}\n", repo_owner));
        message.push_str(&format!("Branch: {}\n", branch));
    } else if let Some((safenode_version, safe_version)) = inventory.version_info {
        message.push_str("*Version Details*\n");
        message.push_str(&format!("safenode version: {}\n", safenode_version));
        message.push_str(&format!("safe version: {}\n", safe_version));
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
