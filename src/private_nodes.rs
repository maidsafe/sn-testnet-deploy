// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    DeploymentInventory, DeploymentType, TestnetDeployer,
};

#[derive(Clone)]
pub struct PrivateNodeOptions {
    pub ansible_verbose: bool,
    pub current_inventory: DeploymentInventory,
}

impl TestnetDeployer {
    pub async fn setup_private_nodes(&self, options: &PrivateNodeOptions) -> Result<()> {
        self.create_or_update_infra(
            &options.current_inventory.name,
            Some(
                match options
                    .current_inventory
                    .environment_details
                    .deployment_type
                {
                    DeploymentType::New => 1,
                    DeploymentType::Bootstrap => 0,
                },
            ),
            Some(options.current_inventory.auditor_vms.len() as u16),
            Some(options.current_inventory.bootstrap_node_vms.len() as u16),
            Some(options.current_inventory.node_vms.len() as u16),
            Some(options.current_inventory.uploader_vms.len() as u16),
            false,
            true,
            &options
                .current_inventory
                .environment_details
                .environment_type
                .get_tfvars_filename(),
        )
        .await
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        let mut n = 1;
        let total = 2;

        let last_vm_private_ip = options
            .current_inventory
            .node_vms
            .iter()
            .find(|vm| {
                vm.name.contains(&format!(
                    "{}-node-{}",
                    options.current_inventory.name,
                    options.current_inventory.node_vms.len()
                ))
            })
            .ok_or_else(|| Error::PrivateIpNotObtained)
            .inspect_err(|err| println!("Failed to obtain node private IP: {err:?}"))?
            .private_ip_addr;

        n += 1;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision NAT Gateway");
        self.ansible_provisioner
            .provision_nat_gateway(&options.current_inventory.name, last_vm_private_ip)
            .await
            .map_err(|err| {
                println!("Failed to provision NAT gateway {err:?}");
                err
            })?;

        n += 1;
        self.ansible_provisioner.print_ansible_run_banner(
            n,
            total,
            "Provision Private Nodes on the last VM",
        );
        // self.ansible_provisioner
        //     .provision_private_nodes(&options.current_inventory.name)
        //     .await
        //     .map_err(|err| {
        //         println!("Failed to provision private nodes {err:?}");
        //         err
        //     })?;

        Ok(())
    }
}
