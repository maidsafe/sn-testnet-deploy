// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{AnsibleInventoryType, AnsiblePlaybook, ExtraVarsDocBuilder},
    error::{Error, Result},
    get_genesis_multiaddr, print_duration, BinaryOption, DeploymentInventory, LogFormat,
    TestnetDeployer,
};
use colored::Colorize;
use std::{net::SocketAddr, time::Instant};

const DEFAULT_BETA_ENCRYPTION_KEY: &str =
    "49113d2083f57a976076adbe85decb75115820de1e6e74b47e0429338cef124a";

pub struct DeployOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_vm_count: u16,
    pub current_inventory: DeploymentInventory,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub node_count: u16,
    pub node_vm_count: u16,
    pub public_rpc: bool,
    pub uploader_vm_count: u16,
}

enum NodeType {
    Bootstrap,
    Normal,
}

impl TestnetDeployer {
    pub async fn deploy(&self, options: &DeployOptions) -> Result<()> {
        let build_custom_binaries = {
            match &options.binary_option {
                BinaryOption::BuildFromSource { .. } => true,
                BinaryOption::Versioned { .. } => false,
            }
        };

        self.create_infra(options, build_custom_binaries)
            .await
            .map_err(|err| {
                println!("Failed to create infra {err:?}");
                err
            })?;

        let mut n = 1;
        let mut total = if build_custom_binaries { 7 } else { 6 };
        if !options.current_inventory.is_empty() {
            total -= 3;
        }

        if build_custom_binaries {
            self.print_ansible_run_banner(n, total, "Build Custom Binaries");
            self.build_safe_network_binaries(options)
                .await
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
            n += 1;
        }

        self.print_ansible_run_banner(n, total, "Provision Genesis Node");
        self.provision_genesis_node(options).await.map_err(|err| {
            println!("Failed to provision genesis node {err:?}");
            err
        })?;
        n += 1;
        let (genesis_multiaddr, _) = get_genesis_multiaddr(&self.ansible_runner, &self.ssh_client)
            .await
            .map_err(|err| {
                println!("Failed to get genesis multiaddr {err:?}");
                err
            })?;
        println!("Obtained multiaddr for genesis node: {genesis_multiaddr}");

        let mut node_provision_failed = false;
        self.print_ansible_run_banner(n, total, "Provision Bootstrap Nodes");
        match self
            .provision_nodes(options, &genesis_multiaddr, NodeType::Bootstrap)
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

        self.print_ansible_run_banner(n, total, "Provision Normal Nodes");
        match self
            .provision_nodes(options, &genesis_multiaddr, NodeType::Normal)
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

        if options.current_inventory.is_empty() {
            // These steps are only necessary on the initial deploy, at which point the inventory
            // will be empty.
            self.print_ansible_run_banner(n, total, "Deploy Faucet");
            self.provision_faucet(options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision faucet {err:?}");
                    err
                })?;
            n += 1;
            self.print_ansible_run_banner(n, total, "Provision RPC Client on Genesis Node");
            self.provision_safenode_rpc_client(options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision safenode rpc client {err:?}");
                    err
                })?;
            n += 1;
            self.print_ansible_run_banner(n, total, "Provision Auditor");
            self.provision_sn_auditor(options, &genesis_multiaddr)
                .await
                .map_err(|err| {
                    println!("Failed to provision sn_auditor {err:?}");
                    err
                })?;
        }

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

    async fn create_infra(&self, options: &DeployOptions, enable_build_vm: bool) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;
        let args = vec![
            (
                "bootstrap_node_vm_count".to_string(),
                options.bootstrap_node_vm_count.to_string(),
            ),
            (
                "node_vm_count".to_string(),
                options.node_vm_count.to_string(),
            ),
            (
                "uploader_vm_count".to_string(),
                options.uploader_vm_count.to_string(),
            ),
            ("use_custom_bin".to_string(), enable_build_vm.to_string()),
        ];
        println!("Running terraform apply...");
        self.terraform_runner.apply(args)?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn provision_genesis_node(&self, options: &DeployOptions) -> Result<()> {
        let start = Instant::now();
        let genesis_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, true)
            .await?;
        let genesis_ip = genesis_inventory[0].1;
        self.ssh_client
            .wait_for_ssh_availability(&genesis_ip, &self.cloud_provider.get_ssh_user())?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Genesis,
            AnsibleInventoryType::Genesis,
            Some(self.build_node_extra_vars_doc(options, None, None)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn provision_nodes(
        &self,
        options: &DeployOptions,
        initial_contact_peer: &str,
        node_type: NodeType,
    ) -> Result<()> {
        let start = Instant::now();
        let inventory_type = match node_type {
            NodeType::Bootstrap => AnsibleInventoryType::BootstrapNodes,
            NodeType::Normal => AnsibleInventoryType::Nodes,
        };

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Nodes,
            inventory_type,
            Some(self.build_node_extra_vars_doc(
                options,
                Some(initial_contact_peer.to_string()),
                Some(options.node_count),
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    ///
    /// Helpers
    ///
    fn print_ansible_run_banner(&self, n: usize, total: usize, s: &str) {
        let ansible_run_msg = format!("Ansible Run {} of {}: ", n, total);
        let line = "=".repeat(s.len() + ansible_run_msg.len());
        println!("{}\n{}{}\n{}", line, ansible_run_msg, s, line);
    }

    fn build_binaries_extra_vars_doc(&self, options: &DeployOptions) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_build_variables(&options.name, &options.binary_option);
        Ok(extra_vars.build())
    }

    fn build_node_extra_vars_doc(
        &self,
        options: &DeployOptions,
        bootstrap_node: Option<String>,
        node_instance_count: Option<u16>,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        if bootstrap_node.is_some() {
            extra_vars.add_variable(
                "genesis_multiaddr",
                &bootstrap_node.ok_or_else(|| Error::GenesisMultiAddrNotSupplied)?,
            );
        }
        if node_instance_count.is_some() {
            extra_vars.add_variable(
                "node_instance_count",
                &node_instance_count.unwrap_or(20).to_string(),
            );
        }
        if let Some(log_format) = options.log_format {
            extra_vars.add_variable("log_format", log_format.as_str());
        }
        if options.public_rpc {
            extra_vars.add_variable("public_rpc", "true");
        }

        extra_vars.add_node_url_or_version(&options.name, &options.binary_option);
        extra_vars.add_node_manager_url(&options.name, &options.binary_option);
        extra_vars.add_node_manager_daemon_url(&options.name, &options.binary_option);

        if let Some(env_vars) = &options.env_variables {
            extra_vars.add_env_variable_list("env_variables", env_vars.clone());
        }

        if let Some((logstash_stack_name, logstash_hosts)) = &options.logstash_details {
            extra_vars.add_variable("logstash_stack_name", logstash_stack_name);
            extra_vars.add_list_variable(
                "logstash_hosts",
                logstash_hosts
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            );
        }

        Ok(extra_vars.build())
    }

    async fn provision_faucet(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy faucet...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Faucet,
            AnsibleInventoryType::Genesis,
            Some(self.build_faucet_extra_vars_doc(options, genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn provision_safenode_rpc_client(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to start safenode_rpc_client service...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::RpcClient,
            AnsibleInventoryType::Genesis,
            Some(self.build_safenode_rpc_client_extra_vars_doc(options, genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn provision_sn_auditor(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against auditor machine to start sn_auditor service...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Auditor,
            AnsibleInventoryType::Auditor,
            Some(self.build_sn_auditor_extra_vars_doc(options, genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn build_safe_network_binaries(&self, options: &DeployOptions) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for build VM...");
        let build_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, true)
            .await?;
        let build_ip = build_inventory[0].1;
        self.ssh_client
            .wait_for_ssh_availability(&build_ip, &self.cloud_provider.get_ssh_user())?;

        println!("Running ansible against build VM...");
        let extra_vars = self.build_binaries_extra_vars_doc(options)?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Build,
            AnsibleInventoryType::Build,
            Some(extra_vars),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    fn build_faucet_extra_vars_doc(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_node_manager_url(&options.name, &options.binary_option);
        extra_vars.add_faucet_url_or_version(&options.name, &options.binary_option);
        Ok(extra_vars.build())
    }

    fn build_safenode_rpc_client_extra_vars_doc(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_rpc_client_url_or_version(&options.name, &options.binary_option);
        Ok(extra_vars.build())
    }

    fn build_sn_auditor_extra_vars_doc(
        &self,
        options: &DeployOptions,
        genesis_multiaddr: &str,
    ) -> Result<String> {
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_variable(
            "beta_encryption_key",
            options
                .beta_encryption_key
                .as_ref()
                .unwrap_or(&DEFAULT_BETA_ENCRYPTION_KEY.to_string()),
        );
        extra_vars.add_node_manager_url(&options.name, &options.binary_option);
        extra_vars.add_sn_auditor_url_or_version(&options.name, &options.binary_option);
        Ok(extra_vars.build())
    }
}
