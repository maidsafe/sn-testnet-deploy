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
    BinaryOption, CloudProvider, TestnetDeployer,
};
use color_eyre::{eyre::eyre, Result};
use log::{debug, trace};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use sn_service_management::NodeRegistry;
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::From,
    fs::File,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use walkdir::WalkDir;

const DEFAULT_CONTACTS_COUNT: usize = 25;
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

impl From<TestnetDeployer> for DeploymentInventoryService {
    fn from(item: TestnetDeployer) -> Self {
        let provider = match item.cloud_provider {
            CloudProvider::Aws => "aws",
            CloudProvider::DigitalOcean => "digital_ocean",
        };
        DeploymentInventoryService {
            ansible_runner: item.ansible_provisioner.ansible_runner.clone(),
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
    /// Generate or retrieve the inventory for the deployment.
    ///
    /// If we're creating a new environment and there is no inventory yet, a empty inventory will
    /// be returned; otherwise the inventory will represent what is deployed currently.
    ///
    /// The `force` flag is used when the `deploy` command runs, to make sure that a new inventory
    /// is generated, because it's possible that an old one with the same environment name has been
    /// cached.
    ///
    /// The binary option will only be present on the first generation of the inventory, when the
    /// testnet is initially deployed. On any subsequent runs, we don't have access to the initial
    /// launch arguments. This means any branch specification is lost. In this case, we'll just
    /// retrieve the version numbers from the genesis node in the node registry. Most of the time
    /// it is the version numbers that will be of interest.
    pub async fn generate_or_retrieve_inventory(
        &self,
        name: &str,
        force: bool,
        binary_option: Option<BinaryOption>,
    ) -> Result<DeploymentInventory> {
        println!("======================================");
        println!("  Generating or Retrieving Inventory  ");
        println!("======================================");
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

        // The following operation is idempotent.
        generate_environment_inventory(
            name,
            &self.inventory_file_path,
            &self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
        )
        .await?;

        let mut misc_vm_list = Vec::new();
        let genesis_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, false)
            .await?;
        if genesis_inventory.is_empty() {
            println!("Genesis node does not exist: we are treating this as a new deployment");
            return Ok(DeploymentInventory::empty(
                name,
                binary_option
                    .ok_or_else(|| eyre!("For a new deployment the binary option must be set"))?,
            ));
        }
        misc_vm_list.push((genesis_inventory[0].0.clone(), genesis_inventory[0].1));

        let auditor_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Auditor, true)
            .await?;
        let auditor_ip = auditor_inventory[0].1;
        misc_vm_list.push((auditor_inventory[0].0.clone(), auditor_ip));

        let build_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, false)
            .await?;
        if !build_inventory.is_empty() {
            misc_vm_list.push((build_inventory[0].0.clone(), build_inventory[0].1));
        }

        let mut node_vm_list = Vec::new();
        let nodes_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Nodes, false)
            .await?;
        for entry in nodes_inventory.iter() {
            node_vm_list.push((entry.0.clone(), entry.1));
        }

        let mut bootstrap_vm_list = Vec::new();
        let bootstrap_nodes_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::BootstrapNodes, false)
            .await?;
        for entry in bootstrap_nodes_inventory.iter() {
            bootstrap_vm_list.push((entry.0.clone(), entry.1));
        }

        let mut uploader_vm_list = Vec::new();
        let uploader_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Uploaders, false)
            .await?;
        for entry in uploader_inventory.iter() {
            uploader_vm_list.push((entry.0.clone(), entry.1));
        }

        println!("Retrieving node registries from all VMs...");
        let mut node_registries = Vec::new();
        let bootstrap_node_registries =
            self.get_node_registries(AnsibleInventoryType::BootstrapNodes)?;

        let generic_node_registries = self.get_node_registries(AnsibleInventoryType::Nodes)?;
        node_registries.extend(bootstrap_node_registries.clone());
        node_registries.extend(generic_node_registries.clone());
        node_registries.extend(self.get_node_registries(AnsibleInventoryType::Genesis)?);
        node_registries.extend(self.get_node_registries(AnsibleInventoryType::Auditor)?);

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

        let bootstrap_peers = bootstrap_node_registries
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
        let node_peers = generic_node_registries
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

        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(&self.ansible_runner, &self.ssh_client).await?;
        let inventory = DeploymentInventory {
            auditor_address: format!("{auditor_ip}:4242"),
            binary_option,
            bootstrap_node_vms: bootstrap_vm_list,
            bootstrap_peers,
            faucet_address: format!("{genesis_ip}:8000"),
            genesis_multiaddr,
            name: name.to_string(),
            misc_vms: misc_vm_list,
            node_vms: node_vm_list,
            node_peers,
            rpc_endpoints: safenode_rpc_endpoints,
            safenodemand_endpoints,
            ssh_user: self.cloud_provider.get_ssh_user(),
            uploaded_files: Vec::new(),
            uploader_vms: uploader_vm_list,
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

        let mut file = std::fs::File::create(&temp_file_path)?;
        let mut rng = rand::thread_rng();

        let bootstrap_peers_len = inventory.bootstrap_peers.len();
        for peer in inventory
            .bootstrap_peers
            .iter()
            .filter(|&peer| peer != STOPPED_PEER_ID)
            .cloned()
            .choose_multiple(&mut rng, DEFAULT_CONTACTS_COUNT)
        {
            writeln!(file, "{peer}",)?;
        }

        if DEFAULT_CONTACTS_COUNT > bootstrap_peers_len {
            for peer in inventory
                .node_peers
                .iter()
                .filter(|&peer| peer != STOPPED_PEER_ID)
                .cloned()
                .choose_multiple(&mut rng, DEFAULT_CONTACTS_COUNT - bootstrap_peers_len)
            {
                writeln!(file, "{peer}",)?;
            }
        }

        self.s3_repository
            .upload_file(TESTNET_BUCKET_NAME, &temp_file_path, true)
            .await?;

        Ok(())
    }

    fn get_node_registries(
        &self,
        inventory_type: AnsibleInventoryType,
    ) -> Result<Vec<NodeRegistry>> {
        debug!("Fetching node manager inventory");
        let temp_dir_path = tempfile::tempdir()?.into_path();
        let temp_dir_json = serde_json::to_string(&temp_dir_path)?;

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::NodeManagerInventory,
            inventory_type,
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

        Ok(node_registry_paths
            .iter()
            .map(|file_path| NodeRegistry::load(file_path).unwrap())
            .collect::<Vec<NodeRegistry>>())
    }
}

pub type VirtualMachine = (String, IpAddr);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub auditor_address: String,
    pub binary_option: BinaryOption,
    pub bootstrap_node_vms: Vec<VirtualMachine>,
    pub bootstrap_peers: Vec<String>,
    pub faucet_address: String,
    pub genesis_multiaddr: String,
    pub misc_vms: Vec<VirtualMachine>,
    pub name: String,
    pub node_vms: Vec<VirtualMachine>,
    pub node_peers: Vec<String>,
    pub rpc_endpoints: BTreeMap<String, SocketAddr>,
    pub safenodemand_endpoints: Vec<SocketAddr>,
    pub ssh_user: String,
    pub uploaded_files: Vec<(String, String)>,
    pub uploader_vms: Vec<VirtualMachine>,
}

impl DeploymentInventory {
    /// Create an inventory for a new deployment which is initially empty, other than the name and
    /// binary option, which will have been selected.
    pub fn empty(name: &str, binary_option: BinaryOption) -> DeploymentInventory {
        Self {
            binary_option,
            name: name.to_string(),
            auditor_address: String::new(),
            bootstrap_node_vms: Vec::new(),
            bootstrap_peers: Vec::new(),
            genesis_multiaddr: String::new(),
            faucet_address: String::new(),
            misc_vms: Vec::new(),
            node_vms: Vec::new(),
            node_peers: Vec::new(),
            rpc_endpoints: BTreeMap::new(),
            safenodemand_endpoints: Vec::new(),
            ssh_user: "root".to_string(),
            uploaded_files: Vec::new(),
            uploader_vms: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bootstrap_node_vms.is_empty() && self.node_vms.is_empty()
    }

    pub fn vm_list(&self) -> Vec<VirtualMachine> {
        let mut list = Vec::new();
        list.extend(self.bootstrap_node_vms.clone());
        list.extend(self.misc_vms.clone());
        list.extend(self.node_vms.clone());
        list
    }

    pub fn peers(&self) -> BTreeSet<String> {
        let mut list = BTreeSet::new();
        list.extend(self.bootstrap_peers.clone());
        list.extend(self.node_peers.clone());
        list
    }

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

    pub fn get_random_peer(&self) -> Option<String> {
        let mut rng = rand::thread_rng();
        self.peers().into_iter().choose(&mut rng)
    }

    pub fn bootstrap_node_count(&self) -> usize {
        self.bootstrap_peers.len() / self.bootstrap_node_vms.len()
    }

    pub fn node_count(&self) -> usize {
        self.node_peers.len() / self.node_vms.len()
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

        println!("=============");
        println!("Bootstrap VMs");
        println!("=============");
        for vm in self.bootstrap_node_vms.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("Nodes per VM: {}", self.bootstrap_node_count());
        println!("SSH user: {}", self.ssh_user);
        println!();

        println!("========");
        println!("Node VMs");
        println!("========");
        for vm in self.node_vms.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("Nodes per VM: {}", self.node_count());
        println!("SSH user: {}", self.ssh_user);
        println!();

        println!("============");
        println!("Uploader VMs");
        println!("============");
        for vm in self.uploader_vms.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("SSH user: {}", self.ssh_user);
        println!();

        println!("=========");
        println!("Other VMs");
        println!("=========");
        for vm in self.misc_vms.iter() {
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
        self.bootstrap_node_vms
            .iter()
            .chain(self.node_vms.iter())
            .map(|vm| vm.1.to_string())
            .for_each(|ip| {
                if let Some(peer) = self.peers().iter().find(|p| p.contains(&ip)) {
                    println!("{peer}");
                }
            });
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
        println!("Faucet address: {:?}", self.faucet_address);
        println!("Check the faucet:");
        println!(
            "safe --peer {} wallet get-faucet {:?}",
            self.genesis_multiaddr, self.faucet_address
        );
        println!();

        println!("===============");
        println!("Auditor Details");
        println!("===============");
        println!("Auditor address: {:?}", self.auditor_address);
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
