// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod error;
pub mod rpc_client;
pub mod s3;
pub mod ssh;
pub mod terraform;

#[cfg(test)]
mod tests;

use crate::ansible::{AnsibleRunner, AnsibleRunnerInterface};
use crate::error::{Error, Result};
use crate::rpc_client::{RpcClient, RpcClientInterface};
use crate::s3::S3AssetRepository;
use crate::ssh::{SshClient, SshClientInterface};
use crate::terraform::{TerraformRunner, TerraformRunnerInterface};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
                .join(provider.to_string()),
            provider.clone(),
            &state_bucket_name,
        );
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
        let s3_repository = S3AssetRepository::new("https://sn-testnet.s3.eu-west-2.amazonaws.com");

        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            Box::new(ansible_runner),
            Box::new(rpc_client),
            Box::new(SshClient::new(ssh_secret_key_path)),
            working_directory_path,
            provider.clone(),
            s3_repository,
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
    pub s3_repository: S3AssetRepository,
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
        s3_repository: S3AssetRepository,
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
                .download_asset(asset_name, &archive_path)
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
        }

        Ok(())
    }

    pub async fn create_infra(
        &self,
        name: &str,
        vm_count: u16,
        enable_build_vm: bool,
    ) -> Result<()> {
        println!("Selecting {name} workspace...");
        self.terraform_runner.workspace_select(name)?;
        let args = vec![
            ("node_count".to_string(), vm_count.to_string()),
            ("use_custom_bin".to_string(), enable_build_vm.to_string()),
        ];
        println!("Running terraform apply...");
        self.terraform_runner.apply(args)?;
        Ok(())
    }

    pub async fn build_custom_safenode(
        &self,
        name: &str,
        repo_owner: &str,
        branch: &str,
    ) -> Result<()> {
        let build_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml")),
        )?;
        let build_ip = build_inventory[0].1.clone();
        self.ssh_client
            .wait_for_ssh_availability(&build_ip, "root")?;

        println!("Running ansible against build VM...");
        self.ansible_runner.run_playbook(
            PathBuf::from("build.yml"),
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml")),
            "root".to_string(),
            Some(format!(
                "{{ \"branch\": \"{branch}\", \"org\": \"{repo_owner}\", \"provider\": \"{}\" }}",
                self.cloud_provider
            )),
        )?;
        Ok(())
    }

    pub async fn provision_genesis_node(
        &self,
        name: &str,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
        let genesis_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
        )?;
        let genesis_ip = genesis_inventory[0].1.clone();
        self.ssh_client
            .wait_for_ssh_availability(&genesis_ip, "root")?;
        let extra_vars =
            self.build_extra_vars_doc(name, None, None, repo_owner.clone(), branch.clone())?;
        println!("{extra_vars}");
        println!("Running ansible against genesis node...");
        self.ansible_runner.run_playbook(
            PathBuf::from("genesis_node.yml"),
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
            "root".to_string(),
            Some(self.build_extra_vars_doc(name, None, None, repo_owner, branch)?),
        )?;
        Ok(())
    }

    pub async fn provision_remaining_nodes(
        &self,
        name: &str,
        genesis_multiaddr: String,
        node_instance_count: u16,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
        println!("Running ansible against remaining nodes...");
        self.ansible_runner.run_playbook(
            PathBuf::from("nodes.yml"),
            PathBuf::from("inventory").join(format!(".{name}_node_inventory_digital_ocean.yml")),
            "root".to_string(),
            Some(self.build_extra_vars_doc(
                name,
                Some(genesis_multiaddr),
                Some(node_instance_count),
                repo_owner,
                branch,
            )?),
        )?;
        Ok(())
    }

    pub async fn get_genesis_multiaddr(&self, name: &str) -> Result<String> {
        let genesis_inventory = self.ansible_runner.inventory_list(
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
        )?;
        let genesis_ip = genesis_inventory[0].1.clone();
        let node_info = self
            .rpc_client
            .get_info(format!("{}:12001", genesis_ip).parse()?)?;
        let multiaddr = format!("/ip4/{}/tcp/12000/p2p/{}", genesis_ip, node_info.peer_id);
        Ok(multiaddr)
    }

    pub async fn deploy(
        &self,
        name: &str,
        vm_count: u16,
        node_instance_count: u16,
        repo_owner: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
        if (repo_owner.is_some() && branch.is_none()) || (branch.is_some() && repo_owner.is_none())
        {
            return Err(Error::CustomBinConfigError);
        }

        self.create_infra(name, vm_count, repo_owner.is_some())
            .await?;
        if repo_owner.is_some() {
            self.build_custom_safenode(
                name,
                &repo_owner.as_ref().unwrap(),
                &branch.as_ref().unwrap(),
            )
            .await?;
        }
        self.provision_genesis_node(name, repo_owner.clone(), branch.clone())
            .await?;
        let multiaddr = self.get_genesis_multiaddr(name).await?;
        println!("Obtained multiaddr for genesis node: {multiaddr}");
        self.provision_remaining_nodes(name, multiaddr, node_instance_count, repo_owner, branch)
            .await?;
        Ok(())
    }

    pub async fn clean(&self, name: &str) -> Result<()> {
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }
        self.terraform_runner.workspace_select(name)?;
        println!("Selected {name} workspace");
        self.terraform_runner.destroy()?;
        // The 'dev' workspace is one we always expect to exist, for admin purposes.
        // You can't delete a workspace while it is selected, so we select 'dev' before we delete
        // the current workspace.
        self.terraform_runner.workspace_select("dev")?;
        self.terraform_runner.workspace_delete(name)?;
        println!("Deleted {name} workspace");

        let inventory_types = ["build", "genesis", "node"];
        for inventory_type in inventory_types.iter() {
            let inventory_file_path = self
                .working_directory_path
                .join("ansible")
                .join("inventory")
                .join(format!(
                    ".{}_{}_inventory_digital_ocean.yml",
                    name, inventory_type
                ));
            std::fs::remove_file(inventory_file_path)?;
        }
        println!("Deleted Ansible inventory for {name}");
        Ok(())
    }

    fn build_extra_vars_doc(
        &self,
        name: &str,
        genesis_multiaddr: Option<String>,
        node_instance_count: Option<u16>,
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
                    "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/safenode-latest-x86_64-unknown-linux-musl.tar.gz",
                    repo_owner.unwrap(),
                    branch.unwrap()),
            );
        }

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
    for arg in args {
        command.arg(arg);
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.current_dir(working_directory_path);
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
