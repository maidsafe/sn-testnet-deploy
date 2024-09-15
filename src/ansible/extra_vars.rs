// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.
use crate::{BinaryOption, Error, Result};
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

    pub fn add_sn_auditor_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
    ) -> Result<(), Error> {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "sn_auditor_archive_url",
                    &format!(
                        "{}/{}/{}/sn_auditor-{}-x86_64-unknown-linux-musl.tar.gz",
                        NODE_S3_BUCKET_URL, repo_owner, branch, deployment_name
                    ),
                    branch,
                    repo_owner,
                );
                Ok(())
            }
            BinaryOption::Versioned {
                sn_auditor_version, ..
            } => match sn_auditor_version {
                Some(version) => {
                    self.variables
                        .push(("version".to_string(), version.to_string()));
                    Ok(())
                }
                None => Err(Error::NoAuditorError),
            },
        }
    }

    pub fn add_safe_url_or_version(
        &mut self,
        deployment_name: &str,
        binary_option: &BinaryOption,
    ) -> Result<(), Error> {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                self.add_branch_url_variable(
                    "safe_archive_url",
                    &format!(
                        "{}/{}/{}/safe-{}-x86_64-unknown-linux-musl.tar.gz",
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
                        "safe_archive_url".to_string(),
                        format!(
                            "{}/safe-{}-x86_64-unknown-linux-musl.tar.gz",
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
            let mut doc = doc.strip_suffix(", ").unwrap().to_string();
            doc.push_str("], ");
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
