// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.
use crate::{
    error::{Error, Result},
    is_binary_on_path, run_external_command, CloudProvider,
};
use log::{debug, warn};
use serde::Deserialize;
use std::{collections::HashMap, net::IpAddr, path::PathBuf, time::Duration};

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
    ) -> AnsibleRunner {
        AnsibleRunner {
            provider,
            working_directory_path,
            ssh_sk_path,
            vault_password_file_path,
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
