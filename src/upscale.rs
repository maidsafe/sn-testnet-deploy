// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::provisioning::{NodeType, ProvisionOptions},
    ansible::AnsibleInventoryType,
    error::{Error, Result},
    get_genesis_multiaddr, DeploymentInventory, TestnetDeployer,
};
use colored::Colorize;
use log::debug;
use std::collections::HashSet;

#[derive(Clone)]
pub struct UpscaleOptions {
    pub ansible_verbose: bool,
    pub current_inventory: DeploymentInventory,
    pub desired_auditor_vm_count: Option<u16>,
    pub desired_bootstrap_node_count: Option<u16>,
    pub desired_bootstrap_node_vm_count: Option<u16>,
    pub desired_node_count: Option<u16>,
    pub desired_node_vm_count: Option<u16>,
    pub desired_uploader_vm_count: Option<u16>,
    pub infra_only: bool,
    pub plan: bool,
    pub public_rpc: bool,
}

impl TestnetDeployer {
    pub async fn upscale(&self, options: &UpscaleOptions) -> Result<()> {
        let desired_auditor_vm_count = options
            .desired_auditor_vm_count
            .unwrap_or(options.current_inventory.auditor_vms.len() as u16);
        if desired_auditor_vm_count < options.current_inventory.auditor_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredAuditorVmCount);
        }
        debug!("Using {desired_auditor_vm_count} for desired auditor node VM count");

        let desired_bootstrap_node_vm_count = options
            .desired_bootstrap_node_vm_count
            .unwrap_or(options.current_inventory.bootstrap_node_vms.len() as u16);
        if desired_bootstrap_node_vm_count
            < options.current_inventory.bootstrap_node_vms.len() as u16
        {
            return Err(Error::InvalidUpscaleDesiredBootstrapVmCount);
        }
        debug!("Using {desired_bootstrap_node_vm_count} for desired bootstrap node VM count");

        let desired_node_vm_count = options
            .desired_node_vm_count
            .unwrap_or(options.current_inventory.node_vms.len() as u16);
        if desired_node_vm_count < options.current_inventory.node_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredNodeVmCount);
        }
        debug!("Using {desired_node_vm_count} for desired node VM count");

        let desired_uploader_vm_count = options
            .desired_uploader_vm_count
            .unwrap_or(options.current_inventory.uploader_vms.len() as u16);
        if desired_uploader_vm_count < options.current_inventory.uploader_vms.len() as u16 {
            return Err(Error::InvalidUpscaleDesiredUploaderVmCount);
        }
        debug!("Using {desired_uploader_vm_count} for desired uploader VM count");

        let desired_bootstrap_node_count = options
            .desired_bootstrap_node_count
            .unwrap_or(options.current_inventory.bootstrap_node_count() as u16);
        if desired_bootstrap_node_count < options.current_inventory.bootstrap_node_count() as u16 {
            return Err(Error::InvalidUpscaleDesiredBootstrapNodeCount);
        }
        debug!("Using {desired_bootstrap_node_count} for desired bootstrap node count");

        let desired_node_count = options
            .desired_node_count
            .unwrap_or(options.current_inventory.node_count() as u16);
        if desired_node_count < options.current_inventory.node_count() as u16 {
            return Err(Error::InvalidUpscaleDesiredNodeCount);
        }
        debug!("Using {desired_node_count} for desired node count");

        if options.plan {
            let vars = vec![
                (
                    "auditor_vm_count".to_string(),
                    desired_auditor_vm_count.to_string(),
                ),
                (
                    "bootstrap_node_vm_count".to_string(),
                    desired_bootstrap_node_vm_count.to_string(),
                ),
                (
                    "node_vm_count".to_string(),
                    desired_node_vm_count.to_string(),
                ),
                (
                    "uploader_vm_count".to_string(),
                    desired_uploader_vm_count.to_string(),
                ),
            ];
            self.plan(
                Some(vars),
                options.current_inventory.environment_details.environment_type.clone(),
            )
            .await?;
            return Ok(());
        }

        self.create_or_update_infra(
            &options.current_inventory.name,
            Some(desired_auditor_vm_count),
            Some(desired_bootstrap_node_vm_count),
            Some(desired_node_vm_count),
            Some(desired_uploader_vm_count),
            false,
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

        if options.infra_only {
            return Ok(());
        }

        let mut n = 1;
        let total = 5;

        let provision_options = ProvisionOptions {
            beta_encryption_key: None,
            binary_option: options.current_inventory.binary_option.clone(),
            bootstrap_node_count: desired_bootstrap_node_count,
            env_variables: None,
            log_format: None,
            logstash_details: None,
            name: options.current_inventory.name.clone(),
            node_count: desired_node_count,
            public_rpc: options.public_rpc,
        };
        let mut node_provision_failed = false;
        let (genesis_multiaddr, genesis_ip) =
            get_genesis_multiaddr(&self.ansible_provisioner.ansible_runner, &self.ssh_client)
                .await
                .map_err(|err| {
                    println!("Failed to get genesis multiaddr {err:?}");
                    err
                })?;
        debug!("Retrieved genesis peer {genesis_multiaddr}");

        self.wait_for_ssh_availability_on_new_machines(
            AnsibleInventoryType::BootstrapNodes,
            &options.current_inventory,
        )
        .await?;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Bootstrap Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(&provision_options, &genesis_multiaddr, NodeType::Bootstrap)
            .await
        {
            Ok(()) => {
                println!("Provisioned bootstrap nodes");
            }
            Err(_) => {
                node_provision_failed = true;
            }
        }
        n += 1;

        self.wait_for_ssh_availability_on_new_machines(
            AnsibleInventoryType::Nodes,
            &options.current_inventory,
        )
        .await?;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Normal Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(&provision_options, &genesis_multiaddr, NodeType::Normal)
            .await
        {
            Ok(()) => {
                println!("Provisioned normal nodes");
            }
            Err(_) => {
                node_provision_failed = true;
            }
        }
        n += 1;

        // make sure faucet is running
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Start Faucet");
        self.ansible_provisioner
            .provision_and_start_faucet(&provision_options, &genesis_multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to stop faucet {err:?}");
                err
            })?;
        n += 1;

        self.wait_for_ssh_availability_on_new_machines(
            AnsibleInventoryType::Uploaders,
            &options.current_inventory,
        )
        .await?;
        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Uploaders");
        self.ansible_provisioner
            .provision_uploaders(&provision_options, &genesis_multiaddr, &genesis_ip)
            .await
            .map_err(|err| {
                println!("Failed to provision uploaders {err:?}");
                err
            })?;
        n += 1;

        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Stop Faucet");
        self.ansible_provisioner
            .provision_and_stop_faucet(&provision_options, &genesis_multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to stop faucet {err:?}");
                err
            })?;

        if node_provision_failed {
            println!();
            println!("{}", "WARNING!".yellow());
            println!("Some nodes failed to provision without error.");
            println!("This usually means a small number of nodes failed to start on a few VMs.");
            println!("However, most of the time the deployment will still be usable.");
            println!("See the output from Ansible to determine which VMs had failures.");
        }

        Ok(())
    }

    async fn wait_for_ssh_availability_on_new_machines(
        &self,
        inventory_type: AnsibleInventoryType,
        current_inventory: &DeploymentInventory,
    ) -> Result<()> {
        let inventory = self
            .ansible_provisioner
            .ansible_runner
            .get_inventory(inventory_type.clone(), true)
            .await?;
        let old_set: HashSet<_> = match inventory_type {
            AnsibleInventoryType::BootstrapNodes => current_inventory
                .bootstrap_node_vms
                .iter()
                .cloned()
                .collect(),
            AnsibleInventoryType::Nodes => current_inventory.node_vms.iter().cloned().collect(),
            AnsibleInventoryType::Uploaders => {
                current_inventory.uploader_vms.iter().cloned().collect()
            }
            it => return Err(Error::UpscaleInventoryTypeNotSupported(it.to_string())),
        };
        let new_vms: Vec<_> = inventory
            .into_iter()
            .filter(|item| !old_set.contains(item))
            .collect();
        for vm in new_vms.iter() {
            self.ssh_client
                .wait_for_ssh_availability(&vm.1, &self.cloud_provider.get_ssh_user())?;
        }
        Ok(())
    }
}
