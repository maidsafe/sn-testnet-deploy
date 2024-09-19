// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        environment_inventory::generate_private_node_static_environment_inventory,
        AnsibleInventoryType,
    },
    error::{Error, Result},
    DeploymentInventory, DeploymentType, TestnetDeployer,
};
use std::path::PathBuf;

#[derive(Clone)]
pub struct HomeNodesOptions {
    pub ansible_verbose: bool,
    pub current_inventory: DeploymentInventory,
    pub output_inventory_dir_path: PathBuf,
}

impl TestnetDeployer {
    pub async fn introduce_home_nodes(&self, options: &HomeNodesOptions) -> Result<()> {
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
            Some(options.current_inventory.private_node_vms.len() as u16),
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
        let total = 4;

        let private_vm_inventory = options
            .current_inventory
            .private_node_vms
            .first()
            .ok_or_else(|| Error::EmptyInventory(AnsibleInventoryType::PrivateNodes))
            .inspect_err(|err| {
                println!("Failed to obtain the inventory of the last vm: {err:?}")
            })?;

        n += 1;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision NAT Gateway");
        self.ansible_provisioner
            .provision_nat_gateway(
                &options.current_inventory.name,
                private_vm_inventory.private_ip_addr,
            )
            .await
            .map_err(|err| {
                println!("Failed to provision NAT gateway {err:?}");
                err
            })?;

        n += 1;
        self.ansible_provisioner.print_ansible_run_banner(
            n,
            total,
            "Get NAT Gateway private IP address",
        );
        let nat_gateway_inventory = self
            .ansible_provisioner
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, true)
            .await
            .map_err(|err| {
                println!("Failed to get NAT Gateway inventory {err:?}");
                err
            })?
            .first()
            .ok_or_else(|| Error::EmptyInventory(AnsibleInventoryType::NatGateway))?
            .clone();

        n += 1;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision home nodes");
        generate_private_node_static_environment_inventory(
            &options.current_inventory.name,
            &options.output_inventory_dir_path,
            &options.current_inventory.private_node_vms,
            &Some(nat_gateway_inventory.clone()),
            &self.ssh_client.private_key_path,
        )?;
        self.ansible_provisioner
            .provision_home_nodes(&options.current_inventory.name, &nat_gateway_inventory)
            .await
            .map_err(|err| {
                println!("Failed to provision home nodes {err:?}");
                err
            })?;

        Ok(())
    }
}
