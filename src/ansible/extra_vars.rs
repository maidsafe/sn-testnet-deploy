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
                        "{}/{}/{}/antnode_rpc_client-{}-x86_64-unknown-linux-musl.tar.gz",
                        BRANCH_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            _ => {
                self.add_variable(
                    "antnode_rpc_client_archive_url",
                    &format!(
                        "{}/antnode_rpc_client-latest-x86_64-unknown-linux-musl.tar.gz",
                        RPC_CLIENT_BUCKET_URL
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
                        "{}/{}/{}/antnode-{}-x86_64-unknown-linux-musl.tar.gz",
                        BRANCH_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned {
                antnode_version, ..
            } => {
                let _ = self.add_variable("version", &antnode_version.to_string());
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
                        "{}/{}/{}/antctl-{}-x86_64-unknown-linux-musl.tar.gz",
                        BRANCH_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned { antctl_version, .. } => {
                self.add_variable(
                    "antctl_archive_url",
                    &format!(
                        "{}/antctl-{}-x86_64-unknown-linux-musl.tar.gz",
                        ANTCTL_S3_BUCKET_URL, antctl_version
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
                        "{}/{}/{}/antctld-{}-x86_64-unknown-linux-musl.tar.gz",
                        BRANCH_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned { antctl_version, .. } => {
                self.add_variable(
                    "antctld_archive_url",
                    &format!(
                        "{}/antctld-{}-x86_64-unknown-linux-musl.tar.gz",
                        ANTCTL_S3_BUCKET_URL, antctl_version
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
        // This applies when upscaling the uploaders.
        // In that scenario, the safe version in the binary option is not set to the correct value
        // because it is not recorded in the inventory.
        if let Some(version) = ant_version {
            self.add_variable(
                "ant_archive_url",
                &format!(
                    "{}/ant-{}-x86_64-unknown-linux-musl.tar.gz",
                    ANT_S3_BUCKET_URL, version
                ),
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
                        "{}/{}/{}/ant-{}-x86_64-unknown-linux-musl.tar.gz",
                        BRANCH_S3_BUCKET_URL, repo_owner, branch, deployment_name
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
                            "{}/ant-{}-x86_64-unknown-linux-musl.tar.gz",
                            ANT_S3_BUCKET_URL, version
                        ),
                    );
                    Ok(())
                }
                None => Err(Error::NoUploadersError),
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
    relay: bool,
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
    extra_vars.add_variable("interval", &options.interval.as_millis().to_string());
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

    if relay {
        extra_vars.add_variable("relay", "true");
    }

    if let Some(network_id) = options.network_id {
        extra_vars.add_variable("network_id", &network_id.to_string());
    }

    extra_vars.add_boolean_variable("enable_telegraf", options.enable_telegraf);

    extra_vars.add_node_url_or_version(&options.name, &options.binary_option);
    extra_vars.add_antctl_url(&options.name, &options.binary_option);
    extra_vars.add_antctld_url(&options.name, &options.binary_option);

    if let Some(env_vars) = &options.node_env_variables {
        extra_vars.add_env_variable_list("node_env_variables", env_vars.clone());
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

    extra_vars.add_variable("rewards_address", &options.rewards_address);
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

pub fn build_uploaders_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    genesis_multiaddr: Option<String>,
    genesis_network_contacts_url: Option<String>,
    sk_map: &HashMap<VirtualMachine, Vec<PrivateKeySigner>>,
) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    if let Some(genesis_multiaddr) = genesis_multiaddr {
        extra_vars.add_variable("genesis_multiaddr", &genesis_multiaddr);
    }
    if let Some(network_contacts_url) = genesis_network_contacts_url {
        extra_vars.add_variable("network_contacts_url", &network_contacts_url);
    }

    extra_vars.add_variable(
        "safe_downloader_instances",
        &options.downloaders_count.to_string(),
    );
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

    extra_vars.add_variable("enable_telegraf", &options.enable_telegraf.to_string());

    let mut serde_map = serde_json::Map::new();
    for (k, v) in sk_map {
        let sks = v
            .iter()
            .map(|sk| format!("{:?}", sk.to_bytes().encode_hex_with_prefix()))
            .collect::<Vec<String>>();
        let sks = Value::Array(sks.into_iter().map(Value::String).collect());
        serde_map.insert(k.name.clone(), sks);
    }
    let serde_map = Value::Object(serde_map);

    extra_vars.add_serde_value("ant_secret_key_map", serde_map);

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

pub fn build_uploader_telegraf_upgrade(name: &str) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    Ok(extra_vars.build())
}

pub fn build_evm_nodes_extra_vars_doc(name: &str, cloud_provider: &CloudProvider) -> String {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    extra_vars.add_variable("provider", &cloud_provider.to_string());
    extra_vars.build()
}
