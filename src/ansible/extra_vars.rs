// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.
use crate::NodeType;
use crate::{ansible::provisioning::ProvisionOptions, CloudProvider};
use crate::{BinaryOption, Error, EvmCustomTestnetData, Result};
use std::collections::HashMap;

const NODE_S3_BUCKET_URL: &str = "https://sn-node.s3.eu-west-2.amazonaws.com";
const NODE_MANAGER_S3_BUCKET_URL: &str = "https://sn-node-manager.s3.eu-west-2.amazonaws.com";
const RPC_CLIENT_BUCKET_URL: &str = "https://sn-node-rpc-client.s3.eu-west-2.amazonaws.com";
const SAFE_S3_BUCKET_URL: &str = "https://sn-cli.s3.eu-west-2.amazonaws.com";

#[derive(Default)]
pub struct ExtraVarsDocBuilder {
    variables: Vec<(String, String)>,
    list_variables: HashMap<String, Vec<String>>,
}

impl ExtraVarsDocBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_variable(&mut self, name: &str, value: &str) -> &mut Self {
        self.variables.push((name.to_string(), value.to_string()));
        self
    }

    pub fn add_list_variable(&mut self, name: &str, values: Vec<String>) -> &mut Self {
        if let Some(list) = self.list_variables.get_mut(name) {
            list.extend(values);
        } else {
            self.list_variables.insert(name.to_string(), values);
        }
        self
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
        self.variables
            .push((name.to_string(), joined_env_vars.to_string()));
        self
    }

    pub fn add_build_variables(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner,
                branch,
                safenode_features,
                protocol_version,
                network_keys,
            } => {
                self.add_variable("custom_bin", "true");
                self.add_variable("testnet_name", deployment_name);
                self.add_variable("org", repo_owner);
                self.add_variable("branch", branch);
                if let Some(features) = safenode_features {
                    self.add_variable("safenode_features_list", features);
                }
                if let Some(protocol_version) = protocol_version {
                    self.add_variable("protocol_version", protocol_version);
                }
                if let Some(network_keys) = network_keys {
                    self.add_variable("foundation_pk", &network_keys.0);
                    self.add_variable("genesis_pk", &network_keys.1);
                    self.add_variable("network_royalties_pk", &network_keys.2);
                    self.add_variable("payment_forward_pk", &network_keys.3);
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
                    "safenode_rpc_client_archive_url",
                    &format!(
                        "{}/{}/{}/safenode_rpc_client-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            _ => {
                self.add_variable(
                    "safenode_rpc_client_archive_url",
                    &format!(
                        "{}/safenode_rpc_client-latest-x86_64-unknown-linux-musl.tar.gz",
                        RPC_CLIENT_BUCKET_URL
                    ),
                );
            }
        }
    }

    pub fn add_faucet_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
    ) -> Result<()> {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "faucet_archive_url",
                    &format!(
                        "{}/{}/{}/faucet-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
                Ok(())
            }
            BinaryOption::Versioned { faucet_version, .. } => match faucet_version {
                Some(version) => {
                    self.variables
                        .push(("version".to_string(), version.to_string()));
                    Ok(())
                }
                None => Err(Error::NoFaucetError),
            },
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
                        "{}/{}/{}/safenode-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned {
                safenode_version, ..
            } => self
                .variables
                .push(("version".to_string(), safenode_version.to_string())),
        }
    }

    pub fn add_node_manager_url(&mut self, deployment_name: &str, binary_option: &BinaryOption) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "node_manager_archive_url",
                    &format!(
                        "{}/{}/{}/safenode-manager-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            BinaryOption::Versioned {
                safenode_manager_version,
                ..
            } => {
                self.variables.push((
                    "node_manager_archive_url".to_string(),
                    format!(
                        "{}/safenode-manager-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_MANAGER_S3_BUCKET_URL, safenode_manager_version
                    ),
                ));
            }
        }
    }

    pub fn add_node_manager_daemon_url(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
    ) {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "safenodemand_archive_url",
                    &format!(
                        "{}/{}/{}/safenodemand-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
            }
            _ => {
                self.variables.push((
                    "safenodemand_archive_url".to_string(),
                    format!(
                        "{}/safenodemand-latest-x86_64-unknown-linux-musl.tar.gz",
                        NODE_MANAGER_S3_BUCKET_URL,
                    ),
                ));
            }
        }
    }

    pub fn add_autonomi_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
        safe_version: Option<String>,
    ) -> Result<(), Error> {
        // This applies when upscaling the uploaders.
        // In that scenario, the safe version in the binary option is not set to the correct value
        // because it is not recorded in the inventory.
        if let Some(version) = safe_version {
            self.variables.push((
                "autonomi_archive_url".to_string(),
                format!(
                    "{}/autonomi-{}-x86_64-unknown-linux-musl.tar.gz",
                    SAFE_S3_BUCKET_URL, version
                ),
            ));
            return Ok(());
        }

        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "autonomi_archive_url",
                    &format!(
                        "{}/{}/{}/autonomi-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
                Ok(())
            }
            BinaryOption::Versioned { safe_version, .. } => match safe_version {
                Some(version) => {
                    self.variables.push((
                        "autonomi_archive_url".to_string(),
                        format!(
                            "{}/autonomi-{}-x86_64-unknown-linux-musl.tar.gz",
                            SAFE_S3_BUCKET_URL, version
                        ),
                    ));
                    Ok(())
                }
                None => Err(Error::NoUploadersError),
            },
        }
    }

    pub fn build(&self) -> String {
        if self.variables.is_empty() && self.list_variables.is_empty() {
            return "{}".to_string();
        }

        let mut doc = String::new();
        doc.push_str("{ ");

        for (name, value) in self.variables.iter() {
            doc.push_str(&format!("\"{name}\": \"{value}\", "));
        }
        for (name, list) in &self.list_variables {
            doc.push_str(&format!("\"{name}\": ["));
            for val in list.iter() {
                doc.push_str(&format!("\"{val}\", "));
            }
            let mut temp_doc = doc.strip_suffix(", ").unwrap().to_string();
            temp_doc.push_str("], ");
            doc = temp_doc;
        }

        let mut doc = doc.strip_suffix(", ").unwrap().to_string();
        doc.push_str(" }");
        doc
    }

    fn add_branch_url_variable(&mut self, name: &str, value: &str, branch: &str, repo_owner: &str) {
        self.variables
            .push(("branch".to_string(), branch.to_string()));
        self.variables
            .push(("org".to_string(), repo_owner.to_string()));
        self.variables.push((name.to_string(), value.to_string()));
    }
}

pub fn build_nat_gateway_extra_vars_doc(name: &str, private_ips: Vec<String>) -> String {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("testnet_name", name);
    extra_vars.add_list_variable("node_private_ips_eth1", private_ips);
    extra_vars.build()
}

pub fn build_node_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    node_type: NodeType,
    bootstrap_multiaddr: Option<String>,
    node_instance_count: u16,
    evm_testnet_data: Option<EvmCustomTestnetData>,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
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
    extra_vars.add_variable(
        "max_archived_log_files",
        &options.max_archived_log_files.to_string(),
    );
    extra_vars.add_variable("max_log_files", &options.max_log_files.to_string());
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

    extra_vars.add_variable("rewards_address", &options.rewards_address);
    if let Some(evm_data) = evm_testnet_data {
        extra_vars.add_variable("evm_network_type", "evm-custom");
        extra_vars.add_variable("evm_rpc_url", &evm_data.rpc_url);
        extra_vars.add_variable("evm_payment_token_address", &evm_data.payment_token_address);
        extra_vars.add_variable("evm_data_payments_address", &evm_data.data_payments_address);
    } else {
        extra_vars.add_variable("evm_network_type", "evm-arbitrum-one");
    }

    Ok(extra_vars.build())
}

pub fn build_faucet_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    genesis_multiaddr: &str,
    stop: bool,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
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

pub fn build_safenode_rpc_client_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    genesis_multiaddr: &str,
) -> Result<String> {
    let mut extra_vars = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
    extra_vars.add_rpc_client_url_or_version(&options.name, &options.binary_option);
    Ok(extra_vars.build())
}

pub fn build_uploaders_extra_vars_doc(
    cloud_provider: &str,
    options: &ProvisionOptions,
    genesis_multiaddr: &str,
    evm_testnet_data: Option<EvmCustomTestnetData>,
) -> Result<String> {
    let mut extra_vars: ExtraVarsDocBuilder = ExtraVarsDocBuilder::default();
    extra_vars.add_variable("provider", cloud_provider);
    extra_vars.add_variable("testnet_name", &options.name);
    extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
    extra_vars.add_variable(
        "safe_downloader_instances",
        &options.downloaders_count.to_string(),
    );
    extra_vars.add_autonomi_url_or_version(
        &options.name,
        &options.binary_option,
        options.safe_version.clone(),
    )?;
    extra_vars.add_variable(
        "autonomi_uploader_instances",
        &options.uploaders_count.unwrap_or(1).to_string(),
    );
    if let Some(evm_testnet_data) = evm_testnet_data {
        extra_vars.add_variable("evm_rpc_url", &evm_testnet_data.rpc_url);
        extra_vars.add_variable(
            "evm_payment_token_address",
            &evm_testnet_data.payment_token_address,
        );
        extra_vars.add_variable(
            "evm_data_payments_address",
            &evm_testnet_data.data_payments_address,
        );
        extra_vars.add_variable(
            "autonomi_secret_key",
            &evm_testnet_data.deployer_wallet_private_key,
        );
    }
    Ok(extra_vars.build())
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
    extra_vars.add_variable("node_type", node_type.telegraph_role());
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
