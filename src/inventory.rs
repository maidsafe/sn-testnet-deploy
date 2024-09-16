// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        generate_environment_inventory, provisioning::AnsibleProvisioner, AnsibleInventoryType,
        AnsibleRunner,
    },
    get_environment_details, get_genesis_multiaddr,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
    BinaryOption, CloudProvider, DeploymentType, EnvironmentDetails, Error, TestnetDeployer,
};
use color_eyre::{eyre::eyre, Result};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use sn_service_management::{NodeRegistry, ServiceStatus};
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::From,
    fs::File,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

const DEFAULT_CONTACTS_COUNT: usize = 25;
const STOPPED_PEER_ID: &str = "-";
const TESTNET_BUCKET_NAME: &str = "sn-testnet";

pub struct DeploymentInventoryService {
    pub ansible_runner: AnsibleRunner,
    // It may seem strange to have both the runner and the provisioner, because the provisioner is
    // a wrapper around the runner, but it's for the purpose of sharing some code. More things
    // could go into the provisioner later, which may eliminate the need to have the runner.
    pub ansible_provisioner: AnsibleProvisioner,
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
            ansible_provisioner: item.ansible_provisioner.clone(),
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

        // For new environments, whether it's a new or bootstrap deploy, the inventory files need
        // to be generated for the Ansible run to work correctly.
        //
        // It is an idempotent operation; the files won't be generated if they already exist.
        generate_environment_inventory(
            name,
            &self.inventory_file_path,
            &self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
        )
        .await?;

        let environment_details = match get_environment_details(name, &self.s3_repository).await {
            Ok(details) => details,
            Err(Error::EnvironmentDetailsNotFound(_)) => {
                println!("Environment details not found: treating this as a new deployment");
                return Ok(DeploymentInventory::empty(
                    name,
                    binary_option.ok_or_else(|| {
                        eyre!("For a new deployment the binary option must be set")
                    })?,
                ));
            }
            Err(e) => return Err(e.into()),
        };

        let mut misc_vm_list = Vec::new();
        let genesis_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, false)
            .await?;

        if !genesis_inventory.is_empty() {
            misc_vm_list.push((genesis_inventory[0].0.clone(), genesis_inventory[0].1));
        }

        let mut auditor_vm_list = Vec::new();
        let auditor_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Auditor, true)
            .await?;
        for entry in auditor_inventory.iter() {
            auditor_vm_list.push((entry.0.clone(), entry.1));
        }

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
        let mut failed_node_registry_vms = Vec::new();

        let bootstrap_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::BootstrapNodes)?;

        let generic_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Nodes)?;

        let genesis_node_registry = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Genesis)?;

        let auditor_node_registry = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Auditor)?;

        node_registries.extend(bootstrap_node_registries.retrieved_registries.clone());
        node_registries.extend(generic_node_registries.retrieved_registries.clone());
        node_registries.extend(genesis_node_registry.retrieved_registries);
        node_registries.extend(auditor_node_registry.retrieved_registries);

        failed_node_registry_vms.extend(bootstrap_node_registries.failed_vms);
        failed_node_registry_vms.extend(generic_node_registries.failed_vms);
        failed_node_registry_vms.extend(genesis_node_registry.failed_vms);
        failed_node_registry_vms.extend(auditor_node_registry.failed_vms);

        let safenode_rpc_endpoints: BTreeMap<String, SocketAddr> = node_registries
            .iter()
            .flat_map(|(_, inv)| {
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
            .filter_map(|(_, reg)| reg.daemon.clone())
            .filter_map(|daemon| daemon.endpoint)
            .collect();

        let bootstrap_peers = bootstrap_node_registries
            .retrieved_registries
            .iter()
            .flat_map(|(_, reg)| {
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
            .retrieved_registries
            .iter()
            .flat_map(|(_, reg)| {
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
            let (faucet_version, safenode_version, sn_auditor_version) =
                match environment_details.deployment_type {
                    DeploymentType::New => {
                        let (_, genesis_node_registry) = node_registries
                            .iter()
                            .find(|(_, reg)| reg.faucet.is_some())
                            .ok_or_else(|| eyre!("Unable to retrieve genesis node registry"))?;
                        let faucet_version = Some(
                            genesis_node_registry
                                .faucet
                                .as_ref()
                                .unwrap()
                                .version
                                .parse()?,
                        );
                        let safenode_version = genesis_node_registry
                            .nodes
                            .first()
                            .ok_or_else(|| eyre!("Unable to obtain the genesis node"))?
                            .version
                            .parse()?;
                        let (_, auditor_node_registry) = node_registries
                            .iter()
                            .find(|(_, reg)| reg.auditor.is_some())
                            .ok_or_else(|| eyre!("Unable to retrieve auditor node registry"))?;
                        let sn_auditor_version = Some(
                            auditor_node_registry
                                .auditor
                                .as_ref()
                                .unwrap()
                                .version
                                .parse()?,
                        );
                        (faucet_version, safenode_version, sn_auditor_version)
                    }
                    DeploymentType::Bootstrap => {
                        let safenode_version = generic_node_registries
                            .retrieved_registries
                            .first()
                            .and_then(|(_, reg)| reg.nodes.first())
                            .ok_or_else(|| eyre!("Unable to obtain a node"))?
                            .version
                            .parse()?;
                        (None, safenode_version, None)
                    }
                };

            let safenode_manager_version = node_registries
                .iter()
                .find_map(|(_, reg)| reg.daemon.as_ref())
                .ok_or_else(|| eyre!("Unable to obtain the daemon"))?
                .version
                .parse()?;
            BinaryOption::Versioned {
                safe_version: Some("0.0.1".parse()?), // todo: store safe version in the safenodeman registry?
                faucet_version,
                safenode_version,
                safenode_manager_version,
                sn_auditor_version,
            }
        };

        let (genesis_multiaddr, genesis_ip) =
            if environment_details.deployment_type == DeploymentType::New {
                let (multiaddr, ip) =
                    get_genesis_multiaddr(&self.ansible_runner, &self.ssh_client).await?;
                (Some(multiaddr), Some(ip))
            } else {
                (None, None)
            };
        let inventory = DeploymentInventory {
            auditor_vms: auditor_vm_list,
            binary_option,
            bootstrap_node_vms: bootstrap_vm_list,
            bootstrap_peers,
            environment_details,
            failed_node_registry_vms,
            faucet_address: genesis_ip.map(|ip| format!("{ip}:8000")),
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
}

pub type VirtualMachine = (String, IpAddr);

#[derive(Clone)]
pub struct DeploymentNodeRegistries {
    pub inventory_type: AnsibleInventoryType,
    pub retrieved_registries: Vec<(String, NodeRegistry)>,
    pub failed_vms: Vec<String>,
}

impl DeploymentNodeRegistries {
    pub fn print(&self) {
        Self::print_banner(&self.inventory_type.to_string());
        for (vm_name, registry) in self.retrieved_registries.iter() {
            println!("{vm_name}:");
            for node in registry.nodes.iter() {
                println!(
                    "  {}: {} {}",
                    node.service_name,
                    node.version,
                    Self::format_status(&node.status)
                );
            }
        }
        if !self.failed_vms.is_empty() {
            println!(
                "Failed to retrieve node registries for {}:",
                self.inventory_type
            );
            for vm_name in self.failed_vms.iter() {
                println!("- {}", vm_name);
            }
        }
    }

    fn format_status(status: &ServiceStatus) -> String {
        match status {
            ServiceStatus::Running => "RUNNING".to_string(),
            ServiceStatus::Stopped => "STOPPED".to_string(),
            ServiceStatus::Added => "ADDED".to_string(),
            ServiceStatus::Removed => "REMOVED".to_string(),
        }
    }

    fn print_banner(text: &str) {
        let padding = 2;
        let text_width = text.len() + padding * 2;
        let border_chars = 2;
        let total_width = text_width + border_chars;
        let top_bottom = "═".repeat(total_width);

        println!("╔{}╗", top_bottom);
        println!("║ {:^width$} ║", text, width = text_width);
        println!("╚{}╝", top_bottom);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub auditor_vms: Vec<VirtualMachine>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_vms: Vec<VirtualMachine>,
    pub bootstrap_peers: Vec<String>,
    pub environment_details: EnvironmentDetails,
    pub failed_node_registry_vms: Vec<String>,
    pub faucet_address: Option<String>,
    pub genesis_multiaddr: Option<String>,
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
            auditor_vms: Vec::new(),
            bootstrap_node_vms: Vec::new(),
            bootstrap_peers: Vec::new(),
            environment_details: EnvironmentDetails::default(),
            genesis_multiaddr: None,
            failed_node_registry_vms: Vec::new(),
            faucet_address: None,
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
        if self.bootstrap_node_vms.is_empty() {
            0
        } else {
            self.bootstrap_peers.len() / self.bootstrap_node_vms.len()
        }
    }

    pub fn node_count(&self) -> usize {
        if self.node_vms.is_empty() {
            0
        } else {
            self.node_peers.len() / self.node_vms.len()
        }
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
                safe_version,
                safenode_version,
                safenode_manager_version,
                sn_auditor_version,
            } => {
                println!("===============");
                println!("Version Details");
                println!("===============");
                println!(
                    "faucet version: {}",
                    faucet_version
                        .as_ref()
                        .map_or("N/A".to_string(), |v| v.to_string())
                );
                println!(
                    "safe version: {}",
                    safe_version
                        .as_ref()
                        .map_or("N/A".to_string(), |v| v.to_string())
                );
                println!("safenode version: {}", safenode_version);
                println!("safenode-manager version: {}", safenode_manager_version);
                println!(
                    "sn_auditor version: {}",
                    sn_auditor_version
                        .as_ref()
                        .map_or("N/A".to_string(), |v| v.to_string())
                );
                println!();
            }
        }

        if !self.bootstrap_node_vms.is_empty() {
            println!("=============");
            println!("Bootstrap VMs");
            println!("=============");
            for vm in self.bootstrap_node_vms.iter() {
                println!("{}: {}", vm.0, vm.1);
            }
            println!("Nodes per VM: {}", self.bootstrap_node_count());
            println!("SSH user: {}", self.ssh_user);
            println!();
        }

        println!("========");
        println!("Node VMs");
        println!("========");
        for vm in self.node_vms.iter() {
            println!("{}: {}", vm.0, vm.1);
        }
        println!("Nodes per VM: {}", self.node_count());
        println!("SSH user: {}", self.ssh_user);
        println!();

        if !self.uploader_vms.is_empty() {
            println!("============");
            println!("Uploader VMs");
            println!("============");
            for vm in self.uploader_vms.iter() {
                println!("{}: {}", vm.0, vm.1);
            }
            println!("SSH user: {}", self.ssh_user);
            println!();
        }

        if !self.misc_vms.is_empty() {
            println!("=========");
            println!("Other VMs");
            println!("=========");
            for vm in self.misc_vms.iter() {
                println!("{}: {}", vm.0, vm.1);
            }
            println!("SSH user: {}", self.ssh_user);
            println!();
        }

        // If there are no bootstrap nodes, it's a bootstrap deploy, and in that case, we're not
        // really interested in available peers.
        if !self.bootstrap_node_vms.is_empty() {
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
            println!();
        }

        println!(
            "Genesis: {}",
            self.genesis_multiaddr
                .as_ref()
                .map_or("N/A", |genesis| genesis)
        );
        let inventory_file_path =
            get_data_directory()?.join(format!("{}-inventory.json", self.name));
        println!(
            "The full inventory is at {}",
            inventory_file_path.to_string_lossy()
        );
        println!();

        if let Some(faucet_address) = &self.faucet_address {
            println!("==============");
            println!("Faucet Details");
            println!("==============");
            println!("Faucet address: {}", faucet_address);
            if let Some(genesis) = &self.genesis_multiaddr {
                println!("Check the faucet:");
                println!(
                    "safe --peer {} wallet get-faucet {}",
                    genesis, faucet_address
                );
            }
            println!();
        }

        if !self.auditor_vms.is_empty() {
            println!("===============");
            println!("Auditor Details");
            println!("===============");
            for vm in self.auditor_vms.iter() {
                println!("{}:4242", vm.1);
            }
            println!();
        }

        if !self.uploaded_files.is_empty() {
            println!("Uploaded files:");
            for file in self.uploaded_files.iter() {
                println!("{}: {}", file.0, file.1);
            }
        }
        Ok(())
    }

    pub fn get_genesis_ip(&self) -> Option<IpAddr> {
        self.misc_vms
            .iter()
            .find(|(name, _)| name.contains("genesis"))
            .map(|(_, ip)| *ip)
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
