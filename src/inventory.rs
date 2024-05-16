// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        generate_environment_inventory, AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner,
    },
    get_genesis_multiaddr,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
    BinaryOption, CloudProvider, TestnetDeploy,
};
use color_eyre::{eyre::eyre, Result};
use log::{debug, trace};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sn_service_management::NodeRegistry;
use std::{
    collections::BTreeMap,
    convert::From,
    fs::File,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;

const DEFAULT_CONTACTS_COUNT: usize = 50;
const STOPPED_PEER_ID: &str = "-";
const TESTNET_BUCKET_NAME: &str = "sn-testnet";

pub struct DeploymentInventoryService {
    pub ansible_runner: AnsibleRunner,
    pub cloud_provider: CloudProvider,
    pub inventory_file_path: PathBuf,
    pub s3_repository: S3Repository,
    pub ssh_client: SshClient,
    pub terraform_runner: TerraformRunner,
    pub working_directory_path: PathBuf,
}

impl From<TestnetDeploy> for DeploymentInventoryService {
    fn from(item: TestnetDeploy) -> Self {
        let provider = match item.cloud_provider {
            CloudProvider::Aws => "aws",
            CloudProvider::DigitalOcean => "digital_ocean",
        };
        DeploymentInventoryService {
            ansible_runner: item.ansible_runner.clone(),
            cloud_provider: item.cloud_provider,
            inventory_file_path: item
                .working_directory_path
                .join("ansible")
                .join("inventory")
                .join(format!("dev_inventory_{}.yml", provider)),
            s3_repository: item.s3_repository.clone(),
            ssh_client: item.ssh_client.clone(),
            terraform_runner: item.terraform_runner.clone(),
            working_directory_path: item.working_directory_path.clone(),
        }
    }
}

impl DeploymentInventoryService {
    /// Generate the inventory for the deployment.
    ///
    /// It will be cached for the purposes of quick display later and also for use with the test
    /// data upload.
    ///
    /// The `force` flag is used when the `deploy` command runs, to make sure that a new inventory
    /// is generated, because it's possible that an old one with the same environment name has been
    /// cached.
    ///
    /// The binary option will only be present on the first generation of the inventory, when the
    /// testnet is initially deployed. On any subsequent runs to generate the inventory, we don't
    /// have access to the initial arguments that the deployment was launched with. This will mean
    /// that any branch specification is lost. In this case, we will just retrieve the version
    /// numbers from the genesis node in the node registry. Most of the time it is the version
    /// numbers that will be of interest.
    pub async fn generate_inventory(
        &self,
        name: &str,
        force: bool,
        binary_option: Option<BinaryOption>,
    ) -> Result<DeploymentInventory> {
        println!("=============================");
        println!("     Generating Inventory    ");
        println!("=============================");
        let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
        if inventory_path.exists() && !force {
            let inventory = DeploymentInventory::read(&inventory_path)?;
            return Ok(inventory);
        }

        // This allows for the inventory to be generated without a Terraform workspace to be
        // initialised, which is the case in the workflow for printing an inventory.
        if !force {
            let environments = self.terraform_runner.workspace_list()?;
            if !environments.contains(&name.to_string()) {
                return Err(eyre!("The '{}' environment does not exist", name));
            }
        }

        if force {
            generate_environment_inventory(
                name,
                &self.inventory_file_path,
                &self
                    .working_directory_path
                    .join("ansible")
                    .join("inventory"),
            )
            .await?
        }

        let genesis_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, false)
            .await?;
        let build_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, false)
            .await?;
        let remaining_nodes_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Nodes, false)
            .await?;
        let auditor_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Auditor, true)
            .await?;
        let auditor_ip = auditor_inventory[0].1;

        // It also seems to be possible for a workspace and inventory files to still exist, but
        // there to be no inventory items returned. Perhaps someone deleted the VMs manually. We
        // only need to test the genesis node in this case, since that must always exist.
        if genesis_inventory.is_empty() {
            return Err(eyre!("The '{}' environment does not exist", name));
        }

        let mut vm_list = Vec::new();
        vm_list.push((genesis_inventory[0].0.clone(), genesis_inventory[0].1));
        if !build_inventory.is_empty() {
            vm_list.push((build_inventory[0].0.clone(), build_inventory[0].1));
        }
        for entry in remaining_nodes_inventory.iter() {
            vm_list.push((entry.0.clone(), entry.1));
        }

        println!("Retrieving node registries from all VMs...");
        let node_registries = {
            debug!("Fetching node manager inventory");
            let temp_dir_path = tempfile::tempdir()?.into_path();
            let temp_dir_json = serde_json::to_string(&temp_dir_path)?;

            self.ansible_runner.run_playbook(
                AnsiblePlaybook::NodeManagerInventory,
                AnsibleInventoryType::Nodes,
                Some(format!("{{ \"dest\": {temp_dir_json} }}")),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::NodeManagerInventory,
                AnsibleInventoryType::Genesis,
                Some(format!("{{ \"dest\": {temp_dir_json} }}")),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::NodeManagerInventory,
                AnsibleInventoryType::Auditor,
                Some(format!("{{ \"dest\": {temp_dir_json} }}")),
            )?;

            let node_registry_paths = WalkDir::new(temp_dir_path)
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

            node_registry_paths
                .iter()
                .map(|file_path| NodeRegistry::load(file_path).unwrap())
                .collect::<Vec<NodeRegistry>>()
        };

        let safenode_rpc_endpoints: BTreeMap<String, SocketAddr> = node_registries
            .iter()
            .flat_map(|inv| {
                inv.nodes.iter().map(|node| {
                    let id = if let Some(peer_id) = node.peer_id {
                        peer_id.to_string().clone()
                    } else {
                        "-".to_string()
                    };
                    (id, node.rpc_socket_addr)
                })
            })
            .collect();

        let safenodemand_endpoints: Vec<SocketAddr> = node_registries
            .iter()
            .filter_map(|reg| reg.daemon.clone())
            .filter_map(|daemon| daemon.endpoint)
            .collect();

        let peers = node_registries
            .iter()
            .flat_map(|reg| {
                reg.nodes.iter().map(|node| {
                    if let Some(listen_addresses) = &node.listen_addr {
                        // It seems to be the case that the listening address with the public IP is
                        // always in the second position. If this ever changes, we could do some
                        // filtering to find the address that does not start with "127." or "10.".
                        listen_addresses[1].to_string()
                    } else {
                        "-".to_string()
                    }
                })
            })
            .collect::<Vec<String>>();

        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(&self.ansible_runner, &self.ssh_client).await?;

        let binary_option = if let Some(binary_option) = binary_option {
            binary_option
        } else {
            let genesis_node_registry = node_registries
                .iter()
                .find(|reg| reg.faucet.is_some())
                .ok_or_else(|| eyre!("Unable to retrieve genesis node registry"))?;
            let faucet_version = &genesis_node_registry.faucet.as_ref().unwrap().version;
            let safenode_version = genesis_node_registry
                .nodes
                .first()
                .ok_or_else(|| eyre!("Unable to obtain the genesis node"))?
                .version
                .clone();
            let safenode_manager_version = genesis_node_registry
                .daemon
                .as_ref()
                .ok_or_else(|| eyre!("Unable to obtain the daemon"))?
                .version
                .clone();
            let auditor_node_registry = node_registries
                .iter()
                .find(|reg| reg.auditor.is_some())
                .ok_or_else(|| eyre!("Unable to retrieve auditor node registry"))?;
            let sn_auditor_version = &auditor_node_registry.auditor.as_ref().unwrap().version;

            BinaryOption::Versioned {
                faucet_version: faucet_version.parse()?,
                safenode_version: safenode_version.parse()?,
                safenode_manager_version: safenode_manager_version.parse()?,
                sn_auditor_version: sn_auditor_version.parse()?,
            }
        };

        let inventory = DeploymentInventory {
            auditor_address: format!("{auditor_ip}:4242"),
            binary_option,
            faucet_address: format!("{genesis_ip}:8000"),
            genesis_multiaddr,
            name: name.to_string(),
            peers,
            rpc_endpoints: safenode_rpc_endpoints,
            safenodemand_endpoints,
            ssh_user: self.cloud_provider.get_ssh_user(),
            vm_list,
            uploaded_files: Vec::new(),
        };
        Ok(inventory)
    }

    pub async fn upload_network_contacts(
        &self,
        inventory: &DeploymentInventory,
        contacts_file_name: Option<String>,
    ) -> Result<()> {
        let temp_dir_path = tempfile::tempdir()?.into_path();
        let temp_file_path = if let Some(file_name) = contacts_file_name {
            temp_dir_path.join(file_name)
        } else {
            temp_dir_path.join(inventory.name.clone())
        };

        let count = if inventory.peers.len() < DEFAULT_CONTACTS_COUNT {
            inventory.peers.len()
        } else {
            DEFAULT_CONTACTS_COUNT
        };

        let mut file = tokio::fs::File::create(&temp_file_path).await?;
        for _ in 0..count {
            let peer = inventory.get_random_peer();
            if peer != STOPPED_PEER_ID {
                file.write_all(format!("{}\n", peer).as_bytes()).await?;
            }
        }

        self.s3_repository
            .upload_file(TESTNET_BUCKET_NAME, &temp_file_path, true)
            .await?;

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub auditor_address: String,
    pub binary_option: BinaryOption,
    pub faucet_address: String,
    pub genesis_multiaddr: String,
    pub name: String,
    pub peers: Vec<String>,
    pub rpc_endpoints: BTreeMap<String, SocketAddr>,
    pub safenodemand_endpoints: Vec<SocketAddr>,
    pub ssh_user: String,
    pub uploaded_files: Vec<(String, String)>,
    pub vm_list: Vec<(String, IpAddr)>,
}

impl DeploymentInventory {
    pub fn save(&self) -> Result<()> {
        let path = get_data_directory()?.join(format!("{}-inventory.json", self.name));
        let serialized_data = serde_json::to_string_pretty(self)?;
        let mut file = File::create(path)?;
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

        println!("Environment Name: {}", self.name);
        println!();
        match &self.binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                println!("==============");
                println!("Branch Details");
                println!("==============");
                println!("Repo owner: {repo_owner}");
                println!("Branch name: {branch}");
                println!();
            }
            BinaryOption::Versioned {
                faucet_version,
                safenode_version,
                safenode_manager_version,
                sn_auditor_version,
            } => {
                println!("===============");
                println!("Version Details");
                println!("===============");
                println!("faucet version: {faucet_version}");
                println!("safenode version: {safenode_version}");
                println!("safenode-manager version: {safenode_manager_version}");
                println!("sn_auditor version: {sn_auditor_version}");

                println!();
            }
        }

        println!("=======");
        println!("VM List");
        println!("=======");
        for vm in self.vm_list.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("SSH user: {}", self.ssh_user);
        println!();

        // Take the first peer from each VM. If you just take, say, the first 10 on the peer list,
        // they will all be from the same machine. They will be unique peers, but they won't look
        // very random.
        println!("============");
        println!("Sample Peers");
        println!("============");
        for ip in self.vm_list.iter().map(|vm| vm.1.to_string()) {
            if let Some(peer) = self.peers.iter().find(|p| p.contains(&ip)) {
                println!("{peer}");
            }
        }
        println!("Genesis: {}", self.genesis_multiaddr);
        let inventory_file_path =
            get_data_directory()?.join(format!("{}-inventory.json", self.name));
        println!(
            "The entire peer list can be found at {}",
            inventory_file_path.to_string_lossy()
        );
        println!();

        println!("==============");
        println!("Faucet Details");
        println!("==============");
        println!("Faucet address: {}", self.faucet_address);
        println!("Check the faucet:");
        println!(
            "safe --peer {} wallet get-faucet {}",
            self.genesis_multiaddr, self.faucet_address
        );
        println!();

        println!("===============");
        println!("Auditor Details");
        println!("===============");
        println!("Auditor address: {}", self.auditor_address);
        println!();

        if !self.uploaded_files.is_empty() {
            println!("Uploaded files:");
            for file in self.uploaded_files.iter() {
                println!("{}: {}", file.0, file.1);
            }
        }
        Ok(())
    }
}

pub fn get_data_directory() -> Result<PathBuf> {
    let path = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Could not retrieve data directory"))?
        .join("safe")
        .join("testnet-deploy");
    if !path.exists() {
        std::fs::create_dir_all(path.clone())?;
    }
    Ok(path)
}
