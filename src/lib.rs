// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod ansible;
pub mod deploy;
pub mod digital_ocean;
pub mod error;
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
    ansible::{generate_inventory, AnsibleRunner, ExtraVarsDocBuilder},
    error::{Error, Result},
    rpc_client::RpcClient,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
};
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, trace};
use rand::Rng;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    net::{IpAddr, SocketAddr},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tar::Archive;
use walkdir::WalkDir;

/// How or where to build the binaries from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SnCodebaseType {
    /// The latest release binaries from `maidsafe/safe_network` is used if the `safenode_features` are not provided.
    /// If the `safenode_features` are provided, the build VM is used to build the `safenode` binary, while the latest
    /// release are fetched for the rest of the binaries.
    Main {
        // CSV of features to enable on main::safenode
        safenode_features: Option<String>,
    },
    /// The build VM is used to build all the binaries that we will be using.
    Branch {
        repo_owner: String,
        branch: String,
        // CSV of features to enable on the custom branch::safenode
        safenode_features: Option<String>,
    },
    /// The specific versions of `safe` and `safenode` are fetched from `maidsafe/safe_network/releases`
    Versioned {
        faucet_version: Version,
        safe_version: Version,
        safenode_version: Version,
        safenode_manager_version: Version,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub name: String,
    pub node_count: u16,
    pub sn_codebase_type: SnCodebaseType,
    pub vm_list: Vec<(String, IpAddr)>,
    // Map of PeerId to SocketAddr
    pub rpc_endpoints: BTreeMap<String, SocketAddr>,
    // Map of PeerId to manager daemon SocketAddr
    pub safenodemand_endpoints: BTreeMap<String, SocketAddr>,
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

    pub fn print_report(&self) -> Result<()> {
        println!("**************************************");
        println!("*                                    *");
        println!("*          Inventory Report          *");
        println!("*                                    *");
        println!("**************************************");

        println!("Name: {}", self.name);
        match &self.sn_codebase_type {
            SnCodebaseType::Main { .. } => {}
            SnCodebaseType::Branch {
                repo_owner, branch, ..
            } => {
                println!("Branch Details");
                println!("==============");
                println!("Repo owner: {}", repo_owner);
                println!("Branch name: {}", branch);
            }
            SnCodebaseType::Versioned {
                faucet_version,
                safe_version,
                safenode_version,
                safenode_manager_version,
            } => {
                println!("Version Details");
                println!("===============");
                println!("faucet version: {}", faucet_version);
                println!("safe version: {}", safe_version);
                println!("safenode version: {}", safenode_version);
                println!("safenode-manager version: {}", safenode_manager_version);
            }
        }

        for vm in self.vm_list.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("SSH user: {}", self.ssh_user);
        let testnet_dir = get_data_directory()?;
        println!("Sample Peers",);
        println!("============");
        for peer in self.peers.iter().take(10) {
            println!("{peer}");
        }
        println!("The entire peer list can be found at {testnet_dir:?}",);

        println!("\nGenesis multiaddr: {}", self.genesis_multiaddr);
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
        Ok(())
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
    ansible_verbose_mode: bool,
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

    pub fn ansible_verbose_mode(&mut self, ansible_verbose_mode: bool) -> &mut Self {
        self.ansible_verbose_mode = ansible_verbose_mode;
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
            self.ansible_verbose_mode,
        );
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

        let testnet = TestnetDeploy::new(
            terraform_runner,
            ansible_runner,
            rpc_client,
            SshClient::new(ssh_secret_key_path),
            working_directory_path,
            provider.clone(),
            S3Repository {},
        );

        Ok(testnet)
    }
}

pub struct TestnetDeploy {
    pub terraform_runner: TerraformRunner,
    pub ansible_runner: AnsibleRunner,
    pub rpc_client: RpcClient,
    pub ssh_client: SshClient,
    pub working_directory_path: PathBuf,
    pub cloud_provider: CloudProvider,
    pub s3_repository: S3Repository,
    pub inventory_file_path: PathBuf,
}

impl TestnetDeploy {
    pub fn new(
        terraform_runner: TerraformRunner,
        ansible_runner: AnsibleRunner,
        rpc_client: RpcClient,
        ssh_client: SshClient,
        working_directory_path: PathBuf,
        cloud_provider: CloudProvider,
        s3_repository: S3Repository,
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

        generate_inventory(
            name,
            &self.inventory_file_path,
            &self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
        )
        .await?;

        Ok(())
    }

    pub async fn get_genesis_multiaddr(&self, name: &str) -> Result<(String, IpAddr)> {
        let genesis_inventory = self
            .ansible_runner
            .inventory_list(
                PathBuf::from("inventory")
                    .join(format!(".{name}_genesis_inventory_digital_ocean.yml")),
                true,
            )
            .await?;
        let genesis_ip = genesis_inventory[0].1;

        let multiaddr = self
            .ssh_client
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
        sn_codebase_type: SnCodebaseType,
        node_instance_count: Option<u16>,
    ) -> Result<()> {
        let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
        if inventory_path.exists() && !force_regeneration {
            let inventory = DeploymentInventory::read(&inventory_path)?;
            inventory.print_report()?;
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

        let (build_inventory_path, genesis_inventory_path, remaining_nodes_inventory_path) =
            if force_regeneration {
                generate_inventory(
                    name,
                    &self.inventory_file_path,
                    &self
                        .working_directory_path
                        .join("ansible")
                        .join("inventory"),
                )
                .await?
            } else {
                // Somehow it might be possible that the workspace wasn't cleared out, but the
                // environment was actually torn down and the generated inventory files were deleted.
                // If the files don't exist, we can reasonably consider the environment non-existent.
                let genesis_inventory_path = PathBuf::from("inventory")
                    .join(format!(".{name}_genesis_inventory_digital_ocean.yml"));
                let build_inventory_path = PathBuf::from("inventory")
                    .join(format!(".{name}_build_inventory_digital_ocean.yml"));
                let remaining_nodes_inventory_path = PathBuf::from("inventory")
                    .join(format!(".{name}_node_inventory_digital_ocean.yml"));
                if !genesis_inventory_path.exists()
                    || !build_inventory_path.exists()
                    || !remaining_nodes_inventory_path.exists()
                {
                    return Err(Error::EnvironmentDoesNotExist(name.to_string()));
                }

                (
                    build_inventory_path,
                    genesis_inventory_path,
                    remaining_nodes_inventory_path,
                )
            };

        let genesis_inventory = self
            .ansible_runner
            .inventory_list(genesis_inventory_path.clone(), false)
            .await?;
        let build_inventory = self
            .ansible_runner
            .inventory_list(build_inventory_path, false)
            .await?;
        let remaining_nodes_inventory = self
            .ansible_runner
            .inventory_list(remaining_nodes_inventory_path.clone(), false)
            .await?;

        // It also seems to be possible for a workspace and inventory files to still exist, but
        // there to be no inventory items returned. Perhaps someone deleted the VMs manually. We
        // only need to test the genesis node in this case, since that must always exist.
        if genesis_inventory.is_empty() {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let mut vm_list = Vec::new();
        vm_list.push((genesis_inventory[0].0.clone(), genesis_inventory[0].1));
        if !build_inventory.is_empty() {
            vm_list.push((build_inventory[0].0.clone(), build_inventory[0].1));
        }
        for entry in remaining_nodes_inventory.iter() {
            vm_list.push((entry.0.clone(), entry.1));
        }
        let (genesis_multiaddr, genesis_ip) = self.get_genesis_multiaddr(name).await?;

        println!("Retrieving node manager inventory. This can take a minute.");

        let node_manager_inventories = {
            debug!("Fetching node manager inventory");
            let temp_dir_path = tempfile::tempdir()?.into_path();
            let temp_dir_json = serde_json::to_string(&temp_dir_path)?;

            self.ansible_runner.run_playbook(
                PathBuf::from("node_manager_inventory.yml"),
                remaining_nodes_inventory_path,
                self.cloud_provider.get_ssh_user(),
                Some(format!("{{ \"dest\": {temp_dir_json} }}")),
            )?;
            self.ansible_runner.run_playbook(
                PathBuf::from("node_manager_inventory.yml"),
                genesis_inventory_path,
                self.cloud_provider.get_ssh_user(),
                Some(format!("{{ \"dest\": {temp_dir_json} }}")),
            )?;

            // collect the manager inventory file paths along with their respective ip addr
            let manager_inventory_files = WalkDir::new(temp_dir_path)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    if entry.file_type().is_file()
                        && entry.path().extension().is_some_and(|ext| ext == "json")
                    {
                        // tempdir/<testnet_name>-node/var/safenode-manager/node_registry.json
                        let mut vm_name = entry.path().to_path_buf();
                        trace!("Found file with json extension: {vm_name:?}");
                        vm_name.pop();
                        vm_name.pop();
                        vm_name.pop();
                        // Extract the <testnet_name>-node string
                        trace!("Extracting the vm name from the path");
                        let vm_name = vm_name.file_name()?.to_str()?;
                        trace!("Extracted vm name from path: {vm_name}");
                        Some(entry.path().to_path_buf())
                    } else {
                        None
                    }
                })
                .collect::<Vec<PathBuf>>();

            manager_inventory_files
                .par_iter()
                .flat_map(|file_path| match get_node_manager_inventory(file_path) {
                    Ok(inventory) => vec![Ok(inventory)],
                    Err(err) => vec![Err(err)],
                })
                .collect::<Result<Vec<NodeManagerInventory>>>()?
        };

        // todo: filter out nodes that are running currently. Nodes can be restarted, which can lead to us collecting
        // RPCs to nodes that do not exists.
        let safenode_rpc_endpoints: BTreeMap<String, SocketAddr> = node_manager_inventories
            .iter()
            .flat_map(|inv| {
                inv.nodes
                    .iter()
                    .flat_map(|node| node.peer_id.clone().map(|id| (id, node.rpc_socket_addr)))
            })
            .collect();

        let safenodemand_endpoints: BTreeMap<String, SocketAddr> = node_manager_inventories
            .iter()
            .flat_map(|inv| {
                inv.nodes.iter().flat_map(|node| {
                    if let (Some(peer_id), Some(Some(daemon_socket_addr))) = (
                        node.peer_id.clone(),
                        inv.daemon.clone().map(|daemon| daemon.endpoint),
                    ) {
                        Some((peer_id, daemon_socket_addr))
                    } else {
                        None
                    }
                })
            })
            .collect();

        // The scripts are relative to the `resources` directory, so we need to change the current
        // working directory back to that location first.
        std::env::set_current_dir(self.working_directory_path.clone())?;
        println!("Retrieving sample peers. This can take a minute.");
        // Todo: RPC into nodes to fetch the multiaddr.
        let peers = remaining_nodes_inventory
            .par_iter()
            .filter_map(|(vm_name, ip_address)| {
                let ip_address = *ip_address;
                match self.ssh_client.run_script(
                    ip_address,
                    "safe",
                    PathBuf::from("scripts").join("get_peer_multiaddr.sh"),
                    true,
                ) {
                    Ok(output) => Some(output),
                    Err(err) => {
                        println!("Failed to SSH into {vm_name:?}: {ip_address} with err: {err:?}");
                        None
                    }
                }
            })
            .flatten()
            .collect::<Vec<_>>();

        // The VM list includes the genesis node and the build machine, hence the subtraction of 2
        // from the total VM count. After that, add one node for genesis, since this machine only
        // runs a single node.
        let node_count = {
            let vms_to_ignore = if build_inventory.is_empty() { 1 } else { 2 };
            let mut node_count =
                (vm_list.len() - vms_to_ignore) as u16 * node_instance_count.unwrap_or(0);
            node_count += 1;
            node_count
        };

        let inventory = DeploymentInventory {
            name: name.to_string(),
            node_count,
            sn_codebase_type,
            vm_list,
            rpc_endpoints: safenode_rpc_endpoints,
            safenodemand_endpoints,
            ssh_user: self.cloud_provider.get_ssh_user(),
            genesis_multiaddr,
            peers,
            faucet_address: format!("{}:8000", genesis_ip),
            uploaded_files: Vec::new(),
        };
        inventory.print_report()?;
        inventory.save(&inventory_path)?;

        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        // The ansible runner will have its working directory set to this location. We need the
        // same here to test the inventory paths, which are relative to the `ansible` directory.
        let ansible_dir_path = self.working_directory_path.join("ansible");
        std::env::set_current_dir(ansible_dir_path.clone())?;

        let genesis_inventory_path = PathBuf::from("inventory")
            .join(format!(".{}_genesis_inventory_digital_ocean.yml", name));
        let remaining_nodes_inventory_path =
            PathBuf::from("inventory").join(format!(".{}_node_inventory_digital_ocean.yml", name));
        if !genesis_inventory_path.exists() || !remaining_nodes_inventory_path.exists() {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        self.ansible_runner.run_playbook(
            PathBuf::from("start_nodes.yml"),
            genesis_inventory_path,
            self.cloud_provider.get_ssh_user(),
            None,
        )?;
        self.ansible_runner.run_playbook(
            PathBuf::from("start_nodes.yml"),
            remaining_nodes_inventory_path,
            self.cloud_provider.get_ssh_user(),
            None,
        )?;

        Ok(())
    }

    pub async fn upgrade(&self, options: UpgradeOptions) -> Result<()> {
        // Set the `forks` config value for Ansible. This environment variable will override
        // whatever is in the ansible.cfg file.
        std::env::set_var("ANSIBLE_FORKS", options.forks.to_string());

        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&options.name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(options.name.to_string()));
        }

        // The ansible runner will have its working directory set to this location. We need the
        // same here to test the inventory paths, which are relative to the `ansible` directory.
        let ansible_dir_path = self.working_directory_path.join("ansible");
        std::env::set_current_dir(ansible_dir_path.clone())?;

        let genesis_inventory_path = PathBuf::from("inventory").join(format!(
            ".{}_genesis_inventory_digital_ocean.yml",
            options.name
        ));
        let remaining_nodes_inventory_path = PathBuf::from("inventory").join(format!(
            ".{}_node_inventory_digital_ocean.yml",
            options.name
        ));
        if !genesis_inventory_path.exists() || !remaining_nodes_inventory_path.exists() {
            return Err(Error::EnvironmentDoesNotExist(options.name.to_string()));
        }

        self.ansible_runner.run_playbook(
            PathBuf::from("upgrade_nodes.yml"),
            remaining_nodes_inventory_path,
            self.cloud_provider.get_ssh_user(),
            Some(options.get_ansible_vars()),
        )?;
        self.ansible_runner.run_playbook(
            PathBuf::from("upgrade_nodes.yml"),
            genesis_inventory_path.clone(),
            self.cloud_provider.get_ssh_user(),
            Some(options.get_ansible_vars()),
        )?;
        self.ansible_runner.run_playbook(
            PathBuf::from("upgrade_faucet.yml"),
            genesis_inventory_path,
            self.cloud_provider.get_ssh_user(),
            Some(options.get_ansible_vars()),
        )?;

        Ok(())
    }

    pub async fn upgrade_node_manager(&self, name: &str, version: Version) -> Result<()> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        // The ansible runner will have its working directory set to this location. We need the
        // same here to test the inventory paths, which are relative to the `ansible` directory.
        let ansible_dir_path = self.working_directory_path.join("ansible");
        std::env::set_current_dir(ansible_dir_path.clone())?;

        let genesis_inventory_path = PathBuf::from("inventory")
            .join(format!(".{}_genesis_inventory_digital_ocean.yml", name));
        let remaining_nodes_inventory_path =
            PathBuf::from("inventory").join(format!(".{}_node_inventory_digital_ocean.yml", name));
        if !genesis_inventory_path.exists() || !remaining_nodes_inventory_path.exists() {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("version", &version.to_string());
        self.ansible_runner.run_playbook(
            PathBuf::from("upgrade_node_manager.yml"),
            genesis_inventory_path,
            self.cloud_provider.get_ssh_user(),
            Some(extra_vars.build()),
        )?;
        self.ansible_runner.run_playbook(
            PathBuf::from("upgrade_node_manager.yml"),
            remaining_nodes_inventory_path,
            self.cloud_provider.get_ssh_user(),
            Some(extra_vars.build()),
        )?;

        Ok(())
    }

    pub async fn clean(&self, name: &str) -> Result<()> {
        do_clean(
            name,
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
    match inventory.sn_codebase_type {
        SnCodebaseType::Main { .. } => {}
        SnCodebaseType::Branch {
            repo_owner, branch, ..
        } => {
            message.push_str("*Branch Details*\n");
            message.push_str(&format!("Repo owner: {}\n", repo_owner));
            message.push_str(&format!("Branch: {}\n", branch));
        }
        SnCodebaseType::Versioned {
            faucet_version,
            safe_version,
            safenode_version,
            safenode_manager_version,
        } => {
            message.push_str("*Version Details*\n");
            message.push_str(&format!("faucet version: {}\n", faucet_version));
            message.push_str(&format!("safe version: {}\n", safe_version));
            message.push_str(&format!("safenode version: {}\n", safenode_version));
            message.push_str(&format!(
                "safenode-manager version: {}\n",
                safenode_manager_version
            ));
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

#[derive(Deserialize)]
struct NodeManagerInventory {
    daemon: Option<Daemon>,
    nodes: Vec<Node>,
}
#[derive(Deserialize, Clone)]
struct Daemon {
    endpoint: Option<SocketAddr>,
}

#[derive(Deserialize)]
struct Node {
    rpc_socket_addr: SocketAddr,
    peer_id: Option<String>,
}

fn get_node_manager_inventory(inventory_file_path: &PathBuf) -> Result<NodeManagerInventory> {
    let file = File::open(inventory_file_path)?;
    let reader = BufReader::new(file);
    let inventory = serde_json::from_reader(reader)?;
    Ok(inventory)
}
