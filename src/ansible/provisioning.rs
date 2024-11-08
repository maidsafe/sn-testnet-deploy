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
    funding::FundingOptions,
    inventory::{DeploymentNodeRegistries, VirtualMachine},
    print_duration, BinaryOption, CloudProvider, EvmCustomTestnetData, EvmNetwork, LogFormat,
    NodeType, SshClient, UpgradeOptions,
};
use evmlib::common::U256;
use log::{debug, error, trace};
use semver::Version;
use sn_service_management::NodeRegistry;
use std::{
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

use crate::ansible::extra_vars;

pub const DEFAULT_BETA_ENCRYPTION_KEY: &str =
    "49113d2083f57a976076adbe85decb75115820de1e6e74b47e0429338cef124a";

#[derive(Clone)]
pub struct ProvisionOptions {
    pub binary_option: BinaryOption,
    pub bootstrap_node_count: u16,
    pub chunk_size: Option<u64>,
    pub downloaders_count: u16,
    pub env_variables: Option<Vec<(String, String)>>,
    pub evm_network: EvmNetwork,
    pub funding_wallet_secret_key: Option<String>,
    pub gas_amount: Option<U256>,
    pub interval: Duration,
    pub log_format: Option<LogFormat>,
    pub logstash_details: Option<(String, Vec<SocketAddr>)>,
    pub name: String,
    pub nat_gateway: Option<VirtualMachine>,
    pub node_count: u16,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub output_inventory_dir_path: PathBuf,
    pub private_node_count: u16,
    pub private_node_vms: Vec<VirtualMachine>,
    pub public_rpc: bool,
    /// The safe version is also in the binary option, but only for an initial deployment.
    /// For the upscale, it needs to be provided explicitly, because currently it is not
    /// recorded in the inventory.
    pub safe_version: Option<String>,
    pub uploaders_count: Option<u16>,
    pub rewards_address: String,
}

impl From<BootstrapOptions> for ProvisionOptions {
    fn from(bootstrap_options: BootstrapOptions) -> Self {
        ProvisionOptions {
            binary_option: bootstrap_options.binary_option,
            bootstrap_node_count: 0,
            chunk_size: bootstrap_options.chunk_size,
            downloaders_count: 0,
            env_variables: bootstrap_options.env_variables,
            evm_network: bootstrap_options.evm_network,
            funding_wallet_secret_key: None,
            gas_amount: None,
            interval: bootstrap_options.interval,
            log_format: bootstrap_options.log_format,
            logstash_details: None,
            max_archived_log_files: bootstrap_options.max_archived_log_files,
            max_log_files: bootstrap_options.max_log_files,
            name: bootstrap_options.name,
            nat_gateway: None,
            node_count: bootstrap_options.node_count,
            output_inventory_dir_path: bootstrap_options.output_inventory_dir_path,
            private_node_count: bootstrap_options.private_node_count,
            private_node_vms: Vec::new(),
            public_rpc: false,
            rewards_address: bootstrap_options.rewards_address,
            safe_version: None,
            uploaders_count: None,
        }
    }
}

impl From<DeployOptions> for ProvisionOptions {
    fn from(deploy_options: DeployOptions) -> Self {
        ProvisionOptions {
            binary_option: deploy_options.binary_option,
            bootstrap_node_count: deploy_options.bootstrap_node_count,
            chunk_size: deploy_options.chunk_size,
            downloaders_count: deploy_options.downloaders_count,
            env_variables: deploy_options.env_variables,
            evm_network: deploy_options.evm_network,
            funding_wallet_secret_key: deploy_options.funding_wallet_secret_key,
            gas_amount: None,
            interval: deploy_options.interval,
            log_format: deploy_options.log_format,
            logstash_details: deploy_options.logstash_details,
            name: deploy_options.name,
            nat_gateway: None,
            node_count: deploy_options.node_count,
            max_archived_log_files: deploy_options.max_archived_log_files,
            max_log_files: deploy_options.max_log_files,
            output_inventory_dir_path: deploy_options.output_inventory_dir_path,
            public_rpc: deploy_options.public_rpc,
            private_node_count: deploy_options.private_node_count,
            private_node_vms: Vec::new(),
            safe_version: None,
            uploaders_count: Some(deploy_options.uploaders_count),
            rewards_address: deploy_options.rewards_address,
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

    pub fn build_safe_network_binaries(&self, options: &ProvisionOptions) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for build VM...");
        let build_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, true)?;
        let build_ip = build_inventory[0].public_ip_addr;
        self.ssh_client
            .wait_for_ssh_availability(&build_ip, &self.cloud_provider.get_ssh_user())?;

        println!("Running ansible against build VM...");
        let extra_vars = extra_vars::build_binaries_extra_vars_doc(options)?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Build,
            AnsibleInventoryType::Build,
            Some(extra_vars),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn cleanup_node_logs(&self, setup_cron: bool) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::CleanupLogs,
                node_inv_type,
                Some(format!("{{ \"setup_cron\": \"{setup_cron}\" }}")),
            )?;
        }

        Ok(())
    }

    pub fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::CopyLogs,
                node_inv_type,
                Some(format!(
                    "{{ \"env_name\": \"{name}\", \"resources_only\" : \"{resources_only}\" }}"
                )),
            )?;
        }
        Ok(())
    }

    pub fn get_all_node_inventory(&self) -> Result<Vec<VirtualMachine>> {
        let mut all_node_inventory = Vec::new();
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            all_node_inventory.extend(self.ansible_runner.get_inventory(node_inv_type, false)?);
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

    pub fn provision_evm_nodes(&self, options: &ProvisionOptions) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for EVM nodes...");
        let evm_node_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::EvmNodes, true)?;
        let evm_node_ip = evm_node_inventory[0].public_ip_addr;
        self.ssh_client
            .wait_for_ssh_availability(&evm_node_ip, &self.cloud_provider.get_ssh_user())?;

        println!("Running ansible against EVM nodes...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::EvmNodes,
            AnsibleInventoryType::EvmNodes,
            Some(extra_vars::build_evm_nodes_extra_vars_doc(
                &options.name,
                &self.cloud_provider,
            )),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_genesis_node(
        &self,
        options: &ProvisionOptions,
        evm_testnet_data: Option<EvmCustomTestnetData>,
    ) -> Result<()> {
        let start = Instant::now();
        let genesis_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, true)?;
        let genesis_ip = genesis_inventory[0].public_ip_addr;
        self.ssh_client
            .wait_for_ssh_availability(&genesis_ip, &self.cloud_provider.get_ssh_user())?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Genesis,
            AnsibleInventoryType::Genesis,
            Some(extra_vars::build_node_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                NodeType::Genesis,
                None,
                1,
                options.evm_network.clone(),
                evm_testnet_data,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_nat_gateway(&self, options: &ProvisionOptions) -> Result<()> {
        let start = Instant::now();
        let nat_gateway_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, true)?;
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
            Some(extra_vars::build_nat_gateway_extra_vars_doc(
                &options.name,
                private_ips,
            )),
        )?;

        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_nodes(
        &self,
        options: &ProvisionOptions,
        initial_contact_peer: &str,
        node_type: NodeType,
        evm_testnet_data: Option<EvmCustomTestnetData>,
    ) -> Result<()> {
        let start = Instant::now();
        let (inventory_type, node_count) = match &node_type {
            NodeType::Bootstrap => (
                node_type.to_ansible_inventory_type(),
                options.bootstrap_node_count,
            ),
            NodeType::Generic => (node_type.to_ansible_inventory_type(), options.node_count),
            NodeType::Private => (
                node_type.to_ansible_inventory_type(),
                options.private_node_count,
            ),
            // use provision_genesis_node fn
            NodeType::Genesis => return Err(Error::InvalidNodeType(node_type)),
        };

        // For a new deployment, it's quite probable that SSH is available, because this part occurs
        // after the genesis node has been provisioned. However, for a bootstrap deploy, we need to
        // check that SSH is available before proceeding.
        println!("Obtaining IP addresses for nodes...");
        let inventory = self.ansible_runner.get_inventory(inventory_type, true)?;

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
            Some(extra_vars::build_node_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                node_type,
                Some(initial_contact_peer.to_string()),
                node_count,
                options.evm_network.clone(),
                evm_testnet_data,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_private_nodes(
        &self,
        options: &mut ProvisionOptions,
        initial_contact_peer: &str,
        evm_testnet_data: Option<EvmCustomTestnetData>,
    ) -> Result<()> {
        let start = Instant::now();

        let nat_gateway_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::NatGateway, true)
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

        self.provision_nodes(
            options,
            initial_contact_peer,
            NodeType::Private,
            evm_testnet_data,
        )?;

        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_safenode_rpc_client(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to start safenode_rpc_client service...");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::RpcClient,
            AnsibleInventoryType::Genesis,
            Some(extra_vars::build_safenode_rpc_client_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                genesis_multiaddr,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_uploaders(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: &str,
        evm_testnet_data: Option<EvmCustomTestnetData>,
    ) -> Result<()> {
        let start = Instant::now();

        let sk_map = self
            .deposit_funds_to_uploaders(&FundingOptions {
                custom_evm_testnet_data: evm_testnet_data.clone(),
                uploaders_count: options.uploaders_count,
                evm_network: options.evm_network.clone(),
                funding_wallet_secret_key: options.funding_wallet_secret_key.clone(),
                token_amount: None,
                gas_amount: options.gas_amount,
            })
            .await?;

        println!("Running ansible against uploader machine to start the uploader script.");
        debug!("Running ansible against uploader machine to start the uploader script.");

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Uploaders,
            AnsibleInventoryType::Uploaders,
            Some(extra_vars::build_uploaders_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                genesis_multiaddr,
                evm_testnet_data,
                &sk_map,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn start_nodes(
        &self,
        environment_name: &str,
        interval: Duration,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("interval", &interval.as_millis().to_string());

        if let Some(node_type) = node_type {
            println!("Running the start nodes playbook for {node_type:?} nodes");
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartNodes,
                node_type.to_ansible_inventory_type(),
                Some(extra_vars.build()),
            )?;
            return Ok(());
        }

        if let Some(custom_inventory) = custom_inventory {
            println!("Running the start nodes playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartNodes,
                AnsibleInventoryType::Custom,
                Some(extra_vars.build()),
            )?;
            return Ok(());
        }

        println!("Running the start nodes playbook for all node types");
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartNodes,
                node_inv_type,
                Some(extra_vars.build()),
            )?;
        }
        Ok(())
    }

    pub fn status(&self) -> Result<()> {
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner
                .run_playbook(AnsiblePlaybook::Status, node_inv_type, None)?;
        }
        Ok(())
    }

    pub fn start_telegraf(
        &self,
        environment_name: &str,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        if let Some(node_type) = node_type {
            println!("Running the start telegraf playbook for {node_type:?} nodes");
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartTelegraf,
                node_type.to_ansible_inventory_type(),
                None,
            )?;
            return Ok(());
        }

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

        println!("Running the start telegraf playbook for all node types");
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StartTelegraf,
                node_inv_type,
                None,
            )?;
        }

        Ok(())
    }

    pub fn stop_nodes(
        &self,
        environment_name: &str,
        interval: Duration,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
        delay: Option<u64>,
    ) -> Result<()> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("interval", &interval.as_millis().to_string());
        if let Some(delay) = delay {
            extra_vars.add_variable("delay", &delay.to_string());
        }
        let extra_vars = extra_vars.build();

        if let Some(node_type) = node_type {
            println!("Running the stop nodes playbook for {node_type:?} nodes");
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StopNodes,
                node_type.to_ansible_inventory_type(),
                Some(extra_vars),
            )?;
            return Ok(());
        }

        if let Some(custom_inventory) = custom_inventory {
            println!("Running the stop nodes playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StopNodes,
                AnsibleInventoryType::Custom,
                Some(extra_vars),
            )?;
            return Ok(());
        }

        println!("Running the stop nodes playbook for all node types");
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StopNodes,
                node_inv_type,
                Some(extra_vars.clone()),
            )?;
        }

        Ok(())
    }

    pub fn stop_telegraf(
        &self,
        environment_name: &str,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        if let Some(node_type) = node_type {
            println!("Running the stop telegraf playbook for {node_type:?} nodes");
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::StopTelegraf,
                node_type.to_ansible_inventory_type(),
                None,
            )?;
            return Ok(());
        }

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

        println!("Running the stop telegraf playbook for all node types");
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner
                .run_playbook(AnsiblePlaybook::StopTelegraf, node_inv_type, None)?;
        }

        Ok(())
    }

    pub fn upgrade_node_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::BootstrapNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::Bootstrap,
            )?),
        )?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::Nodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::Generic,
            )?),
        )?;

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::PrivateNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::Private,
            )?),
        )?;
        Ok(())
    }

    pub fn upgrade_uploader_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeUploaderTelegrafConfig,
            AnsibleInventoryType::Uploaders,
            Some(extra_vars::build_uploader_telegraf_upgrade(name)?),
        )?;
        Ok(())
    }

    pub fn upgrade_nodes(&self, options: &UpgradeOptions) -> Result<()> {
        if let Some(custom_inventory) = &options.custom_inventory {
            println!("Running the UpgradeNodes with a custom inventory");
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

        if let Some(node_type) = &options.node_type {
            println!("Running the UpgradeNodes playbook for {node_type:?} nodes");
            match self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNodes,
                node_type.to_ansible_inventory_type(),
                Some(options.get_ansible_vars()),
            ) {
                Ok(()) => println!("All {node_type:?} nodes were successfully upgraded"),
                Err(_) => {
                    println!(
                        "WARNING: some {node_type:?} nodes may not have been upgraded or restarted"
                    );
                }
            }
            return Ok(());
        }

        println!("Running the UpgradeNodes playbook for all node types");

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
        // Don't use AnsibleInventoryType::iter_node_type() here, because the genesis node should be upgraded last
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
        Ok(())
    }

    pub fn upgrade_node_manager(
        &self,
        environment_name: &str,
        version: &Version,
        node_type: Option<NodeType>,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("version", &version.to_string());

        if let Some(node_type) = node_type {
            println!("Running the upgrade safenode-manager playbook for {node_type:?} nodes");
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNodeManager,
                node_type.to_ansible_inventory_type(),
                Some(extra_vars.build()),
            )?;
            return Ok(());
        }

        if let Some(custom_inventory) = custom_inventory {
            println!("Running the upgrade safenode-manager playbook with a custom inventory");
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

        println!("Running the upgrade safenode-manager playbook for all node types");
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
}
