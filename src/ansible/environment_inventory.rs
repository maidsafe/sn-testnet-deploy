// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::Result;
use crate::inventory::VirtualMachine;
use log::debug;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use super::AnsibleInventoryType;

/// Generate necessary inventory files for a given environment.
///
/// These files are based from a template in the base directory.
pub fn generate_environment_inventory(
    environment_name: &str,
    base_inventory_path: &Path,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let inventory_types = [
        AnsibleInventoryType::Auditor,
        AnsibleInventoryType::BootstrapNodes,
        AnsibleInventoryType::Build,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::NatGateway,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PrivateNodes,
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

        let mut contents = std::fs::read_to_string(src_path)?;
        contents = contents.replace("env_value", environment_name);
        contents = contents.replace("type_value", inventory_type.do_tag());
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
        AnsibleInventoryType::Auditor,
        AnsibleInventoryType::BootstrapNodes,
        AnsibleInventoryType::Build,
        AnsibleInventoryType::Genesis,
        AnsibleInventoryType::NatGateway,
        AnsibleInventoryType::Nodes,
        AnsibleInventoryType::PrivateNodes,
        AnsibleInventoryType::PrivateNodesStatic,
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
    let file = File::create(&dest_path)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "[custom]")?;
    for vm in vm_list.iter() {
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
    println!(
        "Generating private node static inventory. Via ssh proxy: {}",
        nat_gateway_vm.is_some()
    );
    let dest_path = output_inventory_dir_path.join(
        AnsibleInventoryType::PrivateNodesStatic
            .get_inventory_path(environment_name, "digital_ocean"),
    );
    debug!("Created inventory file at {dest_path:?}");

    let mut file = File::create(&dest_path)?;
    writeln!(file, "[private_nodes]")?;
    for vm in private_node_vms.iter() {
        if nat_gateway_vm.is_some() {
            writeln!(file, "{}", vm.private_ip_addr)?;
        } else {
            writeln!(file, "{}", vm.public_ip_addr)?;
        }
    }

    if let Some(nat_gateway_vm) = nat_gateway_vm {
        writeln!(file, "[private_nodes:vars]")?;
        writeln!(
            file,
            "ansible_ssh_common_args='-o ProxyCommand=\"ssh -p 22 -W %h:%p -q root@{} -i \"{}\"\"'",
            nat_gateway_vm.public_ip_addr,
            ssh_sk_path.to_string_lossy()
        )?;
    }

    debug!("Created private node inventory file with ssh proxy at {dest_path:?}");

    Ok(())
}
