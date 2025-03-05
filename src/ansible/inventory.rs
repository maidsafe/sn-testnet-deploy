// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::AnsibleRunner;
use crate::{
    ansible::{provisioning::PrivateNodeProvisionInventory, AnsibleBinary},
    error::Error,
    inventory::VirtualMachine,
    run_external_command, Result,
};
use log::{debug, error, warn};
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
    /// Use to run a playbook against the Full Cone NAT gateway.
    FullConeNatGateway,
    /// Use to run a playbook against a static list of Full Cone NAT gateway.
    FullConeNatGatewayStatic,
    /// Use to run a inventory against the Full Cone NAT private nodes. This does not route the ssh connection through
    /// the NAT gateway and hence cannot run playbooks. Use PrivateNodesStatic for playbooks.
    FullConePrivateNodes,
    /// Use to run a playbook against the private nodes. This is similar to the PrivateNodes inventory, but uses
    /// a static custom inventory file. This is just used for running playbooks and not inventory.
    FullConePrivateNodesStatic,
    /// Use to run a playbook against the genesis node.
    ///
    /// Only one machine will be returned in this inventory.
    Genesis,
    /// Use to run a playbook against the Logstash servers.
    Logstash,
    /// Use to run a playbook against all nodes except the genesis node.
    Nodes,
    /// Use to run a playbook against all Peer Cache nodes.
    PeerCacheNodes,
    /// Use to run a playbook against the Symmetric NAT gateway.
    SymmetricNatGateway,
    /// Use to run a inventory against the Symmetric NAT private nodes. This does not route the ssh connection through
    /// the NAT gateway and hence cannot run playbooks. Use PrivateNodesStatic for playbooks.
    SymmetricPrivateNodes,
    /// Use to run a playbook against the private nodes. This is similar to the PrivateNodes inventory, but uses
    /// a static custom inventory file. This is just used for running playbooks and not inventory.
    SymmetricPrivateNodesStatic,
    /// Use to run a playbook against all the uploader machines.
    Uploaders,
}

impl std::fmt::Display for AnsibleInventoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AnsibleInventoryType::PeerCacheNodes => "PeerCacheNodes",
            AnsibleInventoryType::Build => "Build",
            AnsibleInventoryType::Custom => "Custom",
            AnsibleInventoryType::EvmNodes => "EvmNodes",
            AnsibleInventoryType::FullConeNatGateway => "FullConeNatGateway",
            AnsibleInventoryType::FullConeNatGatewayStatic => "FullConeNatGatewayStatic",
            AnsibleInventoryType::FullConePrivateNodes => "FullConePrivateNodes",
            AnsibleInventoryType::FullConePrivateNodesStatic => "FullConePrivateNodesStatic",
            AnsibleInventoryType::Genesis => "Genesis",
            AnsibleInventoryType::Logstash => "Logstash",
            AnsibleInventoryType::Nodes => "Nodes",
            AnsibleInventoryType::SymmetricNatGateway => "SymmetricNatGateway",
            AnsibleInventoryType::SymmetricPrivateNodes => "SymmetricPrivateNodes",
            AnsibleInventoryType::SymmetricPrivateNodesStatic => "SymmetricPrivateNodesStatic",
            AnsibleInventoryType::Uploaders => "Uploaders",
        };
        write!(f, "{}", s)
    }
}

impl AnsibleInventoryType {
    pub fn get_inventory_path(&self, name: &str, provider: &str) -> PathBuf {
        match &self {
            Self::PeerCacheNodes => {
                PathBuf::from(format!(".{name}_peer_cache_node_inventory_{provider}.yml"))
            }
            Self::Build => PathBuf::from(format!(".{name}_build_inventory_{provider}.yml")),
            Self::Custom => PathBuf::from(format!(".{name}_custom_inventory_{provider}.ini")),
            Self::EvmNodes => PathBuf::from(format!(".{name}_evm_node_inventory_{provider}.yml")),
            Self::FullConeNatGateway => PathBuf::from(format!(
                ".{name}_full_cone_nat_gateway_inventory_{provider}.yml"
            )),
            Self::FullConeNatGatewayStatic => PathBuf::from(format!(
                ".{name}_full_cone_nat_gateway_static_inventory_{provider}.yml"
            )),
            Self::FullConePrivateNodes => PathBuf::from(format!(
                ".{name}_full_cone_private_node_inventory_{provider}.yml"
            )),
            Self::FullConePrivateNodesStatic => PathBuf::from(format!(
                ".{name}_full_cone_private_node_static_inventory_{provider}.yml"
            )),
            Self::Genesis => PathBuf::from(format!(".{name}_genesis_inventory_{provider}.yml")),
            Self::Logstash => PathBuf::from(format!(".{name}_logstash_inventory_{provider}.yml")),
            Self::Nodes => PathBuf::from(format!(".{name}_node_inventory_{provider}.yml")),
            Self::SymmetricNatGateway => PathBuf::from(format!(
                ".{name}_symmetric_nat_gateway_inventory_{provider}.yml"
            )),
            Self::SymmetricPrivateNodes => PathBuf::from(format!(
                ".{name}_symmetric_private_node_inventory_{provider}.yml"
            )),
            Self::SymmetricPrivateNodesStatic => PathBuf::from(format!(
                ".{name}_symmetric_private_node_static_inventory_{provider}.yml"
            )),
            Self::Uploaders => PathBuf::from(format!(".{name}_uploader_inventory_{provider}.yml")),
        }
    }

    pub fn tag(&self) -> &str {
        match self {
            Self::PeerCacheNodes => "peer_cache_node",
            Self::Build => "build",
            Self::Custom => "custom",
            Self::EvmNodes => "evm_node",
            Self::FullConeNatGateway => "full_cone_nat_gateway",
            Self::FullConeNatGatewayStatic => "full_cone_nat_gateway",
            Self::FullConePrivateNodes => "full_cone_private_node",
            Self::FullConePrivateNodesStatic => "full_cone_private_node",
            Self::Genesis => "genesis",
            Self::Logstash => "logstash",
            Self::Nodes => "node",
            Self::SymmetricNatGateway => "symmetric_nat_gateway",
            Self::SymmetricPrivateNodes => "symmetric_private_node",
            Self::SymmetricPrivateNodesStatic => "symmetric_private_node",
            Self::Uploaders => "uploader",
        }
    }

    pub fn iter_node_type() -> impl Iterator<Item = Self> {
        [
            Self::Genesis,
            Self::FullConePrivateNodes,
            Self::Nodes,
            Self::PeerCacheNodes,
            Self::SymmetricPrivateNodes,
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
            if count > 0 {
                debug!("Running inventory list. Retry attempts {count}/{retry_count}");
            } else {
                debug!("Running inventory list.");
            }
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
            if count <= retry_count && re_attempt {
                debug!("Inventory list is empty, re-running after a few seconds.");
                std::thread::sleep(Duration::from_secs(3));
            }
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
        AnsibleInventoryType::Build,
        AnsibleInventoryType::EvmNodes,
        AnsibleInventoryType::FullConeNatGateway,
        AnsibleInventoryType::FullConePrivateNodes,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PeerCacheNodes,
        AnsibleInventoryType::SymmetricNatGateway,
        AnsibleInventoryType::SymmetricPrivateNodes,
        AnsibleInventoryType::Uploaders,
    ];
    for inventory_type in inventory_types.into_iter() {
        let src_path = base_inventory_path;
        let dest_path = output_inventory_dir_path
            .join(inventory_type.get_inventory_path(environment_name, "digital_ocean"));
        if dest_path.is_file() {
            // The inventory has already been generated by a previous run, so just move on.
            continue;
        }

        let mut contents = std::fs::read_to_string(src_path).inspect_err(|err| {
            error!("Failed to read inventory template file at {src_path:?}: {err}",)
        })?;
        contents = contents.replace("env_value", environment_name);
        contents = contents.replace("type_value", inventory_type.tag());
        std::fs::write(&dest_path, contents)
            .inspect_err(|err| error!("Failed to write inventory file at {dest_path:?}: {err}",))?;
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
        AnsibleInventoryType::Build,
        AnsibleInventoryType::EvmNodes,
        AnsibleInventoryType::FullConeNatGateway,
        AnsibleInventoryType::FullConePrivateNodes,
        AnsibleInventoryType::FullConePrivateNodesStatic,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PeerCacheNodes,
        AnsibleInventoryType::SymmetricNatGateway,
        AnsibleInventoryType::SymmetricPrivateNodes,
        AnsibleInventoryType::SymmetricPrivateNodesStatic,
        AnsibleInventoryType::Uploaders,
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

/// Generate the Full Cone NAT gateway static inventory for the environment.
/// This is used during upscale of the Full Cone NAT gateway.
pub fn generate_full_cone_nat_gateway_static_environment_inventory(
    vm_list: &[VirtualMachine],
    environment_name: &str,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let dest_path = output_inventory_dir_path.join(
        AnsibleInventoryType::FullConeNatGatewayStatic
            .get_inventory_path(environment_name, "digital_ocean"),
    );
    debug!("Creating full cone nat gateway static inventory file at {dest_path:#?}");
    let file = File::create(&dest_path)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "[full_cone_nat_gateway]")?;
    for vm in vm_list.iter() {
        debug!(
            "Adding VM to full cone nat gateway static inventory: {}",
            vm.public_ip_addr
        );
        writeln!(writer, "{}", vm.public_ip_addr)?;
    }

    debug!("Created full cone nat gateway inventory file at {dest_path:#?}");

    Ok(())
}

/// Generate the static inventory for the private node that are behind a Symmetric NAT gateway.
/// This is just used during ansible-playbook.
pub fn generate_symmetric_private_node_static_environment_inventory(
    environment_name: &str,
    output_inventory_dir_path: &Path,
    symmetric_private_node_vms: &[VirtualMachine],
    symmetric_nat_gateway_vms: &[VirtualMachine],
    ssh_sk_path: &Path,
) -> Result<()> {
    if symmetric_nat_gateway_vms.is_empty() {
        println!("No Symmetric NAT gateway VMs found. Skipping symmetric private node static inventory generation.");
        return Ok(());
    };

    if symmetric_private_node_vms.is_empty() {
        return Err(Error::EmptyInventory(
            AnsibleInventoryType::SymmetricPrivateNodes,
        ));
    }

    let private_node_nat_gateway_map =
        PrivateNodeProvisionInventory::match_private_node_vm_and_gateway_vm(
            symmetric_private_node_vms,
            symmetric_nat_gateway_vms,
        )?;

    let dest_path = output_inventory_dir_path.join(
        AnsibleInventoryType::SymmetricPrivateNodesStatic
            .get_inventory_path(environment_name, "digital_ocean"),
    );
    debug!("Generating symmetric private node static inventory at {dest_path:?}",);

    let mut file = File::create(&dest_path)?;

    for (privat_node_vm, nat_gateway_vm) in private_node_nat_gateway_map.iter() {
        let node_number = privat_node_vm.name.split('-').last().unwrap();
        writeln!(file, "[symmetric_private_node_{}]", node_number)?;
        writeln!(file, "{}", privat_node_vm.private_ip_addr)?;
        writeln!(file, "[symmetric_private_node_{}:vars]", node_number)?;
        writeln!(
            file,
            "ansible_ssh_common_args='-o ProxyCommand=\"ssh -p 22 -W %h:%p -q root@{} -i \"{}\" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null\"'",
            nat_gateway_vm.public_ip_addr,
            ssh_sk_path.to_string_lossy()
        )?;
        writeln!(file, "ansible_host_key_checking=False")?;
    }

    debug!("Created symmetric private node inventory file with ssh proxy at {dest_path:?}");

    Ok(())
}

/// Generate the static inventory for the private node that are behind a Full Cone NAT gateway.
/// This is just used during ansible-playbook.
pub fn generate_full_cone_private_node_static_environment_inventory(
    environment_name: &str,
    output_inventory_dir_path: &Path,
    full_cone_private_node_vms: &[VirtualMachine],
    full_cone_nat_gateway_vms: &[VirtualMachine],
    ssh_sk_path: &Path,
) -> Result<()> {
    if full_cone_nat_gateway_vms.is_empty() {
        println!("No full cone NAT gateway VMs found. Skipping full cone private node static inventory generation.");
        return Ok(());
    };

    if full_cone_private_node_vms.is_empty() {
        return Err(Error::EmptyInventory(
            AnsibleInventoryType::FullConePrivateNodes,
        ));
    }

    let private_node_nat_gateway_map =
        PrivateNodeProvisionInventory::match_private_node_vm_and_gateway_vm(
            full_cone_private_node_vms,
            full_cone_nat_gateway_vms,
        )?;

    let dest_path = output_inventory_dir_path.join(
        AnsibleInventoryType::FullConePrivateNodesStatic
            .get_inventory_path(environment_name, "digital_ocean"),
    );
    debug!("Generating full cone private node static inventory at {dest_path:?}",);

    let mut file = File::create(&dest_path)?;

    for (privat_node_vm, nat_gateway_vm) in private_node_nat_gateway_map.iter() {
        let node_number = privat_node_vm.name.split('-').last().unwrap();
        writeln!(file, "[full_cone_private_node_{}]", node_number)?;

        writeln!(file, "{}", privat_node_vm.private_ip_addr)?;

        writeln!(file, "[full_cone_private_node_{}:vars]", node_number)?;
        writeln!(
            file,
            "ansible_ssh_common_args='-o ProxyCommand=\"ssh -p 22 -W %h:%p -q root@{} -i \"{}\" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null\"'",
            nat_gateway_vm.public_ip_addr,
            ssh_sk_path.to_string_lossy()

        )?;

        writeln!(file, "ansible_ssh_extra_args='-o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no -i \"{}\"'", ssh_sk_path.to_string_lossy())?;
        writeln!(file, "ansible_host_key_checking=False")?;
    }

    debug!("Created full cone private node inventory file with ssh proxy at {dest_path:?}");

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
