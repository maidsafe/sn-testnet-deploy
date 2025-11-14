// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::inventory::AnsibleInventoryType;
use super::provisioning::PrivateNodeProvisionInventory;
use crate::inventory::VirtualMachine;
use crate::NodeType;
use crate::{ansible::provisioning::ProvisionOptions, CloudProvider, EvmNetwork};
use crate::{BinaryOption, Error, Result};
use alloy::hex::ToHexExt;
use alloy::signers::local::PrivateKeySigner;
use log::error;
use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;

const ANT_S3_BUCKET_URL: &str = "https://autonomi-cli.s3.eu-west-2.amazonaws.com";
const ANTCTL_S3_BUCKET_URL: &str = "https://antctl.s3.eu-west-2.amazonaws.com";
// The old `sn-node` S3 bucket will continue to be used to store custom branch builds.
// They are stored in here regardless of which binary they are.
const BRANCH_S3_BUCKET_URL: &str = "https://sn-node.s3.eu-west-2.amazonaws.com";
const RPC_CLIENT_BUCKET_URL: &str = "https://antnode-rpc-client.s3.eu-west-2.amazonaws.com";

#[derive(Default, Clone)]
pub struct ExtraVarsDocBuilder {
    map: serde_json::Map<String, Value>,
}

impl ExtraVarsDocBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_variable(&mut self, name: &str, value: &str) -> &mut Self {
        self.map
            .insert(name.to_owned(), Value::String(value.to_owned()));
        self
    }

    pub fn add_boolean_variable(&mut self, name: &str, value: bool) -> &mut Self {
        self.map.insert(name.to_owned(), Value::Bool(value));
        self
    }

    pub fn add_list_variable(&mut self, name: &str, values: Vec<String>) -> &mut Self {
        if let Some(list) = self.map.get_mut(name) {
            if let Value::Array(list) = list {
                for val in values {
                    list.push(Value::String(val));
                }
            }
        } else {
            let json_list = values
                .iter()
                .map(|val| Value::String(val.to_owned()))
                .collect();
            self.map.insert(name.to_owned(), Value::Array(json_list));
        }
        self
    }

    /// Add a serde value to the extra vars map. This is useful if you have a complex type.
    pub fn add_serde_value(&mut self, name: &str, value: Value) {
        self.map.insert(name.to_owned(), value);
    }

    pub fn add_env_variable_list(
        &mut self,
        name: &str,
        variables: Vec<(String, String)>,
    ) -> &mut Self {
        let mut joined_env_vars = Vec::new();
        for (name, val) in variables {
            joined_env_vars.push(format!("{name}={val}"));
        }
        let joined_env_vars = joined_env_vars.join(",");
        self.add_variable(name, &joined_env_vars);
        self
    }

    pub fn add_build_variables(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner,
                branch,
                antnode_features,
                skip_binary_build: _,
            } => {
                self.add_variable("custom_bin", "true");
                self.add_variable("testnet_name", deployment_name);
                self.add_variable("org", repo_owner);
                self.add_variable("branch", branch);
                if let Some(features) = antnode_features {
                    self.add_variable("antnode_features_list", features);
                }
            }
            BinaryOption::Versioned { .. } => {
                self.add_variable("custom_bin", "false");
            }
        }
    }

    pub fn add_rpc_client_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
    ) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "antnode_rpc_client_archive_url",
                    &format!(
                        "{BRANCH_S3_BUCKET_URL}/{repo_owner}/{branch}/antnode_rpc_client-{deployment_name}-x86_64-unknown-linux-musl.tar.gz"
                    ),
                    branch,
                    repo_owner,
                );
            }
            _ => {
                self.add_variable(
                    "antnode_rpc_client_archive_url",
                    &format!(
                        "{RPC_CLIENT_BUCKET_URL}/antnode_rpc_client-latest-x86_64-unknown-linux-musl.tar.gz"
                    ),
                );
            }
        }
    }

    pub fn add_node_url_or_version(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "node_archive_url",
                    &format!(
                        "{BRANCH_S3_BUCKET_URL}/{repo_owner}/{branch}/antnode-{deployment_name}-x86_64-unknown-linux-musl.tar.gz"
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned {
                antnode_version, ..
            } => {
                // An unwrap would be justified here because the antnode version must be set for the
                // type of deployment where this will apply.
                self.add_variable("version", &antnode_version.as_ref().unwrap().to_string());
            }
        }
    }

    pub fn add_antctl_url(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "antctl_archive_url",
                    &format!(
                        "{BRANCH_S3_BUCKET_URL}/{repo_owner}/{branch}/antctl-{deployment_name}-x86_64-unknown-linux-musl.tar.gz"
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned { antctl_version, .. } => {
                // An unwrap would be justified here because the antctl version must be set for the
                // type of deployment where this will apply.
                self.add_variable(
                    "antctl_archive_url",
                    &format!(
                        "{}/antctl-{}-x86_64-unknown-linux-musl.tar.gz",
                        ANTCTL_S3_BUCKET_URL,
                        antctl_version.as_ref().unwrap()
                    ),
                );
            }
        }
    }

    pub fn add_antctld_url(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "antctld_archive_url",
                    &format!(
                        "{BRANCH_S3_BUCKET_URL}/{repo_owner}/{branch}/antctld-{deployment_name}-x86_64-unknown-linux-musl.tar.gz"
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned { antctl_version, .. } => {
                // An unwrap would be justified here because the antctl version must be set for the
                // type of deployment where this will apply.
                self.add_variable(
                    "antctld_archive_url",
                    &format!(
                        "{}/antctld-{}-x86_64-unknown-linux-musl.tar.gz",
                        ANTCTL_S3_BUCKET_URL,
                        antctl_version.as_ref().unwrap()
                    ),
                );
            }
        }
    }

    pub fn add_ant_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
        ant_version: Option<String>,
    ) -> Result<(), Error> {
        // This applies when upscaling the Clients.
        // In that scenario, the safe version in the binary option is not set to the correct value
        // because it is not recorded in the inventory.
        if let Some(version) = ant_version {
            self.add_variable(
                "ant_archive_url",
                &format!("{ANT_S3_BUCKET_URL}/ant-{version}-x86_64-unknown-linux-musl.tar.gz"),
            );
            return Ok(());
        }

        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "ant_archive_url",
                    &format!(
                        "{BRANCH_S3_BUCKET_URL}/{repo_owner}/{branch}/ant-{deployment_name}-x86_64-unknown-linux-musl.tar.gz"
                    ),
                    branch,
                    repo_owner,
                );
                Ok(())
            }
            BinaryOption::Versioned { ant_version, .. } => match ant_version {
                Some(version) => {
                    self.add_variable(
                        "ant_archive_url",
                        &format!(
                            "{ANT_S3_BUCKET_URL}/ant-{version}-x86_64-unknown-linux-musl.tar.gz"
                        ),
                    );
                    Ok(())
                }
                None => Err(Error::NoClientError),
            },
        }
    }

    pub fn build(&self) -> String {
        Value::Object(self.map.clone()).to_string()
    }

    fn add_branch_url_variable(&mut self, name: &str, value: &str, branch: &str, repo_owner: &str) {
        self.add_variable("branch", branch);
        self.add_variable("org", repo_owner);
        self.add_variable(name, value);
    }
}

pub fn build_nat_gateway_extra_vars_doc(
    name: &str,
    private_node_ip_map: HashMap<String, IpAddr>,
    action: &str,
) -> String {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);

    let serde_map = Value::Object(
        private_node_ip_map
            .into_iter()
            .map(|(k, v)| (k, Value::String(v.to_string())))
            .collect(),
    );
    extra_vars.add_serde_value("node_private_ip_map", serde_map);
    extra_vars.add_variable("action", action);
    extra_vars.build()
}

#[allow(clippy::too_many_arguments)]
pub fn build_node_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    node_type: NodeType,
    genesis_multiaddr: Option<String>,
    network_contacts_url: Option<String>,
    node_instance_count: u16,
    evm_network: EvmNetwork,
    write_older_cache_files: bool,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    extra_vars.add_variable("node_type", node_type.telegraf_role());
    if let Some(genesis_multiaddr) = genesis_multiaddr {
        extra_vars.add_variable("genesis_multiaddr", &genesis_multiaddr);
    }
    if let Some(network_contacts_url) = network_contacts_url {
        extra_vars.add_variable("network_contacts_url", &network_contacts_url);
    }

    extra_vars.add_variable("node_instance_count", &node_instance_count.to_string());
    if let Some(interval) = options.interval {
        extra_vars.add_variable("interval", &interval.as_millis().to_string());
    }
    if let Some(log_format) = options.log_format {
        extra_vars.add_variable("log_format", log_format.as_str());
    } else {
        extra_vars.add_variable("log_format", "json");
    }

    extra_vars.add_variable(
        "max_archived_log_files",
        &options.max_archived_log_files.to_string(),
    );
    extra_vars.add_variable("max_log_files", &options.max_log_files.to_string());
    if options.public_rpc {
        extra_vars.add_variable("public_rpc", "true");
    }

    match node_type {
        NodeType::FullConePrivateNode => {
            // Full cone private nodes do not need relay as it is a straight port forward.
            extra_vars.add_variable("private_ip", "true");
            extra_vars.add_boolean_variable("enable_upnp", false);
        }
        NodeType::SymmetricPrivateNode => {
            // Symmetric private nodes need relay and private ip.
            extra_vars.add_variable("private_ip", "true");
            extra_vars.add_variable("relay", "true");
            extra_vars.add_boolean_variable("enable_upnp", false);
        }
        NodeType::Upnp => {
            extra_vars.add_boolean_variable("enable_upnp", true);
        }
        _ => {
            extra_vars.add_boolean_variable("enable_upnp", false);
        }
    }

    if write_older_cache_files {
        extra_vars.add_variable("write_older_cache_files", "true");
    }

    if let Some(network_id) = options.network_id {
        extra_vars.add_variable("network_id", &network_id.to_string());
    }

    extra_vars.add_boolean_variable("enable_logging", options.enable_logging);
    extra_vars.add_boolean_variable("enable_metrics", options.enable_metrics);
    extra_vars.add_boolean_variable("disable_nodes", options.disable_nodes);

    extra_vars.add_node_url_or_version(&options.name, &options.binary_option);
    extra_vars.add_antctl_url(&options.name, &options.binary_option);
    extra_vars.add_antctld_url(&options.name, &options.binary_option);

    if let Some(env_vars) = &options.node_env_variables {
        extra_vars.add_env_variable_list("node_env_variables", env_vars.clone());
    }

    if let Some(client_env_vars) = &options.client_env_variables {
        extra_vars.add_env_variable_list("client_env_variables", client_env_vars.clone());
    }

    extra_vars.add_variable(
        "rewards_address",
        options
            .rewards_address
            .as_ref()
            .ok_or_else(|| Error::RewardsAddressNotSet)?,
    );
    extra_vars.add_variable("evm_network_type", &evm_network.to_string());
    if let Some(evm_data_payment_token_address) = &options.evm_data_payments_address {
        extra_vars.add_variable("evm_data_payments_address", evm_data_payment_token_address);
    }
    if let Some(evm_payment_token_address) = &options.evm_payment_token_address {
        extra_vars.add_variable("evm_payment_token_address", evm_payment_token_address);
    }
    if let Some(evm_rpc_url) = &options.evm_rpc_url {
        extra_vars.add_variable("evm_rpc_url", evm_rpc_url);
    }

    extra_vars.add_boolean_variable(
        "start_performance_verifier",
        options.start_performance_verifier,
    );

    extra_vars.add_variable(
        "upload_size",
        &options.upload_size.unwrap_or(100).to_string(),
    );

    if let Some(branch) = &options.network_dashboard_branch {
        extra_vars.add_variable("network_dashboard_branch", branch);
    }

    Ok(extra_vars.build())
}

pub fn build_full_cone_private_node_config_extra_vars_docs(
    private_node_inventory: &PrivateNodeProvisionInventory,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();

    let map = private_node_inventory.full_cone_private_node_and_gateway_map()?;
    if map.is_empty() {
        error!("Private node inventory map is empty");
        return Err(Error::EmptyInventory(
            AnsibleInventoryType::FullConePrivateNodes,
        ));
    }

    let serde_map = Value::Object(
        map.into_iter()
            .map(|(private_node_vm, nat_gateway_vm)| {
                (
                    // hostname of private node returns the private ip address, since we're using static inventory.
                    private_node_vm.private_ip_addr.to_string(),
                    Value::String(nat_gateway_vm.private_ip_addr.to_string()),
                )
            })
            .collect(),
    );
    extra_vars.add_serde_value("nat_gateway_private_ip_map", serde_map);

    Ok(extra_vars.build())
}

pub fn build_port_restricted_cone_private_node_config_extra_vars_docs(
    private_node_inventory: &PrivateNodeProvisionInventory,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();

    let map = private_node_inventory.port_restricted_cone_private_node_and_gateway_map()?;
    if map.is_empty() {
        error!("Private node inventory map is empty");
        return Err(Error::EmptyInventory(
            AnsibleInventoryType::PortRestrictedConePrivateNodes,
        ));
    }

    let serde_map = Value::Object(
        map.into_iter()
            .map(|(private_node_vm, nat_gateway_vm)| {
                (
                    // hostname of private node returns the private ip address, since we're using static inventory.
                    private_node_vm.private_ip_addr.to_string(),
                    Value::String(nat_gateway_vm.private_ip_addr.to_string()),
                )
            })
            .collect(),
    );
    extra_vars.add_serde_value("nat_gateway_private_ip_map", serde_map);

    Ok(extra_vars.build())
}

pub fn build_symmetric_private_node_config_extra_vars_doc(
    private_node_inventory: &PrivateNodeProvisionInventory,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();

    let map = private_node_inventory.symmetric_private_node_and_gateway_map()?;
    if map.is_empty() {
        error!("Private node inventory map is empty");
        return Err(Error::EmptyInventory(
            AnsibleInventoryType::SymmetricPrivateNodes,
        ));
    }

    let serde_map = Value::Object(
        map.into_iter()
            .map(|(private_node_vm, nat_gateway_vm)| {
                (
                    // hostname of private node returns the private ip address, since we're using static inventory.
                    private_node_vm.private_ip_addr.to_string(),
                    Value::String(nat_gateway_vm.private_ip_addr.to_string()),
                )
            })
            .collect(),
    );
    extra_vars.add_serde_value("nat_gateway_private_ip_map", serde_map);

    Ok(extra_vars.build())
}

pub fn build_downloaders_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    peer: Option<String>,
    network_contacts_url: Option<String>,
) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    if let Some(peer) = peer {
        extra_vars.add_variable("peer", &peer);
    }
    if let Some(network_contacts_url) = network_contacts_url {
        extra_vars.add_variable("network_contacts_url", &network_contacts_url);
    }

    extra_vars.add_ant_url_or_version(
        &options.name,
        &options.binary_option,
        options.ant_version.clone(),
    )?;

    extra_vars.add_boolean_variable("start_delayed_verifier", options.start_delayed_verifier);
    extra_vars.add_boolean_variable("enable_metrics", options.enable_metrics);
    extra_vars.add_boolean_variable("start_random_verifier", options.start_random_verifier);
    extra_vars.add_boolean_variable(
        "start_performance_verifier",
        options.start_performance_verifier,
    );

    if let Some(file_address) = &options.file_address {
        extra_vars.add_variable("file_address", file_address);
        if let Some(expected_hash) = &options.expected_hash {
            extra_vars.add_variable("expected_hash", expected_hash);
        }
        if let Some(expected_size) = &options.expected_size {
            extra_vars.add_variable("expected_size", &expected_size.to_string());
        }
    }

    extra_vars.add_variable("evm_network_type", &options.evm_network.to_string());
    if let Some(evm_data_payment_token_address) = &options.evm_data_payments_address {
        extra_vars.add_variable("evm_data_payments_address", evm_data_payment_token_address);
    }
    if let Some(evm_payment_token_address) = &options.evm_payment_token_address {
        extra_vars.add_variable("evm_payment_token_address", evm_payment_token_address);
    }
    if let Some(evm_rpc_url) = &options.evm_rpc_url {
        extra_vars.add_variable("evm_rpc_url", evm_rpc_url);
    }
    if let Some(network_id) = options.network_id {
        extra_vars.add_variable("network_id", &network_id.to_string());
    }
    if let Some(delayed_verifier_batch_size) = options.delayed_verifier_batch_size {
        extra_vars.add_variable(
            "delayed_verifier_batch_size",
            &delayed_verifier_batch_size.to_string(),
        );
    }
    if let Some(delayed_verifier_quorum_value) = &options.delayed_verifier_quorum_value {
        extra_vars.add_variable(
            "delayed_verifier_quorum_value",
            delayed_verifier_quorum_value,
        );
    }
    if let Some(performance_verifier_batch_size) = options.performance_verifier_batch_size {
        extra_vars.add_variable(
            "performance_verifier_batch_size",
            &performance_verifier_batch_size.to_string(),
        );
    }
    if let Some(random_verifier_batch_size) = options.random_verifier_batch_size {
        extra_vars.add_variable(
            "random_verifier_batch_size",
            &random_verifier_batch_size.to_string(),
        );
    }
    if let Some(sleep_duration) = options.sleep_duration {
        extra_vars.add_variable("sleep_duration", &sleep_duration.to_string());
    }

    Ok(extra_vars.build())
}

pub fn build_data_retrieval_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    network_contacts_url: Option<String>,
) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    if let Some(network_contacts_url) = network_contacts_url {
        extra_vars.add_variable("network_contacts_url", &network_contacts_url);
    }

    extra_vars.add_ant_url_or_version(
        &options.name,
        &options.binary_option,
        options.ant_version.clone(),
    )?;

    extra_vars.add_boolean_variable("start_data_retrieval", options.start_data_retrieval);
    extra_vars.add_boolean_variable("enable_metrics", options.enable_metrics);

    extra_vars.add_variable("evm_network_type", &options.evm_network.to_string());
    if let Some(evm_data_payment_token_address) = &options.evm_data_payments_address {
        extra_vars.add_variable("evm_data_payments_address", evm_data_payment_token_address);
    }
    if let Some(evm_payment_token_address) = &options.evm_payment_token_address {
        extra_vars.add_variable("evm_payment_token_address", evm_payment_token_address);
    }
    if let Some(evm_rpc_url) = &options.evm_rpc_url {
        extra_vars.add_variable("evm_rpc_url", evm_rpc_url);
    }
    if let Some(network_id) = options.network_id {
        extra_vars.add_variable("network_id", &network_id.to_string());
    }
    if let Some(client_env_variables) = &options.client_env_variables {
        extra_vars.add_env_variable_list("client_env_variables", client_env_variables.clone());
    }

    Ok(extra_vars.build())
}

pub fn build_clients_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    peer: Option<String>,
    network_contacts_url: Option<String>,
    sk_map: &HashMap<VirtualMachine, Vec<PrivateKeySigner>>,
    client_vms: &[VirtualMachine],
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    if let Some(peer) = peer {
        extra_vars.add_variable("peer", &peer);
    }
    if let Some(network_contacts_url) = network_contacts_url {
        extra_vars.add_variable("network_contacts_url", &network_contacts_url);
    }

    if let Some(branch) = &options.network_dashboard_branch {
        extra_vars.add_variable("network_dashboard_branch", branch);
    }

    extra_vars.add_ant_url_or_version(
        &options.name,
        &options.binary_option,
        options.ant_version.clone(),
    )?;
    extra_vars.add_variable(
        "ant_uploader_instances",
        &options.uploaders_count.unwrap_or(1).to_string(),
    );
    extra_vars.add_variable("evm_network_type", &options.evm_network.to_string());
    if let Some(evm_data_payment_token_address) = &options.evm_data_payments_address {
        extra_vars.add_variable("evm_data_payments_address", evm_data_payment_token_address);
    }
    if let Some(evm_payment_token_address) = &options.evm_payment_token_address {
        extra_vars.add_variable("evm_payment_token_address", evm_payment_token_address);
    }
    if let Some(evm_rpc_url) = &options.evm_rpc_url {
        extra_vars.add_variable("evm_rpc_url", evm_rpc_url);
    }
    if let Some(network_id) = options.network_id {
        extra_vars.add_variable("network_id", &network_id.to_string());
    }
    if let Some(client_env_variables) = &options.client_env_variables {
        extra_vars.add_env_variable_list("client_env_variables", client_env_variables.clone());
    }

    extra_vars.add_boolean_variable("enable_metrics", options.enable_metrics);
    extra_vars.add_boolean_variable("start_uploaders", options.start_uploaders);

    let mut serde_map = serde_json::Map::new();
    for (k, v) in sk_map {
        let sks = v
            .iter()
            .map(|sk| sk.to_bytes().encode_hex_with_prefix())
            .collect::<Vec<String>>();
        let sks = Value::Array(sks.into_iter().map(Value::String).collect());
        serde_map.insert(k.name.clone(), sks);
    }
    let serde_map = Value::Object(serde_map);
    extra_vars.add_serde_value("ant_secret_key_map", serde_map);

    // If the key map is not empty, also pass the first value in the map as the `secret_key`
    // variable. This is useful for certain cases where there are many machines all using the same
    // key, such as with a scan repair.
    if !sk_map.is_empty() {
        if let Some((_, private_key_signers)) = sk_map.iter().next() {
            if let Some(first_signer) = private_key_signers.first() {
                let secret_key_hex = first_signer.to_bytes().encode_hex_with_prefix();
                extra_vars.add_variable("secret_key", &secret_key_hex);
            }
        }
    }

    if let Some(max_uploads) = options.max_uploads {
        extra_vars.add_variable("max_uploads", &max_uploads.to_string());
    }

    extra_vars.add_variable(
        "upload_batch_size",
        &options.upload_batch_size.unwrap_or(16).to_string(),
    );
    extra_vars.add_variable(
        "upload_size",
        &options.upload_size.unwrap_or(100).to_string(),
    );
    extra_vars.add_variable(
        "upload_interval",
        &options.upload_interval.unwrap_or(10).to_string(),
    );
    extra_vars.add_boolean_variable("single_node_payments", options.single_node_payment);

    extra_vars.add_variable(
        "chunk_tracker_instances",
        &options.chunk_tracker_services.unwrap_or(1).to_string(),
    );
    extra_vars.add_variable(
        "start_chunk_trackers",
        &options.start_chunk_trackers.to_string(),
    );

    extra_vars.add_variable(
        "repair_service_count",
        &options.repair_service_count.to_string(),
    );

    if let Some(scan_frequency) = options.scan_frequency {
        extra_vars.add_variable("scan_frequency", &scan_frequency.to_string());
    }

    // Create a map of chunk tracker addresses similar to the secret key map for uploaders.
    let mut chunk_tracker_address_map = serde_json::Map::new();
    if let Some(addresses) = &options.chunk_tracker_data_addresses {
        if !addresses.is_empty() {
            // Each VM gets the same list of addresses (similar to how uploaders work)
            // The number of addresses should match chunk_tracker_instances
            for vm in client_vms {
                let addresses_array = Value::Array(
                    addresses
                        .iter()
                        .map(|addr| Value::String(addr.clone()))
                        .collect(),
                );
                chunk_tracker_address_map.insert(vm.name.clone(), addresses_array);
            }
        }
    }
    let chunk_tracker_address_map = Value::Object(chunk_tracker_address_map);
    extra_vars.add_serde_value("chunk_tracker_data_address_map", chunk_tracker_address_map);

    Ok(extra_vars.build())
}

pub fn build_start_or_stop_uploader_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    skip_err: bool,
) -> String {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    extra_vars.add_variable(
        "ant_uploader_instances",
        &options.uploaders_count.unwrap_or(1).to_string(),
    );
    extra_vars.add_variable("skip_err", &skip_err.to_string());
    extra_vars.build()
}

pub fn build_binaries_extra_vars_doc(options: &ProvisionOptions) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_build_variables(&options.name, &options.binary_option);
    if let Some(chunk_size) = options.chunk_size {
        extra_vars.add_variable("chunk_size", &chunk_size.to_string());
    }
    Ok(extra_vars.build())
}

pub fn build_node_telegraf_upgrade(name: &str, node_type: &NodeType) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    extra_vars.add_variable("node_type", node_type.telegraf_role());
    Ok(extra_vars.build())
}

pub fn build_client_telegraf_upgrade(name: &str) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    Ok(extra_vars.build())
}

pub fn build_evm_nodes_extra_vars_doc(
    name: &str,
    cloud_provider: &CloudProvider,
    binary_options: &BinaryOption,
) -> String {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    extra_vars.add_variable("provider", &cloud_provider.to_string());

    if let BinaryOption::BuildFromSource {
        branch, repo_owner, ..
    } = binary_options
    {
        extra_vars.add_variable("org", repo_owner);
        extra_vars.add_variable("branch", branch);
    }

    extra_vars.build()
}
