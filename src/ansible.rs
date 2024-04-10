// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.
use crate::{
    error::{Error, Result},
    is_binary_on_path, run_external_command, CloudProvider, SnCodebaseType,
};
use log::{debug, warn};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

const NODE_S3_BUCKET_URL: &str = "https://sn-node.s3.eu-west-2.amazonaws.com";
const NODE_MANAGER_S3_BUCKET_URL: &str = "https://sn-node-manager.s3.eu-west-2.amazonaws.com";
const FAUCET_S3_BUCKET_URL: &str = "https://sn-faucet.s3.eu-west-2.amazonaws.com";
const RPC_CLIENT_BUCKET_URL: &str = "https://sn-node-rpc-client.s3.eu-west-2.amazonaws.com";

/// Ansible has multiple 'binaries', e.g., `ansible-playbook`, `ansible-inventory` etc. that are
/// wrappers around the main `ansible` program. It would be a bit cumbersome to create a different
/// runner for all of them, so we can just use this enum to control which program to run.
///
/// Ansible is a Python program, so strictly speaking these are not binaries, but we still use them
/// like a program.
pub enum AnsibleBinary {
    AnsiblePlaybook,
    AnsibleInventory,
    Ansible,
}

impl std::fmt::Display for AnsibleBinary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnsibleBinary::AnsiblePlaybook => write!(f, "ansible-playbook"),
            AnsibleBinary::AnsibleInventory => write!(f, "ansible-inventory"),
            AnsibleBinary::Ansible => write!(f, "ansible"),
        }
    }
}

impl AnsibleBinary {
    pub fn get_binary_path(&self) -> Result<PathBuf> {
        let bin_name = self.to_string();
        if !is_binary_on_path(&bin_name) {
            return Err(Error::ToolBinaryNotFound(bin_name));
        }
        Ok(PathBuf::from(bin_name.clone()))
    }
}

pub struct AnsibleRunner {
    pub provider: CloudProvider,
    pub working_directory_path: PathBuf,
    pub ssh_sk_path: PathBuf,
    pub vault_password_file_path: PathBuf,
    pub verbose_mode: bool,
}

// The following three structs are utilities that are used to parse the output of the
// `ansible-inventory` command.
#[derive(Debug, Deserialize)]
struct HostVars {
    ansible_host: IpAddr,
}
#[derive(Debug, Deserialize)]
struct Meta {
    hostvars: HashMap<String, HostVars>,
}
#[derive(Debug, Deserialize)]
struct Output {
    _meta: Meta,
}

impl AnsibleRunner {
    pub fn new(
        working_directory_path: PathBuf,
        provider: CloudProvider,
        ssh_sk_path: PathBuf,
        vault_password_file_path: PathBuf,
        verbose_mode: bool,
    ) -> AnsibleRunner {
        AnsibleRunner {
            provider,
            working_directory_path,
            ssh_sk_path,
            vault_password_file_path,
            verbose_mode,
        }
    }

    // This function is used to list the inventory of the ansible runner.
    // It takes a PathBuf as an argument which represents the inventory path.
    // It returns a Result containing a vector of tuples. Each tuple contains a string representing the name and the ansible host.
    //
    // Set re_attempt to re-run the ansible runner if the inventory list is empty
    pub async fn inventory_list(
        &self,
        inventory_path: PathBuf,
        re_attempt: bool,
    ) -> Result<Vec<(String, IpAddr)>> {
        // Run the external command and store the output.
        let retry_count = if re_attempt { 3 } else { 0 };
        let mut count = 0;
        let mut inventory = Vec::new();

        while count <= retry_count {
            debug!("Running inventory list. retry attempts {count}/{retry_count}");
            let output = run_external_command(
                AnsibleBinary::AnsibleInventory.get_binary_path()?,
                self.working_directory_path.clone(),
                vec![
                    "--inventory".to_string(),
                    inventory_path.to_string_lossy().to_string(),
                    "--list".to_string(),
                ],
                true,
                false,
            )?;

            // Debug the output of the inventory list.
            debug!("Inventory list output:");
            debug!("{output:#?}");
            // Convert the output into a string and remove any lines that do not start with '{'.
            let mut output_string = output
                .into_iter()
                .skip_while(|line| !line.starts_with('{'))
                .collect::<Vec<String>>()
                .join("\n");
            // Truncate the string at the last '}' character.
            if let Some(end_index) = output_string.rfind('}') {
                output_string.truncate(end_index + 1);
            }
            // Parse the output string into the Output struct.
            let parsed: Output = serde_json::from_str(&output_string)?;
            // Convert the parsed output into a vector of tuples containing the name and ansible host.
            inventory = parsed
                ._meta
                .hostvars
                .into_iter()
                .map(|(name, vars)| (name, vars.ansible_host))
                .collect();

            count += 1;
            if !inventory.is_empty() {
                break;
            }
            debug!("Inventory list is empty, re-running after a few seconds.");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        if inventory.is_empty() {
            warn!("Inventory list is empty after {retry_count} retries");
        }

        // Return the inventory.
        Ok(inventory)
    }

    pub fn run_playbook(
        &self,
        playbook_path: PathBuf,
        inventory_path: PathBuf,
        user: String,
        extra_vars_document: Option<String>,
    ) -> Result<()> {
        // Using `to_string_lossy` will suffice here. With `to_str` returning an `Option`, to avoid
        // unwrapping you would need to `ok_or_else` on every path, and maybe even introduce a new
        // error variant, which is very cumbersome. These paths are extremely unlikely to have any
        // unicode characters in them.
        let mut args = vec![
            "--inventory".to_string(),
            inventory_path.to_string_lossy().to_string(),
            "--private-key".to_string(),
            self.ssh_sk_path.to_string_lossy().to_string(),
            "--user".to_string(),
            user,
            "--vault-password-file".to_string(),
            self.vault_password_file_path.to_string_lossy().to_string(),
        ];
        if let Some(extra_vars) = extra_vars_document {
            args.push("--extra-vars".to_string());
            args.push(extra_vars);
        }
        if self.verbose_mode {
            args.push("-v".to_string());
        }
        args.push(playbook_path.to_string_lossy().to_string());
        run_external_command(
            PathBuf::from(AnsibleBinary::AnsiblePlaybook.to_string()),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }
}

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

    pub fn add_build_variables(&mut self, deployment_name: &str, codebase_type: &SnCodebaseType) {
        match codebase_type {
            SnCodebaseType::Main { safenode_features } => {
                if let Some(features) = safenode_features {
                    self.add_variable("custom_bin", "true");
                    self.add_variable("testnet_name", deployment_name);
                    self.add_variable("org", "maidsafe");
                    self.add_variable("branch", "main");
                    self.add_variable("safenode_features_list", features);
                } else {
                    self.add_variable("custom_bin", "false");
                }
            }
            SnCodebaseType::Branch {
                repo_owner,
                branch,
                safenode_features,
            } => {
                self.add_variable("custom_bin", "true");
                self.add_variable("testnet_name", deployment_name);
                self.add_variable("org", repo_owner);
                self.add_variable("branch", branch);
                if let Some(features) = safenode_features {
                    self.add_variable("safenode_features_list", features);
                }
            }
            SnCodebaseType::Versioned { .. } => {
                self.add_variable("custom_bin", "false");
            }
        }
    }

    pub fn add_rpc_client_url_or_version(
        &mut self,
        deployment_name: &str,
        codebase_type: &SnCodebaseType,
    ) {
        match codebase_type {
            SnCodebaseType::Branch {
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
        codebase_type: &SnCodebaseType,
    ) {
        match codebase_type {
            SnCodebaseType::Main { .. } => {
                self.add_variable(
                    "faucet_archive_url",
                    &format!(
                        "{}/faucet-latest-x86_64-unknown-linux-musl.tar.gz",
                        FAUCET_S3_BUCKET_URL
                    ),
                );
            }
            SnCodebaseType::Branch {
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
            }
            SnCodebaseType::Versioned { faucet_version, .. } => self
                .variables
                .push(("version".to_string(), faucet_version.to_string())),
        }
    }

    pub fn add_node_url_or_version(
        &mut self,
        deployment_name: &str,
        codebase_type: &SnCodebaseType,
    ) {
        match codebase_type {
            SnCodebaseType::Main { safenode_features } => {
                if safenode_features.is_some() {
                    self.variables.push((
                        "node_archive_url".to_string(),
                        format!(
                            "{}/maidsafe/main/safenode-{}-x86_64-unknown-linux-musl.tar.gz",
                            NODE_S3_BUCKET_URL, deployment_name
                        ),
                    ));
                } else {
                    self.variables.push((
                        "node_archive_url".to_string(),
                        format!(
                            "{}/safenode-latest-x86_64-unknown-linux-musl.tar.gz",
                            NODE_S3_BUCKET_URL
                        ),
                    ));
                }
            }
            SnCodebaseType::Branch {
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
            SnCodebaseType::Versioned {
                safenode_version, ..
            } => self
                .variables
                .push(("version".to_string(), safenode_version.to_string())),
        }
    }

    pub fn add_node_manager_url(&mut self, deployment_name: &str, codebase_type: &SnCodebaseType) {
        match codebase_type {
            SnCodebaseType::Main { .. } => {
                self.variables.push((
                    "node_manager_archive_url".to_string(),
                    format!(
                        "{}/safenode-manager-latest-x86_64-unknown-linux-musl.tar.gz",
                        NODE_MANAGER_S3_BUCKET_URL
                    ),
                ));
            }
            SnCodebaseType::Branch {
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
            SnCodebaseType::Versioned {
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
        codebase_type: &SnCodebaseType,
    ) {
        match codebase_type {
            SnCodebaseType::Branch {
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

/// Generates inventory files for a given environment.
///
/// Returns a three-element tuple with the paths of the generated build, genesis, and node
/// inventories, respectively.
pub async fn generate_inventory(
    environment_name: &str,
    base_inventory_path: &Path,
    output_inventory_dir_path: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let mut generated_inventory_paths = vec![];
    let inventory_files = ["build", "genesis", "node"];
    for inventory_type in inventory_files.iter() {
        let src_path = base_inventory_path;
        let dest_path = output_inventory_dir_path.join(format!(
            ".{}_{}_inventory_digital_ocean.yml",
            environment_name, inventory_type
        ));
        if dest_path.is_file() {
            // The inventory has already been generated by a previous run, so just move on.
            generated_inventory_paths.push(dest_path);
            continue;
        }

        let mut contents = std::fs::read_to_string(src_path)?;
        contents = contents.replace("env_value", environment_name);
        contents = contents.replace("type_value", inventory_type);
        std::fs::write(&dest_path, contents)?;
        debug!("Created inventory file at {dest_path:#?}");
        generated_inventory_paths.push(dest_path);
    }

    let mut iter = generated_inventory_paths.into_iter();
    Ok((
        iter.next().unwrap(),
        iter.next().unwrap(),
        iter.next().unwrap(),
    ))
}
