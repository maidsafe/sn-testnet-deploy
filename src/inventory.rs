// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        generate_environment_inventory, AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner,
    },
    error::{Error, Result},
    get_genesis_multiaddr,
    ssh::SshClient,
    terraform::TerraformRunner,
    BinaryOption, CloudProvider, Node, TestnetDeploy,
};
use log::{debug, trace};
use rand::Rng;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    convert::From,
    fs::File,
    io::{BufReader, Write},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use walkdir::WalkDir;

#[derive(Deserialize)]
struct NodeManagerInventory {
    daemon: Option<Daemon>,
    nodes: Vec<Node>,
}

#[derive(Deserialize, Clone)]
struct Daemon {
    endpoint: Option<SocketAddr>,
}

pub struct DeploymentInventoryService {
    pub ansible_runner: AnsibleRunner,
    pub cloud_provider: CloudProvider,
    pub inventory_file_path: PathBuf,
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
    pub async fn generate_inventory(
        &self,
        name: &str,
        force: bool,
        binary_option: BinaryOption,
        node_instance_count: Option<u16>,
    ) -> Result<DeploymentInventory> {
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
                return Err(Error::EnvironmentDoesNotExist(name.to_string()));
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
        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(name, &self.ansible_runner, &self.ssh_client).await?;

        println!("Retrieving node manager inventory. This can take a minute.");

        let node_manager_inventories = {
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
            binary_option,
            vm_list,
            rpc_endpoints: safenode_rpc_endpoints,
            safenodemand_endpoints,
            ssh_user: self.cloud_provider.get_ssh_user(),
            genesis_multiaddr,
            peers,
            faucet_address: format!("{}:8000", genesis_ip),
            uploaded_files: Vec::new(),
        };
        Ok(inventory)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentInventory {
    pub name: String,
    pub node_count: u16,
    pub binary_option: BinaryOption,
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

        println!("Name: {}", self.name);
        match &self.binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                println!("Branch Details");
                println!("==============");
                println!("Repo owner: {}", repo_owner);
                println!("Branch name: {}", branch);
            }
            BinaryOption::Versioned {
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

fn get_node_manager_inventory(inventory_file_path: &PathBuf) -> Result<NodeManagerInventory> {
    let file = File::open(inventory_file_path)?;
    let reader = BufReader::new(file);
    let inventory = serde_json::from_reader(reader)?;
    Ok(inventory)
}
