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
pub mod rpc_client;
pub mod s3;
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
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
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
        let s3_repository = S3Repository::new("sn-testnet");

        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            Box::new(ansible_runner),
            Box::new(rpc_client),
            Box::new(SshClient::new(ssh_secret_key_path)),
            working_directory_path,
            provider.clone(),
            Box::new(s3_repository),
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
            .folder_exists(&format!("testnet-logs/{name}"))
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
            let asset_name = "rpc_client-latest-x86_64-unknown-linux-musl.tar.gz";
            let archive_path = self.working_directory_path.join(asset_name);
            self.s3_repository
                .download_object(asset_name, &archive_path)
                .await?;
            let archive_file = File::open(archive_path.clone())?;
            let decoder = GzDecoder::new(archive_file);
            let mut archive = Archive::new(decoder);
            let entries = archive.entries()?;
            for entry_result in entries {
                let mut entry = entry_result?;
                let mut file = BufWriter::new(File::create(
                    self.working_directory_path.join(entry.path()?),
                )?);
                std::io::copy(&mut entry, &mut file)?;
            }

            std::fs::remove_file(archive_path)?;
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
        repo_owner: Option<String>,
        branch: Option<String>,
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
        let extra_vars = self.build_binaries_extra_vars_doc(name, repo_owner, branch)?;
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
        logstash_stack_name: &str,
        logstash_hosts: &[SocketAddr],
        repo_owner: Option<String>,
        branch: Option<String>,
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
                repo_owner,
                branch,
                logstash_stack_name,
                logstash_hosts,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_faucet(
        &self,
        name: &str,
        genesis_multiaddr: &str,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy faucet...");
        self.ansible_runner.run_playbook(
            PathBuf::from("faucet.yml"),
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
            self.cloud_provider.get_ssh_user(),
            Some(self.build_faucet_extra_vars_doc(name, genesis_multiaddr, repo_owner, branch)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_remaining_nodes(
        &self,
        name: &str,
        logstash_stack_name: &str,
        logstash_hosts: &[SocketAddr],
        genesis_multiaddr: String,
        node_instance_count: u16,
        repo_owner: Option<String>,
        branch: Option<String>,
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
                repo_owner,
                branch,
                logstash_stack_name,
                logstash_hosts,
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
        logstash_stack_name: &str,
        logstash_hosts: &[SocketAddr],
        vm_count: u16,
        node_instance_count: u16,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
        if (repo_owner.is_some() && branch.is_none()) || (branch.is_some() && repo_owner.is_none())
        {
            return Err(Error::CustomBinConfigError);
        }

        self.create_infra(name, vm_count, true).await?;
        self.build_safe_network_binaries(name, repo_owner.clone(), branch.clone())
            .await?;
        self.provision_genesis_node(
            name,
            logstash_stack_name,
            logstash_hosts,
            repo_owner.clone(),
            branch.clone(),
        )
        .await?;
        let (multiaddr, genesis_ip) = self.get_genesis_multiaddr(name).await?;
        println!("Obtained multiaddr for genesis node: {multiaddr}");
        self.provision_faucet(name, &multiaddr, repo_owner.clone(), branch.clone())
            .await?;
        self.provision_remaining_nodes(
            name,
            logstash_stack_name,
            logstash_hosts,
            multiaddr,
            node_instance_count,
            repo_owner,
            branch,
        )
        .await?;
        // For reasons not known, the faucet service needs to be 'nudged' with a restart.
        // It seems to work fine after this.
        self.ssh_client.run_command(
            &genesis_ip,
            &self.cloud_provider.get_ssh_user(),
            "systemctl restart faucet",
        )?;
        self.list_inventory(name).await?;
        Ok(())
    }

    pub async fn list_inventory(&self, name: &str) -> Result<()> {
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

        println!("**************************************");
        println!("*                                    *");
        println!("*          Inventory Report          *");
        println!("*                                    *");
        println!("**************************************");

        let genesis_ip = &genesis_inventory[0].1;
        println!("{}: {}", genesis_inventory[0].0, genesis_ip);
        if !build_inventory.is_empty() {
            println!("{}: {}", build_inventory[0].0, build_inventory[0].1);
        }
        for entry in remaining_nodes_inventory.iter() {
            println!("{}: {}", entry.0, entry.1);
        }
        println!("SSH user: {}", self.cloud_provider.get_ssh_user());
        let (genesis_multiaddr, _) = self.get_genesis_multiaddr(name).await?;
        println!("Genesis multiaddr: {}", genesis_multiaddr);
        println!();

        // The scripts are relative to the `resources` directory, so we need to change the current
        // working directory back to that location first.
        std::env::set_current_dir(self.working_directory_path.clone())?;
        println!("Sample peers:");
        for (_, ip_address) in remaining_nodes_inventory {
            self.ssh_client.run_script(
                &ip_address,
                "safe",
                PathBuf::from("scripts").join("get_peer_multiaddr.sh"),
            )?;
        }
        println!();

        println!("Faucet address: {}:8000", genesis_ip);
        println!("Check the faucet:");
        println!("safe --peer {genesis_multiaddr} wallet get-faucet {genesis_ip}:8000");
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

    fn build_node_extra_vars_doc(
        &self,
        name: &str,
        genesis_multiaddr: Option<String>,
        node_instance_count: Option<u16>,
        repo_owner: Option<String>,
        branch: Option<String>,
        logstash_stack_name: &str,
        logstash_hosts: &[SocketAddr],
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
        if repo_owner.is_some() {
            Self::add_value(
                &mut extra_vars,
                "node_archive_url",
                &format!(
                    "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/safenode-{}-x86_64-unknown-linux-musl.tar.gz",
                    repo_owner.unwrap(),
                    branch.unwrap(),
                    name),
            );
        }
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
        repo_owner: Option<String>,
        branch: Option<String>,
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
        if let Some(branch) = branch {
            Self::add_value(&mut extra_vars, "branch", &branch);
        }
        if let Some(repo_owner) = repo_owner {
            Self::add_value(&mut extra_vars, "org", &repo_owner);
        }

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");
        Ok(extra_vars)
    }

    fn build_binaries_extra_vars_doc(
        &self,
        name: &str,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");

        if let Some(branch) = branch {
            Self::add_value(&mut extra_vars, "custom_safenode", "true");
            Self::add_value(&mut extra_vars, "branch", &branch);
        } else {
            Self::add_value(&mut extra_vars, "custom_safenode", "false");
        }
        if let Some(repo_owner) = repo_owner {
            Self::add_value(&mut extra_vars, "org", &repo_owner);
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

fn print_duration(duration: Duration) {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    debug!("Time taken: {} minutes and {} seconds", minutes, seconds);
}
