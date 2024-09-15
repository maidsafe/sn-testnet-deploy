// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{
    extra_vars::ExtraVarsDocBuilder, AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner,
};
use crate::{
    ansible::generate_custom_environment_inventory,
    bootstrap::BootstrapOptions,
    deploy::DeployOptions,
    error::{Error, Result},
    inventory::{DeploymentNodeRegistries, VirtualMachine},
    print_duration, BinaryOption, CloudProvider, LogFormat, SshClient, UpgradeOptions,
};
use log::{debug, trace};
use semver::Version;
use sn_service_management::NodeRegistry;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Instant,
};
use walkdir::WalkDir;

const DEFAULT_BETA_ENCRYPTION_KEY: &str =
    "49113d2083f57a976076adbe85decb75115820de1e6e74b47e0429338cef124a";

pub enum NodeType {
    Bootstrap,
    Normal,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Bootstrap => write!(f, "bootstrap_node"),
            NodeType::Normal => write!(f, "generic_node"),
        }
    }
}

pub struct ProvisionOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub node_count: u16,
    pub public_rpc: bool,
}

impl From<BootstrapOptions> for ProvisionOptions {
    fn from(bootstrap_options: BootstrapOptions) -> Self {
        ProvisionOptions {
            beta_encryption_key: None,
            binary_option: bootstrap_options.binary_option,
            bootstrap_node_count: 0,
            env_variables: bootstrap_options.env_variables,
            log_format: bootstrap_options.log_format,
            logstash_details: None,
            name: bootstrap_options.name,
            node_count: bootstrap_options.node_count,
            public_rpc: false,
        }
    }
}

impl From<DeployOptions> for ProvisionOptions {
    fn from(deploy_options: DeployOptions) -> Self {
        ProvisionOptions {
            beta_encryption_key: deploy_options.beta_encryption_key,
            binary_option: deploy_options.binary_option,
            bootstrap_node_count: deploy_options.bootstrap_node_count,
            env_variables: deploy_options.env_variables,
            log_format: deploy_options.log_format,
            logstash_details: deploy_options.logstash_details,
            name: deploy_options.name,
            node_count: deploy_options.node_count,
            public_rpc: deploy_options.public_rpc,
        }
    }
}

#[derive(Clone)]
pub struct AnsibleProvisioner {
    pub ansible_runner: AnsibleRunner,
    pub cloud_provider: CloudProvider,
    pub ssh_client: SshClient,
}

impl AnsibleProvisioner {
    pub fn new(
        ansible_runner: AnsibleRunner,
        cloud_provider: CloudProvider,
        ssh_client: SshClient,
    ) -> Self {
        Self {
            ansible_runner,
            cloud_provider,
            ssh_client,
        }
    }

    pub async fn build_safe_network_binaries(&self, options: &ProvisionOptions) -> Result<()> {
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

    pub async fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Logs,
            AnsibleInventoryType::Genesis,
            Some(format!(
                "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
            )),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Logs,
            AnsibleInventoryType::BootstrapNodes,
            Some(format!(
                "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
            )),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Logs,
            AnsibleInventoryType::Nodes,
            Some(format!(
                "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
            )),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Logs,
            AnsibleInventoryType::Auditor,
            Some(format!(
                "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
            )),
        )?;
        Ok(())
    }

    pub async fn get_all_node_inventory(&self) -> Result<Vec<(String, IpAddr)>> {
        let mut all_node_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, false)
            .await?;
        all_node_inventory.extend(
            self.ansible_runner
                .get_inventory(AnsibleInventoryType::BootstrapNodes, false)
                .await?,
        );
        all_node_inventory.extend(
            self.ansible_runner
                .get_inventory(AnsibleInventoryType::Nodes, false)
                .await?,
        );
        Ok(all_node_inventory)
    }

    pub fn get_node_registries(
        &self,
        inventory_type: &AnsibleInventoryType,
    ) -> Result<DeploymentNodeRegistries> {
        debug!("Fetching node manager inventory");
        let temp_dir_path = tempfile::tempdir()?.into_path();
        let temp_dir_json = serde_json::to_string(&temp_dir_path)?;

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::NodeManagerInventory,
            inventory_type.clone(),
            Some(format!("{{ \"dest\": {temp_dir_json} }}")),
        )?;

        let node_registry_paths = WalkDir::new(temp_dir_path)
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
                    Some((vm_name.to_string(), entry.path().to_path_buf()))
                } else {
                    None
                }
            })
            .collect::<Vec<(String, PathBuf)>>();

        let mut node_registries = Vec::new();
        let mut failed_vms = Vec::new();
        for (vm_name, file_path) in node_registry_paths {
            match NodeRegistry::load(&file_path) {
                Ok(node_registry) => node_registries.push((vm_name.clone(), node_registry)),
                Err(_) => failed_vms.push(vm_name.clone()),
            }
        }

        let deployment_registries = DeploymentNodeRegistries {
            inventory_type: inventory_type.clone(),
            retrieved_registries: node_registries,
            failed_vms,
        };
        Ok(deployment_registries)
    }

    pub async fn provision_genesis_node(&self, options: &ProvisionOptions) -> Result<()> {
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
            Some(self.build_node_extra_vars_doc(options, NodeType::Bootstrap, None, 1)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_nodes(
        &self,
        options: &ProvisionOptions,
        initial_contact_peer: &str,
        node_type: NodeType,
    ) -> Result<()> {
        let start = Instant::now();
        let (inventory_type, node_count) = match node_type {
            NodeType::Bootstrap => (
                AnsibleInventoryType::BootstrapNodes,
                options.bootstrap_node_count,
            ),
            NodeType::Normal => (AnsibleInventoryType::Nodes, options.node_count),
        };

        // For a new deployment, it's quite probable that SSH is available, because this part occurs
        // after the genesis node has been provisioned. However, for a bootstrap deploy, we need to
        // check that SSH is available before proceeding.
        println!("Obtaining IP addresses for nodes...");
        let inventory = self
            .ansible_runner
            .get_inventory(inventory_type.clone(), true)
            .await?;

        println!("Waiting for SSH availability on {} nodes...", node_type);
        for (vm_name, ip) in inventory.iter() {
            println!("Checking SSH availability for {}: {}", vm_name, ip);
            self.ssh_client
                .wait_for_ssh_availability(ip, &self.cloud_provider.get_ssh_user())
                .map_err(|e| {
                    println!("Failed to establish SSH connection to {}: {}", vm_name, e);
                    e
                })?;
        }

        println!("SSH is available on all nodes. Proceeding with provisioning...");

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Nodes,
            inventory_type,
            Some(self.build_node_extra_vars_doc(
                options,
                node_type,
                Some(initial_contact_peer.to_string()),
                node_count,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    /// Provision the faucet service on the genesis node and start it.
    pub async fn provision_and_start_faucet(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy and start faucet...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Faucet,
            AnsibleInventoryType::Genesis,
            Some(self.build_faucet_extra_vars_doc(options, genesis_multiaddr, false)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    /// Stop the faucet service on the genesis node. If the faucet is not provisioned, this will also provision it.
    pub async fn provision_and_stop_faucet(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node stop the faucet...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Faucet,
            AnsibleInventoryType::Genesis,
            Some(self.build_faucet_extra_vars_doc(options, genesis_multiaddr, true)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_safenode_rpc_client(
        &self,
        options: &ProvisionOptions,
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

    pub async fn provision_sn_auditor(
        &self,
        options: &ProvisionOptions,
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

    pub async fn provision_uploaders(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
        genesis_ip: &IpAddr,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against uploader machine to start the uploader script.");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Uploaders,
            AnsibleInventoryType::Uploaders,
            Some(self.build_uploaders_extra_vars_doc(options, genesis_multiaddr, genesis_ip)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn start_nodes(&self) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartNodes,
            AnsibleInventoryType::Genesis,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartNodes,
            AnsibleInventoryType::BootstrapNodes,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartNodes,
            AnsibleInventoryType::Nodes,
            None,
        )?;
        Ok(())
    }

    pub async fn status(&self) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Status,
            AnsibleInventoryType::Genesis,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Status,
            AnsibleInventoryType::BootstrapNodes,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Status,
            AnsibleInventoryType::Nodes,
            None,
        )?;
        Ok(())
    }

    pub async fn start_telegraf(
        &self,
        environment_name: &str,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        if let Some(custom_inventory) = custom_inventory {
            println!("Running the start telegraf playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartTelegraf,
                AnsibleInventoryType::Custom,
                None,
            )?;
            return Ok(());
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartTelegraf,
            AnsibleInventoryType::Genesis,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartTelegraf,
            AnsibleInventoryType::BootstrapNodes,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StartTelegraf,
            AnsibleInventoryType::Nodes,
            None,
        )?;
        Ok(())
    }

    pub async fn stop_telegraf(
        &self,
        environment_name: &str,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        if let Some(custom_inventory) = custom_inventory {
            println!("Running the stop telegraf playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StopTelegraf,
                AnsibleInventoryType::Custom,
                None,
            )?;
            return Ok(());
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StopTelegraf,
            AnsibleInventoryType::Genesis,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StopTelegraf,
            AnsibleInventoryType::BootstrapNodes,
            None,
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StopTelegraf,
            AnsibleInventoryType::Nodes,
            None,
        )?;
        Ok(())
    }

    pub async fn upgrade_node_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::BootstrapNodes,
            Some(self.build_node_telegraf_upgrade(name, &NodeType::Bootstrap)?),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::Nodes,
            Some(self.build_node_telegraf_upgrade(name, &NodeType::Normal)?),
        )?;
        Ok(())
    }

    pub async fn upgrade_uploader_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeUploaderTelegrafConfig,
            AnsibleInventoryType::Uploaders,
            Some(self.build_uploader_telegraf_upgrade(name)?),
        )?;
        Ok(())
    }

    pub async fn upgrade_nodes(&self, options: &UpgradeOptions) -> Result<()> {
        if let Some(custom_inventory) = &options.custom_inventory {
            println!("Running the upgrade with a custom inventory");
            generate_custom_environment_inventory(
                custom_inventory,
                &options.name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            match self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNodes,
                AnsibleInventoryType::Custom,
                Some(options.get_ansible_vars()),
            ) {
                Ok(()) => println!("All nodes were successfully upgraded"),
                Err(_) => {
                    println!("WARNING: some nodes may not have been upgraded or restarted");
                }
            }
            return Ok(());
        }

        match self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodes,
            AnsibleInventoryType::BootstrapNodes,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("All bootstrap nodes were successfully upgraded"),
            Err(_) => {
                println!("WARNING: some bootstrap nodes may not have been upgraded or restarted");
            }
        }
        match self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodes,
            AnsibleInventoryType::Nodes,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("All generic nodes were successfully upgraded"),
            Err(_) => {
                println!("WARNING: some nodes may not have been upgraded or restarted");
            }
        }
        match self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodes,
            AnsibleInventoryType::Genesis,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("The genesis nodes was successfully upgraded"),
            Err(_) => {
                println!("WARNING: the genesis node may not have been upgraded or restarted");
            }
        }
        match self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeFaucet,
            AnsibleInventoryType::Genesis,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("The faucet was successfully upgraded"),
            Err(_) => {
                println!("WARNING: the faucet may not have been upgraded or restarted");
            }
        }
        Ok(())
    }

    pub async fn upgrade_node_manager(
        &self,
        environment_name: &str,
        version: &Version,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("version", &version.to_string());

        if let Some(custom_inventory) = custom_inventory {
            println!("Running the upgrade node manager playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNodeManager,
                AnsibleInventoryType::Custom,
                Some(extra_vars.build()),
            )?;
            return Ok(());
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeManager,
            AnsibleInventoryType::Genesis,
            Some(extra_vars.build()),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeManager,
            AnsibleInventoryType::BootstrapNodes,
            Some(extra_vars.build()),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeManager,
            AnsibleInventoryType::Nodes,
            Some(extra_vars.build()),
        )?;
        Ok(())
    }

    pub fn print_ansible_run_banner(&self, n: usize, total: usize, s: &str) {
        let ansible_run_msg = format!("Ansible Run {} of {}: ", n, total);
        let line = "=".repeat(s.len() + ansible_run_msg.len());
        println!("{}\n{}{}\n{}", line, ansible_run_msg, s, line);
    }

    fn build_node_extra_vars_doc(
        &self,
        options: &ProvisionOptions,
        node_type: NodeType,
        bootstrap_multiaddr: Option<String>,
        node_instance_count: u16,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("node_type", &node_type.to_string());
        if bootstrap_multiaddr.is_some() {
            extra_vars.add_variable(
                "genesis_multiaddr",
                &bootstrap_multiaddr.ok_or_else(|| Error::GenesisMultiAddrNotSupplied)?,
            );
        }

        extra_vars.add_variable("node_instance_count", &node_instance_count.to_string());
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

    /// If the `stop` flag is set to true, the playbook will stop the faucet service.
    /// Otherwise, it will start the faucet service.
    fn build_faucet_extra_vars_doc(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
        stop: bool,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        if stop {
            extra_vars.add_variable("action", "stop");
        } else {
            extra_vars.add_variable("action", "start");
        }
        extra_vars.add_node_manager_url(&options.name, &options.binary_option);
        extra_vars.add_faucet_url_or_version(&options.name, &options.binary_option)?;
        Ok(extra_vars.build())
    }

    fn build_safenode_rpc_client_extra_vars_doc(
        &self,
        options: &ProvisionOptions,
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
        options: &ProvisionOptions,
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
        extra_vars.add_sn_auditor_url_or_version(&options.name, &options.binary_option)?;
        Ok(extra_vars.build())
    }

    fn build_uploaders_extra_vars_doc(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
        genesis_ip: &IpAddr,
    ) -> Result<String> {
        let faucet_address = format!("{genesis_ip}:8000");
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &options.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_variable("faucet_address", &faucet_address);
        extra_vars.add_safe_url_or_version(&options.name, &options.binary_option)?;
        Ok(extra_vars.build())
    }

    fn build_binaries_extra_vars_doc(&self, options: &ProvisionOptions) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_build_variables(&options.name, &options.binary_option);
        Ok(extra_vars.build())
    }

    fn build_node_telegraf_upgrade(&self, name: &str, node_type: &NodeType) -> Result<String> {
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("testnet_name", name);
        extra_vars.add_variable("node_type", &node_type.to_string());
        Ok(extra_vars.build())
    }

    fn build_uploader_telegraf_upgrade(&self, name: &str) -> Result<String> {
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("testnet_name", name);
        Ok(extra_vars.build())
    }
}
