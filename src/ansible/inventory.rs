// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::AnsibleRunner;
use crate::{
    ansible::AnsibleBinary, error::Error, inventory::VirtualMachine, run_external_command, Result,
};
use log::{debug, warn};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

/// Represents the inventory types that apply to our own domain.
#[derive(Clone, Debug, Copy)]
pub enum AnsibleInventoryType {
    /// Use to run a playbook against the build machine.
    ///
    /// This is a larger machine that is used for building binaries from source.
    ///
    /// Only one machine will be returned in this inventory.
    Build,
    /// Provide a static list of VMs to connect to.
    Custom,
    /// Use to run a playbook against all EVM nodes.
    EvmNodes,
    /// Use to run a playbook against the genesis node.
    ///
    /// Only one machine will be returned in this inventory.
    Genesis,
    /// Use to run a playbook against the Logstash servers.
    Logstash,
    /// Use to run a playbook against the NAT gateway.
    NatGateway,
    /// Use to run a playbook against all nodes except the genesis node.
    Nodes,
    /// Use to run a playbook against all Peer Cache nodes.
    PeerCacheNodes,
    /// Use to run a inventory against the private nodes. This does not route the ssh connection through the NAT gateway
    /// and hence cannot run playbooks. Use PrivateNodesStatic for that.
    PrivateNodes,
    /// Use to run a playbook against the private nodes. This is similar to the PrivateNodes inventory, but uses
    /// a static custom inventory file. This is just used for running playbooks and not inventory.
    PrivateNodesStatic,
    /// Use to run a playbook against all the uploader machines.
    Uploaders,
}

impl std::fmt::Display for AnsibleInventoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AnsibleInventoryType::PeerCacheNodes => "BootstrapNodes",
            AnsibleInventoryType::Build => "Build",
            AnsibleInventoryType::Custom => "Custom",
            AnsibleInventoryType::EvmNodes => "EvmNodes",
            AnsibleInventoryType::Genesis => "Genesis",
            AnsibleInventoryType::Logstash => "Logstash",
            AnsibleInventoryType::NatGateway => "NatGateway",
            AnsibleInventoryType::Nodes => "Nodes",
            AnsibleInventoryType::PrivateNodes => "PrivateNodes",
            AnsibleInventoryType::PrivateNodesStatic => "PrivateNodesStatic",
            AnsibleInventoryType::Uploaders => "Uploaders",
        };
        write!(f, "{}", s)
    }
}

impl AnsibleInventoryType {
    pub fn get_inventory_path(&self, name: &str, provider: &str) -> PathBuf {
        match &self {
            Self::PeerCacheNodes => {
                PathBuf::from(format!(".{name}_bootstrap_node_inventory_{provider}.yml"))
            }
            Self::Build => PathBuf::from(format!(".{name}_build_inventory_{provider}.yml")),
            Self::Custom => PathBuf::from(format!(".{name}_custom_inventory_{provider}.ini")),
            Self::EvmNodes => PathBuf::from(format!(".{name}_evm_node_inventory_{provider}.yml")),
            Self::Genesis => PathBuf::from(format!(".{name}_genesis_inventory_{provider}.yml")),
            Self::Logstash => PathBuf::from(format!(".{name}_logstash_inventory_{provider}.yml")),
            Self::NatGateway => {
                PathBuf::from(format!(".{name}_nat_gateway_inventory_{provider}.yml"))
            }
            Self::Nodes => PathBuf::from(format!(".{name}_node_inventory_{provider}.yml")),
            Self::PrivateNodes => {
                PathBuf::from(format!(".{name}_private_node_inventory_{provider}.yml"))
            }
            Self::PrivateNodesStatic => PathBuf::from(format!(
                ".{name}_private_node_static_inventory_{provider}.yml"
            )),
            Self::Uploaders => PathBuf::from(format!(".{name}_uploader_inventory_{provider}.yml")),
        }
    }

    pub fn tag(&self) -> &str {
        match self {
            Self::PeerCacheNodes => "bootstrap_node",
            Self::Build => "build",
            Self::Custom => "custom",
            Self::EvmNodes => "evm_node",
            Self::Genesis => "genesis",
            Self::Logstash => "logstash",
            Self::NatGateway => "nat_gateway",
            Self::Nodes => "node",
            Self::PrivateNodes => "private_node",
            Self::PrivateNodesStatic => "private_node",
            Self::Uploaders => "uploader",
        }
    }

    pub fn iter_node_type() -> impl Iterator<Item = Self> {
        [
            Self::Genesis,
            Self::PeerCacheNodes,
            Self::Nodes,
            Self::PrivateNodes,
        ]
        .into_iter()
    }
}

impl AnsibleRunner {
    /// Runs Ansible's inventory command and returns a list of VirtualMachines.
    pub fn get_inventory(
        &self,
        inventory_type: AnsibleInventoryType,
        re_attempt: bool,
    ) -> Result<Vec<VirtualMachine>> {
        let retry_count = if re_attempt { 3 } else { 0 };
        let mut count = 0;
        let mut inventory = Vec::new();

        while count <= retry_count {
            debug!("Running inventory list. retry attempts {count}/{retry_count}");
            let output = run_external_command(
                AnsibleBinary::AnsibleInventory.get_binary_path()?,
                self.working_directory_path.clone(),
                vec![
                    "--inventory".to_string(),
                    self.get_inventory_path(&inventory_type)?
                        .to_string_lossy()
                        .to_string(),
                    "--list".to_string(),
                ],
                true,
                false,
            )?;

            debug!("Inventory list output:");
            debug!("{output:#?}");
            let mut output_string = output
                .into_iter()
                .skip_while(|line| !line.starts_with('{'))
                .collect::<Vec<String>>()
                .join("\n");
            if let Some(end_index) = output_string.rfind('}') {
                output_string.truncate(end_index + 1);
            }
            let parsed: Output = serde_json::from_str(&output_string)?;

            for host in parsed._meta.hostvars.values() {
                let public_ip_details = host
                    .do_networks
                    .v4
                    .iter()
                    .find(|&ip| ip.ip_type == IpType::Public)
                    .ok_or_else(|| Error::IpDetailsNotObtained)?;

                let private_ip_details = host
                    .do_networks
                    .v4
                    .iter()
                    .find(|&ip| ip.ip_type == IpType::Private)
                    .ok_or_else(|| Error::IpDetailsNotObtained)?;

                inventory.push(VirtualMachine {
                    id: host.do_id,
                    name: host.do_name.clone(),
                    public_ip_addr: public_ip_details.ip_address,
                    private_ip_addr: private_ip_details.ip_address,
                });
            }

            count += 1;
            if !inventory.is_empty() {
                break;
            }
            debug!("Inventory list is empty, re-running after a few seconds.");
            std::thread::sleep(Duration::from_secs(3));
        }
        if inventory.is_empty() {
            warn!("Inventory list is empty after {retry_count} retries");
        }

        Ok(inventory)
    }
}

/// Generate necessary inventory files for a given environment.
///
/// These files are based from a template in the base directory.
pub fn generate_environment_inventory(
    environment_name: &str,
    base_inventory_path: &Path,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let inventory_types = [
        AnsibleInventoryType::PeerCacheNodes,
        AnsibleInventoryType::Build,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::NatGateway,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PrivateNodes,
        AnsibleInventoryType::Uploaders,
        AnsibleInventoryType::EvmNodes,
    ];
    for inventory_type in inventory_types.into_iter() {
        let src_path = base_inventory_path;
        let dest_path = output_inventory_dir_path
            .join(inventory_type.get_inventory_path(environment_name, "digital_ocean"));
        if dest_path.is_file() {
            // The inventory has already been generated by a previous run, so just move on.
            continue;
        }

        let mut contents = std::fs::read_to_string(src_path)?;
        contents = contents.replace("env_value", environment_name);
        contents = contents.replace("type_value", inventory_type.tag());
        std::fs::write(&dest_path, contents)?;
        debug!("Created inventory file at {dest_path:#?}");
    }

    Ok(())
}

/// Cleanup the inventory files for a given environment.
///
/// If no inventory_type are provided, the default inventory files are removed.
pub fn cleanup_environment_inventory(
    environment_name: &str,
    output_inventory_dir_path: &Path,
    inventory_types: Option<Vec<AnsibleInventoryType>>,
) -> Result<()> {
    let default_inventory_types = [
        AnsibleInventoryType::PeerCacheNodes,
        AnsibleInventoryType::Build,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::NatGateway,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PrivateNodes,
        AnsibleInventoryType::PrivateNodesStatic,
        AnsibleInventoryType::Uploaders,
        AnsibleInventoryType::EvmNodes,
    ];
    let inventory_types = inventory_types
        .as_deref()
        .unwrap_or(&default_inventory_types);

    for inventory_type in inventory_types.iter() {
        let dest_path = output_inventory_dir_path
            .join(inventory_type.get_inventory_path(environment_name, "digital_ocean"));
        if dest_path.is_file() {
            std::fs::remove_file(&dest_path)?;
            debug!("Removed inventory file at {dest_path:#?}");
        }
    }

    Ok(())
}

/// Generate the custom inventory for the environment.
pub fn generate_custom_environment_inventory(
    vm_list: &[VirtualMachine],
    environment_name: &str,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let dest_path = output_inventory_dir_path
        .join(AnsibleInventoryType::Custom.get_inventory_path(environment_name, "digital_ocean"));
    debug!("Creating custom inventory file at {dest_path:#?}");
    let file = File::create(&dest_path)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "[custom]")?;
    for vm in vm_list.iter() {
        debug!("Adding VM to custom inventory: {}", vm.public_ip_addr);
        writeln!(writer, "{}", vm.public_ip_addr)?;
    }

    debug!("Created custom inventory file at {dest_path:#?}");

    Ok(())
}

/// Generate the static inventory for the private node. This is just used during ansible-playbook.
pub fn generate_private_node_static_environment_inventory(
    environment_name: &str,
    output_inventory_dir_path: &Path,
    private_node_vms: &[VirtualMachine],
    nat_gateway_vm: &Option<VirtualMachine>,
    ssh_sk_path: &Path,
) -> Result<()> {
    let Some(nat_gateway_vm) = nat_gateway_vm.clone() else {
        println!("No NAT gateway VM found. Skipping private node static inventory generation.");
        return Ok(());
    };

    if private_node_vms.is_empty() {
        return Err(Error::EmptyInventory(AnsibleInventoryType::PrivateNodes));
    }

    let dest_path = output_inventory_dir_path.join(
        AnsibleInventoryType::PrivateNodesStatic
            .get_inventory_path(environment_name, "digital_ocean"),
    );
    if dest_path.exists() {
        return Ok(());
    }
    debug!("Generating private node static inventory at {dest_path:?}",);

    let mut file = File::create(&dest_path)?;
    writeln!(file, "[private_nodes]")?;
    for vm in private_node_vms.iter() {
        writeln!(file, "{}", vm.private_ip_addr)?;
    }

    writeln!(file, "[private_nodes:vars]")?;
    writeln!(
        file,
        "ansible_ssh_common_args='-o ProxyCommand=\"ssh -p 22 -W %h:%p -q root@{} -i \"{}\" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null\"'",
        nat_gateway_vm.public_ip_addr,
        ssh_sk_path.to_string_lossy()
    )?;
    writeln!(file, "ansible_host_key_checking=False")?;

    debug!("Created private node inventory file with ssh proxy at {dest_path:?}");

    Ok(())
}

// The following three structs are utilities that are used to parse the output of the
// `ansible-inventory` command.
#[derive(Debug, Deserialize, Clone, PartialEq)]
enum IpType {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "private")]
    Private,
}

#[derive(Debug, Deserialize, Clone)]
struct IpDetails {
    ip_address: IpAddr,
    #[serde(rename = "type")]
    ip_type: IpType,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanNetwork {
    v4: Vec<IpDetails>,
}

#[derive(Debug, Deserialize)]
struct HostVar {
    do_id: u64,
    do_name: String,
    do_networks: DigitalOceanNetwork,
}
#[derive(Debug, Deserialize)]
struct Meta {
    hostvars: HashMap<String, HostVar>,
}
#[derive(Debug, Deserialize)]
struct Output {
    _meta: Meta,
}
