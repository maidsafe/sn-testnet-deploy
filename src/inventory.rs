// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        inventory::{
            generate_environment_inventory, generate_private_node_static_environment_inventory,
            AnsibleInventoryType,
        },
        provisioning::AnsibleProvisioner,
        AnsibleRunner,
    },
    get_bootstrap_cache_url, get_environment_details, get_genesis_multiaddr,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
    BinaryOption, CloudProvider, DeploymentType, EnvironmentDetails, Error, TestnetDeployer,
};
use alloy::hex::ToHexExt;
use ant_service_management::{NodeRegistry, ServiceStatus};
use color_eyre::{eyre::eyre, Result};
use log::debug;
use rand::seq::{IteratorRandom, SliceRandom};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    convert::From,
    fs::File,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

const DEFAULT_CONTACTS_COUNT: usize = 100;
const UNAVAILABLE_NODE: &str = "-";
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

impl From<&TestnetDeployer> for DeploymentInventoryService {
    fn from(item: &TestnetDeployer) -> Self {
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
        let output_inventory_dir_path = self
            .working_directory_path
            .join("ansible")
            .join("inventory");
        generate_environment_inventory(
            name,
            &self.inventory_file_path,
            &output_inventory_dir_path,
        )?;

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

        let genesis_vm = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, false)?;

        let mut misc_vms = Vec::new();
        let build_vm = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, false)?;
        misc_vms.extend(build_vm);

        let nat_gateway_vm = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, false)?
            .first()
            .cloned();

        let generic_node_vms = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Nodes, false)?;

        let private_node_vms = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::PrivateNodes, false)?;

        // Create static inventory for private nodes. Will be used during ansible-playbook run.
        generate_private_node_static_environment_inventory(
            name,
            &output_inventory_dir_path,
            &private_node_vms,
            &nat_gateway_vm,
            &self.ssh_client.private_key_path,
        )?;

        // Set up the SSH client to route through the NAT gateway if it exists. This updates all the client clones.
        if let Some(nat_gateway) = &nat_gateway_vm {
            self.ssh_client
                .set_routed_vms(private_node_vms.clone(), nat_gateway.public_ip_addr)?;
        }

        let peer_cache_node_vms = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::PeerCacheNodes, false)?;

        let uploader_vms = if environment_details.deployment_type != DeploymentType::Bootstrap {
            let uploader_and_sks = self.ansible_provisioner.get_uploader_secret_keys()?;
            uploader_and_sks
                .iter()
                .map(|(vm, sks)| UploaderVirtualMachine {
                    vm: vm.clone(),
                    wallet_public_key: sks
                        .iter()
                        .enumerate()
                        .map(|(user, sk)| (format!("safe{}", user + 1), sk.address().encode_hex()))
                        .collect(),
                })
                .collect()
        } else {
            Vec::new()
        };

        println!("Retrieving node registries from all VMs...");
        let mut failed_node_registry_vms = Vec::new();

        let peer_cache_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::PeerCacheNodes)?;
        let peer_cache_node_vms =
            NodeVirtualMachine::from_list(&peer_cache_node_vms, &peer_cache_node_registries);

        let generic_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Nodes)?;
        let generic_node_vms =
            NodeVirtualMachine::from_list(&generic_node_vms, &generic_node_registries);

        let private_node_registries = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::PrivateNodes)?;
        let private_node_vms =
            NodeVirtualMachine::from_list(&private_node_vms, &private_node_registries);

        let genesis_node_registry = self
            .ansible_provisioner
            .get_node_registries(&AnsibleInventoryType::Genesis)?;
        let genesis_vm = NodeVirtualMachine::from_list(&genesis_vm, &genesis_node_registry);
        let genesis_vm = if !genesis_vm.is_empty() {
            Some(genesis_vm[0].clone())
        } else {
            None
        };

        failed_node_registry_vms.extend(peer_cache_node_registries.failed_vms);
        failed_node_registry_vms.extend(generic_node_registries.failed_vms);
        failed_node_registry_vms.extend(private_node_registries.failed_vms);
        failed_node_registry_vms.extend(genesis_node_registry.failed_vms);

        let binary_option = if let Some(binary_option) = binary_option {
            binary_option
        } else {
            let antnode_version = match environment_details.deployment_type {
                DeploymentType::New => {
                    let (_, genesis_node_registry) = genesis_node_registry
                        .retrieved_registries
                        .iter()
                        .find(|(vm_name, _)| vm_name.contains("genesis-bootstrap"))
                        .ok_or_else(|| eyre!("Unable to retrieve genesis node registry"))?;
                    genesis_node_registry
                        .nodes
                        .first()
                        .ok_or_else(|| eyre!("Unable to obtain the genesis node"))?
                        .version
                        .parse()?
                }
                DeploymentType::Bootstrap => generic_node_registries
                    .retrieved_registries
                    .first()
                    .and_then(|(_, reg)| reg.nodes.first())
                    .ok_or_else(|| eyre!("Unable to obtain a node"))?
                    .version
                    .parse()?,
            };

            let antctl_version = {
                let mut random_vm = None;
                if !generic_node_vms.is_empty() {
                    random_vm = generic_node_vms.first().cloned();
                } else if !peer_cache_node_vms.is_empty() {
                    random_vm = peer_cache_node_vms.first().cloned();
                } else if genesis_vm.is_some() {
                    random_vm = genesis_vm.clone()
                };

                let Some(random_vm) = random_vm else {
                    return Err(eyre!(
                        "Unable to obtain a VM to retrieve the safenode-manager version"
                    ));
                };

                self.get_bin_version(&random_vm.vm, "antctl --version", "Autonomi Node Manager v")?
            };

            let ant_version = if environment_details.deployment_type != DeploymentType::Bootstrap {
                let random_uploader_vm = uploader_vms
                    .choose(&mut rand::thread_rng())
                    .ok_or_else(|| eyre!("No uploader VMs available to retrieve ant version"))?;
                Some(self.get_bin_version(
                    &random_uploader_vm.vm,
                    "ant --version",
                    "Autonomi Client v",
                )?)
            } else {
                None
            };

            println!("Retrieved binary versions from previous deployment:");
            println!("  antnode: {}", antnode_version);
            println!("  antctl: {}", antctl_version);
            if let Some(version) = &ant_version {
                println!("  ant: {}", version);
            }

            BinaryOption::Versioned {
                ant_version,
                antnode_version,
                antctl_version,
            }
        };

        let (genesis_multiaddr, genesis_ip) =
            if environment_details.deployment_type == DeploymentType::New {
                match get_genesis_multiaddr(&self.ansible_runner, &self.ssh_client) {
                    Ok((multiaddr, ip)) => (Some(multiaddr), Some(ip)),
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };
        let inventory = DeploymentInventory {
            binary_option,
            environment_details,
            failed_node_registry_vms,
            faucet_address: genesis_ip.map(|ip| format!("{ip}:8000")),
            genesis_multiaddr,
            genesis_vm,
            name: name.to_string(),
            misc_vms,
            nat_gateway_vm,
            node_vms: generic_node_vms,
            peer_cache_node_vms,
            private_node_vms,
            ssh_user: self.cloud_provider.get_ssh_user(),
            ssh_private_key_path: self.ssh_client.private_key_path.clone(),
            uploaded_files: Vec::new(),
            uploader_vms,
        };
        Ok(inventory)
    }

    /// Create all the environment inventory files. This also updates the SSH client to route the private nodes
    /// the NAT gateway if it exists.
    ///
    /// This is used when 'generate_or_retrieve_inventory' is not used, but you still need to set up the inventory files.
    pub fn setup_environment_inventory(&self, name: &str) -> Result<()> {
        let output_inventory_dir_path = self
            .working_directory_path
            .join("ansible")
            .join("inventory");
        generate_environment_inventory(
            name,
            &self.inventory_file_path,
            &output_inventory_dir_path,
        )?;

        let nat_gateway_vm = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, false)?
            .first()
            .cloned();

        let private_node_vms = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::PrivateNodes, false)?;

        // Create static inventory for private nodes. Will be used during ansible-playbook run.
        generate_private_node_static_environment_inventory(
            name,
            &output_inventory_dir_path,
            &private_node_vms,
            &nat_gateway_vm,
            &self.ssh_client.private_key_path,
        )?;

        // Set up the SSH client to route through the NAT gateway if it exists. This updates all the client clones.
        if let Some(nat_gateway) = &nat_gateway_vm {
            self.ssh_client
                .set_routed_vms(private_node_vms.clone(), nat_gateway.public_ip_addr)?;
        }

        Ok(())
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

        let peer_cache_peers = inventory
            .peer_cache_node_vms
            .iter()
            .flat_map(|vm| vm.get_quic_addresses())
            .collect::<Vec<_>>();
        let peer_cache_peers_len = peer_cache_peers.len();
        for peer in peer_cache_peers
            .iter()
            .filter(|&peer| peer != UNAVAILABLE_NODE)
            .cloned()
            .choose_multiple(&mut rng, DEFAULT_CONTACTS_COUNT)
        {
            writeln!(file, "{peer}",)?;
        }

        if DEFAULT_CONTACTS_COUNT > peer_cache_peers_len {
            let node_peers = inventory
                .node_vms
                .iter()
                .flat_map(|vm| vm.get_quic_addresses())
                .collect::<Vec<_>>();
            for peer in node_peers
                .iter()
                .filter(|&peer| peer != UNAVAILABLE_NODE)
                .cloned()
                .choose_multiple(&mut rng, DEFAULT_CONTACTS_COUNT - peer_cache_peers_len)
            {
                writeln!(file, "{peer}",)?;
            }
        }

        self.s3_repository
            .upload_file(TESTNET_BUCKET_NAME, &temp_file_path, true)
            .await?;

        Ok(())
    }

    /// Connects to a VM with SSH and runs a command to retrieve the version of a binary.
    fn get_bin_version(&self, vm: &VirtualMachine, command: &str, prefix: &str) -> Result<Version> {
        let output = self.ssh_client.run_command(
            &vm.public_ip_addr,
            &self.cloud_provider.get_ssh_user(),
            command,
            true,
        )?;
        let version_line = output
            .first()
            .ok_or_else(|| eyre!("No output from {} command", command))?;
        let version_str = version_line
            .strip_prefix(prefix)
            .ok_or_else(|| eyre!("Unexpected output format from {} command", command))?;
        Version::parse(version_str).map_err(|e| eyre!("Failed to parse {} version: {}", command, e))
    }
}

impl NodeVirtualMachine {
    pub fn from_list(
        vms: &[VirtualMachine],
        node_registries: &DeploymentNodeRegistries,
    ) -> Vec<Self> {
        let mut node_vms = Vec::new();
        for vm in vms {
            let node_registry = node_registries
                .retrieved_registries
                .iter()
                .find(|(name, _)|
                // for private nodes, DeploymentNodeRegistries::name = private_ip_addr
                if vm.name.contains("private-node") {
                    name == &vm.private_ip_addr.to_string()
                } else {
                    name == &vm.name
                })
                .map(|(_, reg)| reg);
            let Some(node_registry) = node_registry else {
                continue;
            };

            let node_vm = Self {
                node_count: node_registry.nodes.len(),
                node_listen_addresses: node_registry
                    .nodes
                    .iter()
                    .map(|node| {
                        if let Some(listen_addresses) = &node.listen_addr {
                            listen_addresses
                                .iter()
                                .map(|addr| addr.to_string())
                                .collect()
                        } else {
                            vec![UNAVAILABLE_NODE.to_string()]
                        }
                    })
                    .collect(),
                rpc_endpoint: node_registry
                    .nodes
                    .iter()
                    .map(|node| {
                        let id = if let Some(peer_id) = node.peer_id {
                            peer_id.to_string().clone()
                        } else {
                            UNAVAILABLE_NODE.to_string()
                        };
                        (id, node.rpc_socket_addr)
                    })
                    .collect(),
                safenodemand_endpoint: node_registry
                    .daemon
                    .as_ref()
                    .and_then(|daemon| daemon.endpoint),
                vm: vm.clone(),
            };
            node_vms.push(node_vm);
        }
        node_vms
    }

    pub fn get_quic_addresses(&self) -> Vec<String> {
        self.node_listen_addresses
            .iter()
            .map(|addresses| {
                addresses
                    .iter()
                    .find(|addr| {
                        addr.contains("/quic-v1")
                            && !addr.starts_with("/ip4/127.0.0.1")
                            && !addr.starts_with("/ip4/10.")
                    })
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| UNAVAILABLE_NODE.to_string())
            })
            .collect()
    }
}

/// The name of the OS user.
pub type OsUser = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploaderVirtualMachine {
    pub vm: VirtualMachine,
    /// The public key of the wallet for each OS user (1 uploader instance per OS user).
    pub wallet_public_key: HashMap<OsUser, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeVirtualMachine {
    pub vm: VirtualMachine,
    pub node_count: usize,
    pub node_listen_addresses: Vec<Vec<String>>,
    pub rpc_endpoint: HashMap<String, SocketAddr>,
    pub safenodemand_endpoint: Option<SocketAddr>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct VirtualMachine {
    pub id: u64,
    pub name: String,
    pub public_ip_addr: IpAddr,
    pub private_ip_addr: IpAddr,
}

#[derive(Clone)]
pub struct DeploymentNodeRegistries {
    pub inventory_type: AnsibleInventoryType,
    /// The (name, NodeRegistry) pairs for each VM that was successfully retrieved.
    /// Note: for private nodes, the name is set to the private address of the VM.
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
    pub binary_option: BinaryOption,
    pub environment_details: EnvironmentDetails,
    pub failed_node_registry_vms: Vec<String>,
    pub faucet_address: Option<String>,
    pub genesis_vm: Option<NodeVirtualMachine>,
    pub genesis_multiaddr: Option<String>,
    pub misc_vms: Vec<VirtualMachine>,
    pub name: String,
    pub nat_gateway_vm: Option<VirtualMachine>,
    pub node_vms: Vec<NodeVirtualMachine>,
    pub peer_cache_node_vms: Vec<NodeVirtualMachine>,
    pub private_node_vms: Vec<NodeVirtualMachine>,
    pub ssh_user: String,
    pub ssh_private_key_path: PathBuf,
    pub uploaded_files: Vec<(String, String)>,
    pub uploader_vms: Vec<UploaderVirtualMachine>,
}

impl DeploymentInventory {
    /// Create an inventory for a new deployment which is initially empty, other than the name and
    /// binary option, which will have been selected.
    pub fn empty(name: &str, binary_option: BinaryOption) -> DeploymentInventory {
        Self {
            binary_option,
            environment_details: EnvironmentDetails::default(),
            genesis_vm: None,
            genesis_multiaddr: None,
            failed_node_registry_vms: Vec::new(),
            faucet_address: None,
            misc_vms: Vec::new(),
            name: name.to_string(),
            nat_gateway_vm: None,
            node_vms: Vec::new(),
            peer_cache_node_vms: Vec::new(),
            private_node_vms: Vec::new(),
            ssh_user: "root".to_string(),
            ssh_private_key_path: PathBuf::new(),
            uploaded_files: Vec::new(),
            uploader_vms: Vec::new(),
        }
    }

    pub fn get_tfvars_filename(&self) -> String {
        let filename = self
            .environment_details
            .environment_type
            .get_tfvars_filename(&self.name);
        debug!("Using tfvars file {filename}",);
        filename
    }

    pub fn is_empty(&self) -> bool {
        self.peer_cache_node_vms.is_empty() && self.node_vms.is_empty()
    }

    pub fn vm_list(&self) -> Vec<VirtualMachine> {
        let mut list = Vec::new();
        list.extend(self.nat_gateway_vm.clone());
        list.extend(
            self.peer_cache_node_vms
                .iter()
                .map(|node_vm| node_vm.vm.clone()),
        );
        list.extend(self.genesis_vm.iter().map(|node_vm| node_vm.vm.clone()));
        list.extend(self.node_vms.iter().map(|node_vm| node_vm.vm.clone()));
        list.extend(self.misc_vms.clone());
        list.extend(
            self.private_node_vms
                .iter()
                .map(|node_vm| node_vm.vm.clone()),
        );
        list.extend(
            self.uploader_vms
                .iter()
                .map(|uploader_vm| uploader_vm.vm.clone()),
        );
        list
    }

    pub fn node_vm_list(&self) -> Vec<NodeVirtualMachine> {
        let mut list = Vec::new();
        list.extend(self.peer_cache_node_vms.iter().cloned());
        list.extend(self.genesis_vm.iter().cloned());
        list.extend(self.node_vms.iter().cloned());
        list.extend(self.private_node_vms.iter().cloned());

        list
    }

    pub fn peers(&self) -> HashSet<String> {
        let mut list = HashSet::new();
        list.extend(
            self.peer_cache_node_vms
                .iter()
                .flat_map(|node_vm| node_vm.get_quic_addresses()),
        );
        list.extend(
            self.genesis_vm
                .iter()
                .flat_map(|node_vm| node_vm.get_quic_addresses()),
        );
        list.extend(
            self.node_vms
                .iter()
                .flat_map(|node_vm| node_vm.get_quic_addresses()),
        );
        list.extend(
            self.private_node_vms
                .iter()
                .flat_map(|node_vm| node_vm.get_quic_addresses()),
        );
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

    pub fn peer_cache_node_count(&self) -> usize {
        if let Some(first_vm) = self.peer_cache_node_vms.first() {
            first_vm.node_count
        } else {
            0
        }
    }

    pub fn genesis_node_count(&self) -> usize {
        if let Some(genesis_vm) = &self.genesis_vm {
            genesis_vm.node_count
        } else {
            0
        }
    }

    pub fn node_count(&self) -> usize {
        if let Some(first_vm) = self.node_vms.first() {
            first_vm.node_count
        } else {
            0
        }
    }

    pub fn private_node_count(&self) -> usize {
        if let Some(first_vm) = self.private_node_vms.first() {
            first_vm.node_count
        } else {
            0
        }
    }

    pub fn print_report(&self, full: bool) -> Result<()> {
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
                ant_version: safe_version,
                antnode_version: safenode_version,
                antctl_version: safenode_manager_version,
            } => {
                println!("===============");
                println!("Version Details");
                println!("===============");
                println!(
                    "safe version: {}",
                    safe_version
                        .as_ref()
                        .map_or("N/A".to_string(), |v| v.to_string())
                );
                println!("safenode version: {}", safenode_version);
                println!("safenode-manager version: {}", safenode_manager_version);
                println!();
            }
        }

        if !self.peer_cache_node_vms.is_empty() {
            println!("==============");
            println!("Peer Cache VMs");
            println!("==============");
            for node_vm in self.peer_cache_node_vms.iter() {
                println!("{}: {}", node_vm.vm.name, node_vm.vm.public_ip_addr);
            }
            println!("Nodes per VM: {}", self.peer_cache_node_count());
            println!("SSH user: {}", self.ssh_user);
            println!();

            self.print_peer_cache_webserver();
        }

        println!("========");
        println!("Node VMs");
        println!("========");
        if let Some(genesis_vm) = &self.genesis_vm {
            println!("{}: {}", genesis_vm.vm.name, genesis_vm.vm.public_ip_addr);
        }
        for node_vm in self.node_vms.iter() {
            println!("{}: {}", node_vm.vm.name, node_vm.vm.public_ip_addr);
        }
        println!("Nodes per VM: {}", self.node_count());
        println!("SSH user: {}", self.ssh_user);
        println!();

        println!("=================");
        println!("Private Node VMs");
        println!("=================");
        for node_vm in self.private_node_vms.iter() {
            println!("{}: {}", node_vm.vm.name, node_vm.vm.public_ip_addr);
            if let Some(nat_gateway_vm) = &self.nat_gateway_vm {
                let ssh = if let Some(ssh_key_path) = self.ssh_private_key_path.to_str() {
                    format!(
                        "ssh -i {ssh_key_path} -o ProxyCommand=\"ssh -W %h:%p root@{} -i {ssh_key_path}\" root@{}",
                        nat_gateway_vm.public_ip_addr, node_vm.vm.private_ip_addr
                    )
                } else {
                    format!(
                        "ssh -o ProxyCommand=\"ssh -W %h:%p root@{}\" root@{}",
                        nat_gateway_vm.public_ip_addr, node_vm.vm.private_ip_addr
                    )
                };
                println!("SSH using NAT gateway: {ssh}");
            } else {
                println!("Error: NAT gateway VM not found");
            }
        }
        println!("Nodes per VM: {}", self.node_count());
        println!("SSH user: {}", self.ssh_user);
        println!();

        if !self.uploader_vms.is_empty() {
            println!("============");
            println!("Uploader VMs");
            println!("============");
            for uploader_vm in self.uploader_vms.iter() {
                println!("{}: {}", uploader_vm.vm.name, uploader_vm.vm.public_ip_addr);
            }
            println!();

            println!("===========================");
            println!("Uploader Wallet Public Keys");
            println!("===========================");
            for uploader_vm in self.uploader_vms.iter() {
                for (user, key) in uploader_vm.wallet_public_key.iter() {
                    println!("{}@{}: {}", uploader_vm.vm.name, user, key);
                }
            }
        }

        if !self.misc_vms.is_empty() || self.nat_gateway_vm.is_some() {
            println!("=========");
            println!("Other VMs");
            println!("=========");
        }
        if !self.misc_vms.is_empty() {
            for vm in self.misc_vms.iter() {
                println!("{}: {}", vm.name, vm.public_ip_addr);
            }
        }

        if let Some(nat_gateway_vm) = &self.nat_gateway_vm {
            println!("{}: {}", nat_gateway_vm.name, nat_gateway_vm.public_ip_addr);
        }

        println!("SSH user: {}", self.ssh_user);
        println!();

        if full {
            println!("===============");
            println!("Full Peer List");
            println!("===============");
            let mut quic_listeners = Vec::new();
            let mut ws_listeners = Vec::new();

            for node_vm in self.peer_cache_node_vms.iter().chain(self.node_vms.iter()) {
                for addresses in &node_vm.node_listen_addresses {
                    for addr in addresses {
                        if !addr.starts_with("/ip4/127.0.0.1") && !addr.starts_with("/ip4/10.") {
                            if addr.contains("/quic") {
                                quic_listeners.push(addr.clone());
                            } else if addr.contains("/ws") {
                                ws_listeners.push(addr.clone());
                            }
                        }
                    }
                }
            }

            if !quic_listeners.is_empty() {
                println!("QUIC:");
                for addr in quic_listeners {
                    println!("  {addr}");
                }
                println!();
            }

            if !ws_listeners.is_empty() {
                println!("Websocket:");
                for addr in ws_listeners {
                    println!("  {addr}");
                }
                println!();
            }
        } else {
            println!("============");
            println!("Sample Peers");
            println!("============");
            self.peer_cache_node_vms
                .iter()
                .chain(self.node_vms.iter())
                .map(|node_vm| node_vm.vm.public_ip_addr.to_string())
                .for_each(|ip| {
                    if let Some(peer) = self.peers().iter().find(|p| p.contains(&ip)) {
                        println!("{peer}");
                    }
                });
        }
        println!();

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

        if !self.uploaded_files.is_empty() {
            println!("Uploaded files:");
            for file in self.uploaded_files.iter() {
                println!("{}: {}", file.0, file.1);
            }
        }

        if self.environment_details.evm_data_payments_address.is_some()
            || self.environment_details.evm_payment_token_address.is_some()
            || self.environment_details.evm_rpc_url.is_some()
        {
            println!("===========");
            println!("EVM Details");
            println!("===========");
            println!(
                "EVM data payments address: {}",
                self.environment_details
                    .evm_data_payments_address
                    .as_ref()
                    .map_or("N/A", |addr| addr)
            );
            println!(
                "EVM payment token address: {}",
                self.environment_details
                    .evm_payment_token_address
                    .as_ref()
                    .map_or("N/A", |addr| addr)
            );
            println!(
                "EVM RPC URL: {}",
                self.environment_details
                    .evm_rpc_url
                    .as_ref()
                    .map_or("N/A", |addr| addr)
            );
        }

        Ok(())
    }

    pub fn get_genesis_ip(&self) -> Option<IpAddr> {
        self.misc_vms
            .iter()
            .find(|vm| vm.name.contains("genesis"))
            .map(|vm| vm.public_ip_addr)
    }

    pub fn print_peer_cache_webserver(&self) {
        println!("=====================");
        println!("Peer Cache Webservers");
        println!("=====================");

        for node_vm in &self.peer_cache_node_vms {
            let webserver = get_bootstrap_cache_url(&node_vm.vm.public_ip_addr);
            println!("{}: {webserver}", node_vm.vm.name);
        }
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
