// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::provisioning::{NodeType, ProvisionOptions},
    error::Result,
    write_environment_details, BinaryOption, DeploymentType, EnvironmentDetails, EnvironmentType,
    LogFormat, TestnetDeployer,
};
use colored::Colorize;

#[derive(Clone)]
pub struct BootstrapOptions {
    pub binary_option: BinaryOption,
    pub bootstrap_peer: String,
    pub environment_type: EnvironmentType,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: Option<u16>,
    pub private_node_vm_count: u16,
}

impl TestnetDeployer {
    pub async fn bootstrap(&self, options: &BootstrapOptions) -> Result<()> {
        let build_custom_binaries = {
            match &options.binary_option {
                BinaryOption::BuildFromSource { .. } => true,
                BinaryOption::Versioned { .. } => false,
            }
        };

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                environment_type: options.environment_type.clone(),
                deployment_type: DeploymentType::Bootstrap,
            },
        )
        .await?;

        self.create_or_update_infra(
            &options.name,
            Some(0),
            Some(0),
            Some(0),
            options.node_vm_count,
            Some(options.private_node_vm_count),
            Some(0),
            build_custom_binaries,
            false,
            &options.environment_type.get_tfvars_filename(),
        )
        .await
        .map_err(|err| {
            println!("Failed to create infra {err:?}");
            err
        })?;

        let mut n = 1;
        let total = if build_custom_binaries { 2 } else { 1 };

        let provision_options = ProvisionOptions::from(options.clone());
        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner(n, total, "Build Custom Binaries");
            self.ansible_provisioner
                .build_safe_network_binaries(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
            n += 1;
        }

        let mut failed_to_provision = false;

        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Normal Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(
                &provision_options,
                &options.bootstrap_peer,
                NodeType::Normal,
            )
            .await
        {
            Ok(()) => {
                println!("Provisioned normal nodes");
            }
            Err(e) => {
                println!("Failed to provision normal nodes: {e:?}");
                failed_to_provision = true;
            }
        }

        self.ansible_provisioner
            .print_ansible_run_banner(n, total, "Provision Private Nodes");
        match self
            .ansible_provisioner
            .provision_nodes(
                &provision_options,
                &options.bootstrap_peer,
                NodeType::Private,
            )
            .await
        {
            Ok(()) => {
                println!("Provisioned private nodes");
            }
            Err(e) => {
                println!("Failed to provision private nodes: {e:?}");
                failed_to_provision = true;
            }
        }

        if failed_to_provision {
            println!("{}", "WARNING!".yellow());
            println!("Some nodes failed to provision without error.");
            println!("This usually means a small number of nodes failed to start on a few VMs.");
            println!("However, most of the time the deployment will still be usable.");
            println!("See the output from Ansible to determine which VMs had failures.");
        }

        Ok(())
    }
}
