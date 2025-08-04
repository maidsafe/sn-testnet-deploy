// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{
    extra_vars::ExtraVarsDocBuilder,
    inventory::{
        generate_full_cone_private_node_static_environment_inventory,
        generate_symmetric_private_node_static_environment_inventory,
    },
    AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner,
};
use crate::{
    ansible::inventory::{
        generate_custom_environment_inventory,
        generate_full_cone_nat_gateway_static_environment_inventory,
    },
    bootstrap::BootstrapOptions,
    clients::ClientsDeployOptions,
    deploy::DeployOptions,
    error::{Error, Result},
    funding::FundingOptions,
    inventory::{DeploymentNodeRegistries, VirtualMachine},
    print_duration, run_external_command, BinaryOption, CloudProvider, EvmNetwork, LogFormat,
    NodeType, SshClient, UpgradeOptions,
};
use ant_service_management::NodeRegistry;
use evmlib::common::U256;
use log::{debug, error, trace};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    path::PathBuf,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

use crate::ansible::extra_vars;

pub const DEFAULT_BETA_ENCRYPTION_KEY: &str =
    "49113d2083f57a976076adbe85decb75115820de1e6e74b47e0429338cef124a";

#[derive(Clone, Serialize, Deserialize)]
pub struct ProvisionOptions {
    /// The safe version is also in the binary option, but only for an initial deployment.
    /// For the upscale, it needs to be provided explicitly, because currently it is not
    /// recorded in the inventory.
    pub ant_version: Option<String>,
    pub binary_option: BinaryOption,
    pub chunk_size: Option<u64>,
    pub client_env_variables: Option<Vec<(String, String)>>,
    pub enable_download_verifier: bool,
    pub enable_random_verifier: bool,
    pub enable_performance_verifier: bool,
    pub enable_telegraf: bool,
    pub enable_uploaders: bool,
    pub evm_data_payments_address: Option<String>,
    pub evm_network: EvmNetwork,
    pub evm_payment_token_address: Option<String>,
    pub evm_rpc_url: Option<String>,
    pub expected_hash: Option<String>,
    pub expected_size: Option<u64>,
    pub file_address: Option<String>,
    pub full_cone_private_node_count: u16,
    pub funding_wallet_secret_key: Option<String>,
    pub gas_amount: Option<U256>,
    pub interval: Option<Duration>,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub max_uploads: Option<u32>,
    pub name: String,
    pub network_id: Option<u8>,
    pub network_dashboard_branch: Option<String>,
    pub node_count: u16,
    pub node_env_variables: Option<Vec<(String, String)>>,
    pub output_inventory_dir_path: PathBuf,
    pub peer_cache_node_count: u16,
    pub public_rpc: bool,
    pub rewards_address: Option<String>,
    pub symmetric_private_node_count: u16,
    pub token_amount: Option<U256>,
    pub upload_size: Option<u16>,
    pub upload_interval: Option<u16>,
    pub uploaders_count: Option<u16>,
    pub wallet_secret_keys: Option<Vec<String>>,
}

/// These are obtained by running the inventory playbook
#[derive(Clone, Debug)]
pub struct PrivateNodeProvisionInventory {
    pub full_cone_nat_gateway_vms: Vec<VirtualMachine>,
    pub full_cone_private_node_vms: Vec<VirtualMachine>,
    pub symmetric_nat_gateway_vms: Vec<VirtualMachine>,
    pub symmetric_private_node_vms: Vec<VirtualMachine>,
}

impl PrivateNodeProvisionInventory {
    pub fn new(
        provisioner: &AnsibleProvisioner,
        full_cone_private_node_vm_count: Option<u16>,
        symmetric_private_node_vm_count: Option<u16>,
    ) -> Result<Self> {
        // All the environment types set private_node_vm count to >0 if not specified.
        let should_provision_full_cone_private_nodes = full_cone_private_node_vm_count
            .map(|count| count > 0)
            .unwrap_or(true);
        let should_provision_symmetric_private_nodes = symmetric_private_node_vm_count
            .map(|count| count > 0)
            .unwrap_or(true);

        let mut inventory = Self {
            full_cone_nat_gateway_vms: Default::default(),
            full_cone_private_node_vms: Default::default(),
            symmetric_nat_gateway_vms: Default::default(),
            symmetric_private_node_vms: Default::default(),
        };

        if should_provision_full_cone_private_nodes {
            let full_cone_private_node_vms = provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::FullConePrivateNodes, true)
                .inspect_err(|err| {
                    println!("Failed to obtain the inventory of Full Cone private node: {err:?}");
                })?;

            let full_cone_nat_gateway_inventory = provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::FullConeNatGateway, true)
                .inspect_err(|err| {
                    println!("Failed to get Full Cone NAT Gateway inventory {err:?}");
                })?;

            if full_cone_nat_gateway_inventory.len() != full_cone_private_node_vms.len() {
                println!("The number of Full Cone private nodes does not match the number of Full Cone NAT Gateway VMs");
                return Err(Error::VmCountMismatch(
                    Some(AnsibleInventoryType::FullConePrivateNodes),
                    Some(AnsibleInventoryType::FullConeNatGateway),
                ));
            }

            inventory.full_cone_private_node_vms = full_cone_private_node_vms;
            inventory.full_cone_nat_gateway_vms = full_cone_nat_gateway_inventory;
        }

        if should_provision_symmetric_private_nodes {
            let symmetric_private_node_vms = provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::SymmetricPrivateNodes, true)
                .inspect_err(|err| {
                    println!("Failed to obtain the inventory of Symmetric private node: {err:?}");
                })?;

            let symmetric_nat_gateway_inventory = provisioner
                .ansible_runner
                .get_inventory(AnsibleInventoryType::SymmetricNatGateway, true)
                .inspect_err(|err| {
                    println!("Failed to get Symmetric NAT Gateway inventory {err:?}");
                })?;

            if symmetric_nat_gateway_inventory.len() != symmetric_private_node_vms.len() {
                println!("The number of Symmetric private nodes does not match the number of Symmetric NAT Gateway VMs");
                return Err(Error::VmCountMismatch(
                    Some(AnsibleInventoryType::SymmetricPrivateNodes),
                    Some(AnsibleInventoryType::SymmetricNatGateway),
                ));
            }

            inventory.symmetric_private_node_vms = symmetric_private_node_vms;
            inventory.symmetric_nat_gateway_vms = symmetric_nat_gateway_inventory;
        }

        Ok(inventory)
    }

    pub fn should_provision_full_cone_private_nodes(&self) -> bool {
        !self.full_cone_private_node_vms.is_empty()
    }

    pub fn should_provision_symmetric_private_nodes(&self) -> bool {
        !self.symmetric_private_node_vms.is_empty()
    }

    pub fn symmetric_private_node_and_gateway_map(
        &self,
    ) -> Result<HashMap<VirtualMachine, VirtualMachine>> {
        Self::match_private_node_vm_and_gateway_vm(
            &self.symmetric_private_node_vms,
            &self.symmetric_nat_gateway_vms,
        )
    }

    pub fn full_cone_private_node_and_gateway_map(
        &self,
    ) -> Result<HashMap<VirtualMachine, VirtualMachine>> {
        Self::match_private_node_vm_and_gateway_vm(
            &self.full_cone_private_node_vms,
            &self.full_cone_nat_gateway_vms,
        )
    }

    pub fn match_private_node_vm_and_gateway_vm(
        private_node_vms: &[VirtualMachine],
        nat_gateway_vms: &[VirtualMachine],
    ) -> Result<HashMap<VirtualMachine, VirtualMachine>> {
        if private_node_vms.len() != nat_gateway_vms.len() {
            println!(
            "The number of private node VMs ({}) does not match the number of NAT Gateway VMs ({})",
            private_node_vms.len(),
            nat_gateway_vms.len()
        );
            error!("The number of private node VMs does not match the number of NAT Gateway VMs: Private VMs: {private_node_vms:?} Nat gateway VMs: {nat_gateway_vms:?}");
            return Err(Error::VmCountMismatch(None, None));
        }

        let mut map = HashMap::new();
        for private_vm in private_node_vms {
            let nat_gateway = nat_gateway_vms
                .iter()
                .find(|vm| {
                    let private_node_name = private_vm.name.split('-').next_back().unwrap();
                    let nat_gateway_name = vm.name.split('-').next_back().unwrap();
                    private_node_name == nat_gateway_name
                })
                .ok_or_else(|| {
                    println!(
                        "Failed to find a matching NAT Gateway for private node: {}",
                        private_vm.name
                    );
                    error!("Failed to find a matching NAT Gateway for private node: {}. Private VMs: {private_node_vms:?} Nat gateway VMs: {nat_gateway_vms:?}", private_vm.name);
                    Error::VmCountMismatch(None, None)
                })?;

            let _ = map.insert(private_vm.clone(), nat_gateway.clone());
        }

        Ok(map)
    }
}

impl From<BootstrapOptions> for ProvisionOptions {
    fn from(bootstrap_options: BootstrapOptions) -> Self {
        ProvisionOptions {
            ant_version: None,
            binary_option: bootstrap_options.binary_option,
            chunk_size: bootstrap_options.chunk_size,
            client_env_variables: None,
            enable_download_verifier: false,
            enable_random_verifier: false,
            enable_performance_verifier: false,
            enable_telegraf: true,
            enable_uploaders: false,
            evm_data_payments_address: bootstrap_options.evm_data_payments_address,
            evm_network: bootstrap_options.evm_network,
            evm_payment_token_address: bootstrap_options.evm_payment_token_address,
            evm_rpc_url: bootstrap_options.evm_rpc_url,
            expected_hash: None,
            expected_size: None,
            file_address: None,
            full_cone_private_node_count: bootstrap_options.full_cone_private_node_count,
            funding_wallet_secret_key: None,
            gas_amount: None,
            interval: Some(bootstrap_options.interval),
            log_format: bootstrap_options.log_format,
            max_archived_log_files: bootstrap_options.max_archived_log_files,
            max_log_files: bootstrap_options.max_log_files,
            max_uploads: None,
            name: bootstrap_options.name,
            network_id: Some(bootstrap_options.network_id),
            network_dashboard_branch: None,
            node_count: bootstrap_options.node_count,
            node_env_variables: bootstrap_options.node_env_variables,
            output_inventory_dir_path: bootstrap_options.output_inventory_dir_path,
            peer_cache_node_count: 0,
            public_rpc: false,
            rewards_address: Some(bootstrap_options.rewards_address),
            symmetric_private_node_count: bootstrap_options.symmetric_private_node_count,
            token_amount: None,
            upload_size: None,
            upload_interval: None,
            uploaders_count: None,
            wallet_secret_keys: None,
        }
    }
}

impl From<DeployOptions> for ProvisionOptions {
    fn from(deploy_options: DeployOptions) -> Self {
        ProvisionOptions {
            ant_version: None,
            binary_option: deploy_options.binary_option,
            chunk_size: deploy_options.chunk_size,
            client_env_variables: deploy_options.client_env_variables,
            enable_download_verifier: deploy_options.enable_download_verifier,
            enable_performance_verifier: deploy_options.enable_performance_verifier,
            enable_random_verifier: deploy_options.enable_random_verifier,
            enable_telegraf: deploy_options.enable_telegraf,
            enable_uploaders: true,
            node_env_variables: deploy_options.node_env_variables,
            evm_data_payments_address: deploy_options.evm_data_payments_address,
            evm_network: deploy_options.evm_network,
            evm_payment_token_address: deploy_options.evm_payment_token_address,
            evm_rpc_url: deploy_options.evm_rpc_url,
            expected_hash: None,
            expected_size: None,
            file_address: None,
            full_cone_private_node_count: deploy_options.full_cone_private_node_count,
            funding_wallet_secret_key: deploy_options.funding_wallet_secret_key,
            gas_amount: deploy_options.initial_gas,
            interval: Some(deploy_options.interval),
            log_format: deploy_options.log_format,
            max_archived_log_files: deploy_options.max_archived_log_files,
            max_log_files: deploy_options.max_log_files,
            max_uploads: None,
            name: deploy_options.name,
            network_id: Some(deploy_options.network_id),
            network_dashboard_branch: deploy_options.network_dashboard_branch,
            node_count: deploy_options.node_count,
            output_inventory_dir_path: deploy_options.output_inventory_dir_path,
            peer_cache_node_count: deploy_options.peer_cache_node_count,
            public_rpc: deploy_options.public_rpc,
            rewards_address: Some(deploy_options.rewards_address),
            symmetric_private_node_count: deploy_options.symmetric_private_node_count,
            token_amount: deploy_options.initial_tokens,
            upload_size: None,
            upload_interval: Some(deploy_options.upload_interval),
            uploaders_count: Some(deploy_options.uploaders_count),
            wallet_secret_keys: None,
        }
    }
}

impl From<ClientsDeployOptions> for ProvisionOptions {
    fn from(client_options: ClientsDeployOptions) -> Self {
        Self {
            ant_version: None,
            binary_option: client_options.binary_option,
            chunk_size: client_options.chunk_size,
            client_env_variables: client_options.client_env_variables,
            enable_download_verifier: client_options.enable_download_verifier,
            enable_random_verifier: client_options.enable_random_verifier,
            enable_performance_verifier: client_options.enable_performance_verifier,
            enable_telegraf: client_options.enable_telegraf,
            enable_uploaders: client_options.enable_uploaders,
            evm_data_payments_address: client_options.evm_details.data_payments_address,
            evm_network: client_options.evm_details.network,
            evm_payment_token_address: client_options.evm_details.payment_token_address,
            evm_rpc_url: client_options.evm_details.rpc_url,
            expected_hash: client_options.expected_hash,
            expected_size: client_options.expected_size,
            file_address: client_options.file_address,
            full_cone_private_node_count: 0,
            funding_wallet_secret_key: client_options.funding_wallet_secret_key,
            gas_amount: client_options.initial_gas,
            interval: None,
            log_format: None,
            max_archived_log_files: client_options.max_archived_log_files,
            max_log_files: client_options.max_log_files,
            max_uploads: client_options.max_uploads,
            name: client_options.name,
            network_id: client_options.network_id,
            network_dashboard_branch: None,
            node_count: 0,
            node_env_variables: None,
            output_inventory_dir_path: client_options.output_inventory_dir_path,
            peer_cache_node_count: 0,
            public_rpc: false,
            rewards_address: None,
            symmetric_private_node_count: 0,
            token_amount: client_options.initial_tokens,
            upload_size: client_options.upload_size,
            upload_interval: None,
            uploaders_count: Some(client_options.uploaders_count),
            wallet_secret_keys: client_options.wallet_secret_keys,
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

    pub fn build_autonomi_binaries(
        &self,
        options: &ProvisionOptions,
        binaries_to_build: Option<Vec<String>>,
    ) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for build VM...");
        let build_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, true)?;
        let build_ip = build_inventory[0].public_ip_addr;
        self.ssh_client
            .wait_for_ssh_availability(&build_ip, &self.cloud_provider.get_ssh_user())?;

        println!("Running ansible against build VM...");
        let base_extra_vars = extra_vars::build_binaries_extra_vars_doc(options)?;

        let extra_vars = if let Some(binaries) = binaries_to_build {
            let mut build_ant = false;
            let mut build_antnode = false;
            let mut build_antctl = false;
            let mut build_antctld = false;

            for binary in &binaries {
                match binary.as_str() {
                    "ant" => build_ant = true,
                    "antnode" => build_antnode = true,
                    "antctl" => build_antctl = true,
                    "antctld" => build_antctld = true,
                    _ => return Err(Error::InvalidBinaryName(binary.clone())),
                }
            }

            let mut json_value: serde_json::Value = serde_json::from_str(&base_extra_vars)?;
            if let serde_json::Value::Object(ref mut map) = json_value {
                map.insert("build_ant".to_string(), serde_json::Value::Bool(build_ant));
                map.insert(
                    "build_antnode".to_string(),
                    serde_json::Value::Bool(build_antnode),
                );
                map.insert(
                    "build_antctl".to_string(),
                    serde_json::Value::Bool(build_antctl),
                );
                map.insert(
                    "build_antctld".to_string(),
                    serde_json::Value::Bool(build_antctld),
                );
            }
            json_value.to_string()
        } else {
            base_extra_vars
        };

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

    pub fn get_symmetric_nat_gateway_inventory(&self) -> Result<Vec<VirtualMachine>> {
        self.ansible_runner
            .get_inventory(AnsibleInventoryType::SymmetricNatGateway, false)
    }

    pub fn get_full_cone_nat_gateway_inventory(&self) -> Result<Vec<VirtualMachine>> {
        self.ansible_runner
            .get_inventory(AnsibleInventoryType::FullConeNatGateway, false)
    }

    pub fn get_client_inventory(&self) -> Result<Vec<VirtualMachine>> {
        self.ansible_runner
            .get_inventory(AnsibleInventoryType::Clients, false)
    }

    pub fn get_node_registries(
        &self,
        inventory_type: &AnsibleInventoryType,
    ) -> Result<DeploymentNodeRegistries> {
        debug!("Fetching node manager inventory for {inventory_type:?}");
        let temp_dir_path = tempfile::tempdir()?.into_path();
        let temp_dir_json = serde_json::to_string(&temp_dir_path)?;

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::AntCtlInventory,
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
                &options.binary_option,
            )),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_genesis_node(&self, options: &ProvisionOptions) -> Result<()> {
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
                None,
                1,
                options.evm_network.clone(),
                false,
            )?),
        )?;

        print_duration(start.elapsed());

        Ok(())
    }

    pub fn provision_full_cone(
        &self,
        options: &ProvisionOptions,
        initial_contact_peer: Option<String>,
        initial_network_contacts_url: Option<String>,
        private_node_inventory: PrivateNodeProvisionInventory,
        new_full_cone_nat_gateway_new_vms_for_upscale: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        // Step 1 of Full Cone NAT Gateway
        let start = Instant::now();
        self.print_ansible_run_banner("Provision Full Cone NAT Gateway - Step 1");

        for vm in new_full_cone_nat_gateway_new_vms_for_upscale
            .as_ref()
            .unwrap_or(&private_node_inventory.full_cone_nat_gateway_vms)
            .iter()
        {
            println!(
                "Checking SSH availability for Full Cone NAT Gateway: {}",
                vm.public_ip_addr
            );
            self.ssh_client
                .wait_for_ssh_availability(&vm.public_ip_addr, &self.cloud_provider.get_ssh_user())
                .map_err(|e| {
                    println!("Failed to establish SSH connection to Full Cone NAT Gateway: {e}");
                    e
                })?;
        }

        let mut modified_private_node_inventory = private_node_inventory.clone();

        // If we are upscaling, then we cannot access the gateway VMs which are already deployed.
        if let Some(new_full_cone_nat_gateway_new_vms_for_upscale) =
            &new_full_cone_nat_gateway_new_vms_for_upscale
        {
            debug!("Removing existing full cone NAT Gateway and private node VMs from the inventory. Old inventory: {modified_private_node_inventory:?}");
            let mut names_to_keep = Vec::new();

            for vm in new_full_cone_nat_gateway_new_vms_for_upscale.iter() {
                let nat_gateway_name = vm.name.split('-').next_back().unwrap();
                names_to_keep.push(nat_gateway_name);
            }

            modified_private_node_inventory
                .full_cone_nat_gateway_vms
                .retain(|vm| {
                    let nat_gateway_name = vm.name.split('-').next_back().unwrap();
                    names_to_keep.contains(&nat_gateway_name)
                });
            modified_private_node_inventory
                .full_cone_private_node_vms
                .retain(|vm| {
                    let nat_gateway_name = vm.name.split('-').next_back().unwrap();
                    names_to_keep.contains(&nat_gateway_name)
                });
            debug!("New inventory after removing existing full cone NAT Gateway and private node VMs: {modified_private_node_inventory:?}");
        }

        if modified_private_node_inventory
            .full_cone_nat_gateway_vms
            .is_empty()
        {
            error!("There are no full cone NAT Gateway VMs available to upscale");
            return Ok(());
        }

        let private_node_ip_map = modified_private_node_inventory
            .full_cone_private_node_and_gateway_map()?
            .into_iter()
            .map(|(k, v)| {
                let gateway_name = if new_full_cone_nat_gateway_new_vms_for_upscale.is_some() {
                    debug!("Upscaling, using public IP address for gateway name");
                    v.public_ip_addr.to_string()
                } else {
                    v.name.clone()
                };
                (gateway_name, k.private_ip_addr)
            })
            .collect::<HashMap<String, IpAddr>>();

        if private_node_ip_map.is_empty() {
            println!("There are no full cone private node VM available to be routed through the full cone NAT Gateway");
            return Err(Error::EmptyInventory(
                AnsibleInventoryType::FullConePrivateNodes,
            ));
        }

        let vars = extra_vars::build_nat_gateway_extra_vars_doc(
            &options.name,
            private_node_ip_map.clone(),
            "step1",
        );
        debug!("Provisioning Full Cone NAT Gateway - Step 1 with vars: {vars}");
        let gateway_inventory = if new_full_cone_nat_gateway_new_vms_for_upscale.is_some() {
            debug!("Upscaling, using static inventory for full cone nat gateway.");
            generate_full_cone_nat_gateway_static_environment_inventory(
                &modified_private_node_inventory.full_cone_nat_gateway_vms,
                &options.name,
                &options.output_inventory_dir_path,
            )?;

            AnsibleInventoryType::FullConeNatGatewayStatic
        } else {
            AnsibleInventoryType::FullConeNatGateway
        };
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StaticFullConeNatGateway,
            gateway_inventory,
            Some(vars),
        )?;

        // setup private node config
        self.print_ansible_run_banner("Provisioning Full Cone Private Node Config");

        generate_full_cone_private_node_static_environment_inventory(
            &options.name,
            &options.output_inventory_dir_path,
            &private_node_inventory.full_cone_private_node_vms,
            &private_node_inventory.full_cone_nat_gateway_vms,
            &self.ssh_client.private_key_path,
        )
        .inspect_err(|err| {
            error!("Failed to generate full cone private node static inv with err: {err:?}")
        })?;

        // For a new deployment, it's quite probable that SSH is available, because this part occurs
        // after the genesis node has been provisioned. However, for a bootstrap deploy, we need to
        // check that SSH is available before proceeding.
        println!("Obtaining IP addresses for nodes...");
        let inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::FullConePrivateNodes, true)?;

        println!("Waiting for SSH availability on Symmetric Private nodes...");
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
            AnsiblePlaybook::PrivateNodeConfig,
            AnsibleInventoryType::FullConePrivateNodes,
            Some(
                extra_vars::build_full_cone_private_node_config_extra_vars_docs(
                    &private_node_inventory,
                )?,
            ),
        )?;

        // Step 2 of Full Cone NAT Gateway

        let vars = extra_vars::build_nat_gateway_extra_vars_doc(
            &options.name,
            private_node_ip_map,
            "step2",
        );

        self.print_ansible_run_banner("Provisioning Full Cone NAT Gateway - Step 2");
        debug!("Provisioning Full Cone NAT Gateway - Step 2 with vars: {vars}");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::StaticFullConeNatGateway,
            gateway_inventory,
            Some(vars),
        )?;

        // provision the nodes

        let home_dir = std::env::var("HOME").inspect_err(|err| {
            println!("Failed to get home directory with error: {err:?}",);
        })?;
        let known_hosts_path = format!("{home_dir}/.ssh/known_hosts");
        debug!("Cleaning up known hosts file at {known_hosts_path} ");
        run_external_command(
            PathBuf::from("rm"),
            std::env::current_dir()?,
            vec![known_hosts_path],
            false,
            false,
        )?;

        self.print_ansible_run_banner("Provision Full Cone Private Nodes");

        self.ssh_client.set_full_cone_nat_routed_vms(
            &private_node_inventory.full_cone_private_node_vms,
            &private_node_inventory.full_cone_nat_gateway_vms,
        )?;

        self.provision_nodes(
            options,
            initial_contact_peer,
            initial_network_contacts_url,
            NodeType::FullConePrivateNode,
        )?;

        print_duration(start.elapsed());
        Ok(())
    }
    pub fn provision_symmetric_nat_gateway(
        &self,
        options: &ProvisionOptions,
        private_node_inventory: &PrivateNodeProvisionInventory,
    ) -> Result<()> {
        let start = Instant::now();
        for vm in &private_node_inventory.symmetric_nat_gateway_vms {
            println!(
                "Checking SSH availability for Symmetric NAT Gateway: {}",
                vm.public_ip_addr
            );
            self.ssh_client
                .wait_for_ssh_availability(&vm.public_ip_addr, &self.cloud_provider.get_ssh_user())
                .map_err(|e| {
                    println!("Failed to establish SSH connection to Symmetric NAT Gateway: {e}");
                    e
                })?;
        }

        let private_node_ip_map = private_node_inventory
            .symmetric_private_node_and_gateway_map()?
            .into_iter()
            .map(|(k, v)| (v.name.clone(), k.private_ip_addr))
            .collect::<HashMap<String, IpAddr>>();

        if private_node_ip_map.is_empty() {
            println!("There are no Symmetric private node VM available to be routed through the Symmetric NAT Gateway");
            return Err(Error::EmptyInventory(
                AnsibleInventoryType::SymmetricPrivateNodes,
            ));
        }

        let vars = extra_vars::build_nat_gateway_extra_vars_doc(
            &options.name,
            private_node_ip_map,
            "symmetric",
        );
        debug!("Provisioning Symmetric NAT Gateway with vars: {vars}");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::SymmetricNatGateway,
            AnsibleInventoryType::SymmetricNatGateway,
            Some(vars),
        )?;

        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_nodes(
        &self,
        options: &ProvisionOptions,
        initial_contact_peer: Option<String>,
        initial_network_contacts_url: Option<String>,
        node_type: NodeType,
    ) -> Result<()> {
        let start = Instant::now();
        let mut write_older_cache_files = false;
        let (inventory_type, node_count) = match &node_type {
            NodeType::FullConePrivateNode => (
                node_type.to_ansible_inventory_type(),
                options.full_cone_private_node_count,
            ),
            // use provision_genesis_node fn
            NodeType::Generic => (node_type.to_ansible_inventory_type(), options.node_count),
            NodeType::Genesis => return Err(Error::InvalidNodeType(node_type)),
            NodeType::PeerCache => {
                write_older_cache_files = true;
                (
                    node_type.to_ansible_inventory_type(),
                    options.peer_cache_node_count,
                )
            }
            NodeType::SymmetricPrivateNode => (
                node_type.to_ansible_inventory_type(),
                options.symmetric_private_node_count,
            ),
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

        let playbook = match node_type {
            NodeType::Generic => AnsiblePlaybook::Nodes,
            NodeType::PeerCache => AnsiblePlaybook::PeerCacheNodes,
            NodeType::FullConePrivateNode => AnsiblePlaybook::Nodes,
            NodeType::SymmetricPrivateNode => AnsiblePlaybook::Nodes,
            _ => return Err(Error::InvalidNodeType(node_type.clone())),
        };
        self.ansible_runner.run_playbook(
            playbook,
            inventory_type,
            Some(extra_vars::build_node_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                node_type.clone(),
                initial_contact_peer,
                initial_network_contacts_url,
                node_count,
                options.evm_network.clone(),
                write_older_cache_files,
            )?),
        )?;

        print_duration(start.elapsed());
        Ok(())
    }

    pub fn provision_symmetric_private_nodes(
        &self,
        options: &mut ProvisionOptions,
        initial_contact_peer: Option<String>,
        initial_network_contacts_url: Option<String>,
        private_node_inventory: &PrivateNodeProvisionInventory,
    ) -> Result<()> {
        let start = Instant::now();
        self.print_ansible_run_banner("Provision Symmetric Private Node Config");

        generate_symmetric_private_node_static_environment_inventory(
            &options.name,
            &options.output_inventory_dir_path,
            &private_node_inventory.symmetric_private_node_vms,
            &private_node_inventory.symmetric_nat_gateway_vms,
            &self.ssh_client.private_key_path,
        )
        .inspect_err(|err| {
            error!("Failed to generate symmetric private node static inv with err: {err:?}")
        })?;

        self.ssh_client.set_symmetric_nat_routed_vms(
            &private_node_inventory.symmetric_private_node_vms,
            &private_node_inventory.symmetric_nat_gateway_vms,
        )?;

        let inventory_type = AnsibleInventoryType::SymmetricPrivateNodes;

        // For a new deployment, it's quite probable that SSH is available, because this part occurs
        // after the genesis node has been provisioned. However, for a bootstrap deploy, we need to
        // check that SSH is available before proceeding.
        println!("Obtaining IP addresses for nodes...");
        let inventory = self.ansible_runner.get_inventory(inventory_type, true)?;

        println!("Waiting for SSH availability on Symmetric Private nodes...");
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
            AnsiblePlaybook::PrivateNodeConfig,
            inventory_type,
            Some(
                extra_vars::build_symmetric_private_node_config_extra_vars_doc(
                    private_node_inventory,
                )?,
            ),
        )?;

        println!("Provisioned Symmetric Private Node Config");
        print_duration(start.elapsed());

        self.provision_nodes(
            options,
            initial_contact_peer,
            initial_network_contacts_url,
            NodeType::SymmetricPrivateNode,
        )?;

        Ok(())
    }

    pub async fn provision_downloaders(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: Option<String>,
        genesis_network_contacts_url: Option<String>,
    ) -> Result<()> {
        let start = Instant::now();

        println!("Running ansible against Client machine to start the downloader script.");
        debug!("Running ansible against Client machine to start the downloader script.");

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Downloaders,
            AnsibleInventoryType::Clients,
            Some(extra_vars::build_downloaders_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                genesis_multiaddr,
                genesis_network_contacts_url,
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_clients(
        &self,
        options: &ProvisionOptions,
        genesis_multiaddr: Option<String>,
        genesis_network_contacts_url: Option<String>,
    ) -> Result<()> {
        let start = Instant::now();

        let sk_map = if let Some(wallet_keys) = &options.wallet_secret_keys {
            self.prepare_pre_funded_wallets(wallet_keys).await?
        } else {
            self.deposit_funds_to_clients(&FundingOptions {
                evm_data_payments_address: options.evm_data_payments_address.clone(),
                evm_network: options.evm_network.clone(),
                evm_payment_token_address: options.evm_payment_token_address.clone(),
                evm_rpc_url: options.evm_rpc_url.clone(),
                funding_wallet_secret_key: options.funding_wallet_secret_key.clone(),
                gas_amount: options.gas_amount,
                token_amount: options.token_amount,
                uploaders_count: options.uploaders_count,
            })
            .await?
        };

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Uploaders,
            AnsibleInventoryType::Clients,
            Some(extra_vars::build_clients_extra_vars_doc(
                &self.cloud_provider.to_string(),
                options,
                genesis_multiaddr,
                genesis_network_contacts_url,
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
        service_names: Option<Vec<String>>,
    ) -> Result<()> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("interval", &interval.as_millis().to_string());
        if let Some(delay) = delay {
            extra_vars.add_variable("delay", &delay.to_string());
        }
        if let Some(service_names) = service_names {
            extra_vars.add_list_variable("service_names", service_names);
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
            AnsibleInventoryType::PeerCacheNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::PeerCache,
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
            AnsibleInventoryType::SymmetricPrivateNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::SymmetricPrivateNode,
            )?),
        )?;

        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNodeTelegrafConfig,
            AnsibleInventoryType::FullConePrivateNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::FullConePrivateNode,
            )?),
        )?;
        Ok(())
    }

    pub fn upgrade_client_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeClientTelegrafConfig,
            AnsibleInventoryType::Clients,
            Some(extra_vars::build_client_telegraf_upgrade(name)?),
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
            AnsibleInventoryType::PeerCacheNodes,
            Some(options.get_ansible_vars()),
        ) {
            Ok(()) => println!("All Peer Cache nodes were successfully upgraded"),
            Err(_) => {
                println!("WARNING: some Peer Cacche nodes may not have been upgraded or restarted");
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
            AnsibleInventoryType::SymmetricPrivateNodes,
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

    pub fn upgrade_antctl(
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
                AnsiblePlaybook::UpgradeAntctl,
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
                AnsiblePlaybook::UpgradeAntctl,
                AnsibleInventoryType::Custom,
                Some(extra_vars.build()),
            )?;
            return Ok(());
        }

        println!("Running the upgrade safenode-manager playbook for all node types");
        for node_inv_type in AnsibleInventoryType::iter_node_type() {
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeAntctl,
                node_inv_type,
                Some(extra_vars.build()),
            )?;
        }

        Ok(())
    }

    pub fn upgrade_nginx_config(
        &self,
        environment_name: &str,
        custom_inventory: Option<Vec<VirtualMachine>>,
    ) -> Result<()> {
        if let Some(custom_inventory) = custom_inventory {
            println!("Running the upgrade nginx config playbook with a custom inventory");
            generate_custom_environment_inventory(
                &custom_inventory,
                environment_name,
                &self.ansible_runner.working_directory_path.join("inventory"),
            )?;
            self.ansible_runner.run_playbook(
                AnsiblePlaybook::UpgradeNginx,
                AnsibleInventoryType::Custom,
                None,
            )?;
            return Ok(());
        }

        println!("Running the upgrade nginx config playbook for peer cache nodes");
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeNginx,
            AnsibleInventoryType::PeerCacheNodes,
            None,
        )?;
        Ok(())
    }

    pub fn upgrade_geoip_telegraf(&self, name: &str) -> Result<()> {
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::UpgradeGeoIpTelegrafConfig,
            AnsibleInventoryType::PeerCacheNodes,
            Some(extra_vars::build_node_telegraf_upgrade(
                name,
                &NodeType::PeerCache,
            )?),
        )?;
        Ok(())
    }

    pub fn print_ansible_run_banner(&self, s: &str) {
        let ansible_run_msg = "Ansible Run: ";
        let line = "=".repeat(s.len() + ansible_run_msg.len());
        println!("{line}\n{ansible_run_msg}{s}\n{line}");
    }
}
