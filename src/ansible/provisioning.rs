// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{
    extra_vars::ExtraVarsDocBuilder, inventory::generate_private_node_static_environment_inventory,
    AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner,
};
use crate::{
    ansible::inventory::generate_custom_environment_inventory,
    bootstrap::BootstrapOptions,
    deploy::DeployOptions,
    error::{Error, Result},
    inventory::{DeploymentNodeRegistries, VirtualMachine},
    print_duration, BinaryOption, CloudProvider, LogFormat, SshClient, UpgradeOptions,
};
use log::{debug, error, trace};
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

#[derive(Debug)]
pub enum NodeType {
    Bootstrap,
    Normal,
    Private,
}

impl NodeType {
    fn telegraph_role(&self) -> &'static str {
        match self {
            NodeType::Bootstrap => "BOOTSTRAP_NODE",
            NodeType::Normal => "GENERIC_NODE",
            NodeType::Private => "NAT_RANDOMIZED_NODE",
        }
    }
}

#[derive(Clone)]
pub struct ProvisionOptions {
    pub beta_encryption_key: Option<String>,
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub chunk_size: Option<u64>,
    pub downloaders_count: u16,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub nat_gateway: Option<VirtualMachine>,
    pub node_count: u16,
    pub output_inventory_dir_path: PathBuf,
    pub private_node_count: u16,
    pub private_node_vms: Vec<VirtualMachine>,
    pub public_rpc: bool,
    /// The safe version is also in the binary option, but only for an initial deployment.
    /// For the upscale, it needs to be provided explicitly, because currently it is not
    /// recorded in the inventory.
    pub safe_version: Option<String>,
}

impl From<BootstrapOptions> for ProvisionOptions {
    fn from(bootstrap_options: BootstrapOptions) -> Self {
        ProvisionOptions {
            beta_encryption_key: None,
            binary_option: bootstrap_options.binary_option,
            bootstrap_node_count: 0,
            chunk_size: None,
            downloaders_count: 0,
            env_variables: bootstrap_options.env_variables,
            log_format: bootstrap_options.log_format,
            logstash_details: None,
            name: bootstrap_options.name,
            nat_gateway: None,
            node_count: bootstrap_options.node_count,
            output_inventory_dir_path: bootstrap_options.output_inventory_dir_path,
            private_node_count: bootstrap_options.private_node_count,
            private_node_vms: Vec::new(),
            public_rpc: false,
            safe_version: None,
        }
    }
}

impl From<DeployOptions> for ProvisionOptions {
    fn from(deploy_options: DeployOptions) -> Self {
        ProvisionOptions {
            beta_encryption_key: deploy_options.beta_encryption_key,
            binary_option: deploy_options.binary_option,
            bootstrap_node_count: deploy_options.bootstrap_node_count,
            chunk_size: deploy_options.chunk_size,
            downloaders_count: deploy_options.downloaders_count,
            env_variables: deploy_options.env_variables,
            log_format: deploy_options.log_format,
            logstash_details: deploy_options.logstash_details,
            name: deploy_options.name,
            nat_gateway: None,
            node_count: deploy_options.node_count,
            output_inventory_dir_path: deploy_options.output_inventory_dir_path,
            public_rpc: deploy_options.public_rpc,
            private_node_count: deploy_options.private_node_count,
            private_node_vms: Vec::new(),
            safe_version: None,
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
        let build_ip = build_inventory[0].public_ip_addr;
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

    pub async fn cleanup_node_logs(&self, setup_cron: bool) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::CleanupLogs,
                node_inv_type,
                Some(format!("{{ \"setup_cron\": \"{setup_cron}\" }}")),
            )?;
        }

        Ok(())
    }

    pub async fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::CopyLogs,
                node_inv_type,
                Some(format!(
                    "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
                )),
            )?;
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::CopyLogs,
            AnsibleInventoryType::Auditor,
            Some(format!(
                "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
            )),
        )?;
        Ok(())
    }

    pub async fn get_all_node_inventory(&self) -> Result<Vec<VirtualMachine>> {
        let mut all_node_inventory = Vec::new();
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            all_node_inventory.extend(
                self.ansible_runner
                    .get_inventory(node_inv_type, false)
                    .await?,
            );
        }

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
            *inventory_type,
            Some(format!("{{ \"dest\": {temp_dir_json} }}")),
        )?;

        let node_registry_paths = WalkDir::new(temp_dir_path)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                if entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|ext| ext == "json")
                {
                    trace!("Found file with json extension: {:?}", entry.path());
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
            inventory_type: *inventory_type,
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
        let genesis_ip = genesis_inventory[0].public_ip_addr;
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

    pub async fn provision_nat_gateway(&self, options: &ProvisionOptions) -> Result<()> {
        let start = Instant::now();
        let nat_gateway_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, true)
            .await?;
        let nat_gateway_ip = nat_gateway_inventory[0].public_ip_addr;
        self.ssh_client
            .wait_for_ssh_availability(&nat_gateway_ip, &self.cloud_provider.get_ssh_user())?;
        let private_ips = options
            .private_node_vms
            .iter()
            .map(|vm| vm.private_ip_addr.to_string())
            .collect::<Vec<_>>();

        if private_ips.is_empty() {
            println!("There are no private node VM available to be routed through the NAT Gateway");
            return Err(Error::EmptyInventory(AnsibleInventoryType::PrivateNodes));
        }

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::NatGateway,
            AnsibleInventoryType::NatGateway,
            Some(self.build_nat_gateway_extra_vars_doc(&options.name, private_ips)),
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
            NodeType::Private => (
                AnsibleInventoryType::PrivateNodes,
                options.private_node_count,
            ),
        };

        // For a new deployment, it's quite probable that SSH is available, because this part occurs
        // after the genesis node has been provisioned. However, for a bootstrap deploy, we need to
        // check that SSH is available before proceeding.
        println!("Obtaining IP addresses for nodes...");
        let inventory = self
            .ansible_runner
            .get_inventory(inventory_type, true)
            .await?;

        println!("Waiting for SSH availability on {node_type:?} nodes...");
        for vm in inventory.iter() {
            println!(
                "Checking SSH availability for {}: {}",
                vm.name, vm.public_ip_addr
            );
            self.ssh_client
                .wait_for_ssh_availability(&vm.public_ip_addr, &self.cloud_provider.get_ssh_user())
                .map_err(|e| {
                    println!("Failed to establish SSH connection to {}: {}", vm.name, e);
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

    pub async fn provision_private_nodes(
        &self,
        options: &mut ProvisionOptions,
        initial_contact_peer: &str,
    ) -> Result<()> {
        let start = Instant::now();

        let nat_gateway_inventory = self
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

        options.nat_gateway = Some(nat_gateway_inventory.clone());
        generate_private_node_static_environment_inventory(
            &options.name,
            &options.output_inventory_dir_path,
            &options.private_node_vms,
            &Some(nat_gateway_inventory),
            &self.ssh_client.private_key_path,
        )
        .inspect_err(|err| {
            error!("Failed to generate private node static inv with err: {err:?}")
        })?;

        self.provision_nodes(options, initial_contact_peer, NodeType::Private)
            .await?;

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
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner
                .run_playbook(AnsiblePlaybook::StartNodes, node_inv_type, None)?;
        }

        Ok(())
    }

    pub async fn status(&self) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner
                .run_playbook(AnsiblePlaybook::Status, node_inv_type, None)?;
        }
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

        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartTelegraf,
                node_inv_type,
                None,
            )?;
        }

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

        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner
                .run_playbook(AnsiblePlaybook::StopTelegraf, node_inv_type, None)?;
        }

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

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::PrivateNodes,
            Some(self.build_node_telegraf_upgrade(name, &NodeType::Private)?),
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
            AnsibleInventoryType::PrivateNodes,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("All private nodes were successfully upgraded"),
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

        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNodeManager,
                node_inv_type,
                Some(extra_vars.build()),
            )?;
        }

        Ok(())
    }

    pub fn print_ansible_run_banner(&self, n: usize, total: usize, s: &str) {
        let ansible_run_msg = format!("Ansible Run {} of {}: ", n, total);
        let line = "=".repeat(s.len() + ansible_run_msg.len());
        println!("{}\n{}{}\n{}", line, ansible_run_msg, s, line);
    }

    fn build_nat_gateway_extra_vars_doc(&self, name: &str, private_ips: Vec<String>) -> String {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("testnet_name", name);
        extra_vars.add_list_variable("node_private_ips_eth1", private_ips);
        extra_vars.build()
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
        extra_vars.add_variable("node_type", node_type.telegraph_role());
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

        if let Some(nat_gateway) = &options.nat_gateway {
            extra_vars.add_variable(
                "nat_gateway_private_ip_eth1",
                &nat_gateway.private_ip_addr.to_string(),
            );
            extra_vars.add_variable("make_vm_private", "true");
        } else if matches!(node_type, NodeType::Private) {
            return Err(Error::NatGatewayNotSupplied);
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
        extra_vars.add_variable(
            "safe_downloader_instances",
            &options.downloaders_count.to_string(),
        );
        extra_vars.add_safe_url_or_version(
            &options.name,
            &options.binary_option,
            options.safe_version.clone(),
        )?;
        Ok(extra_vars.build())
    }

    fn build_binaries_extra_vars_doc(&self, options: &ProvisionOptions) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_build_variables(&options.name, &options.binary_option);
        if let Some(chunk_size) = options.chunk_size {
            extra_vars.add_variable("chunk_size", &chunk_size.to_string());
        }
        Ok(extra_vars.build())
    }

    fn build_node_telegraf_upgrade(&self, name: &str, node_type: &NodeType) -> Result<String> {
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("testnet_name", name);
        extra_vars.add_variable("node_type", node_type.telegraph_role());
        Ok(extra_vars.build())
    }

    fn build_uploader_telegraf_upgrade(&self, name: &str) -> Result<String> {
        let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("testnet_name", name);
        Ok(extra_vars.build())
    }
}
