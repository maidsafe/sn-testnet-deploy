// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod extra_vars;
pub mod provisioning;

use crate::{
    error::{Error, Result},
    inventory::VirtualMachine,
    is_binary_on_path, run_external_command, CloudProvider,
};
use log::{debug, warn};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

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

/// Represents the playbooks that apply to our own domain.
pub enum AnsiblePlaybook {
    /// The auditor playbook will provision setup the auditor to run as a service. The auditor is
    /// typically running on a separate auditor machine, but can be run from any machine.
    ///
    /// Use in combination with `AnsibleInventoryType::Auditor` or `AnsibleInventoryType::Nodes`.
    Auditor,
    /// The build playbook will build the `faucet`, `safe`, `safenode` and `safenode-manager`
    /// binaries and upload them to S3.
    ///
    /// Use in combination with `AnsibleInventoryType::Build`.
    Build,
    /// The faucet playbook will provision setup the faucet to run as a service. The faucet is
    /// typically running on the genesis node.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis`.
    Faucet,
    /// The genesis playbook will use the node manager to setup the genesis node, which the other
    /// nodes will bootstrap against.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis`.
    Genesis,
    /// The logs playbook will retrieve node logs from any machines it is run against.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    Logs,
    /// The Logstash playbook will provision machines to run Logstash.
    ///
    /// Use in combination with `AnsibleInventoryType::Logstash`.
    Logstash,
    /// The node manager inventory playbook will retrieve the node manager's inventory from any
    /// machines it is run against.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    NodeManagerInventory,
    /// The node playbook will setup any nodes except the genesis node. These nodes will bootstrap
    /// using genesis as a peer reference.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    Nodes,
    /// The rpc client playbook will setup the `safenode_rpc_client` binary on the genesis node.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis`.
    RpcClient,
    /// The start nodes playbook will use the node manager to start any node services on any
    /// machines it runs against.
    ///
    /// It is useful for starting any nodes that failed to start after they were upgraded. The node
    /// manager's `start` command is idempotent, so it will skip nodes that are already running.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    StartNodes,
    /// Run `safenode-manager status` on the machine.
    ///
    /// Useful to determine the state of all the nodes in a deployment.
    Status,
    /// This playbook will stop the Telegraf service running on each machine.
    ///
    /// It can be necessary for running upgrades, since Telegraf will run `safenode-manager
    /// status`, which writes to the registry file and can interfere with an upgrade.
    StopTelegraf,
    /// The upgrade faucet playbook will upgrade the faucet to the latest version.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis`.
    UpgradeFaucet,
    /// The upgrade node manager playbook will upgrade the node manager to the latest version.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    UpgradeNodeManager,
    /// The upgrade node manager playbook will upgrade node services to the latest version.
    ///
    /// Use in combination with `AnsibleInventoryType::Genesis` or `AnsibleInventoryType::Nodes`.
    UpgradeNodes,
    /// Update the Telegraf configuration to the latest version in the repository.
    UpgradeTelegrafConfig,
    /// The uploader playbook will setup the uploader scripts on the uploader VMs.
    ///
    /// Use in combination with `AnsibleInventoryType::Uploaders`.
    Uploaders,
}

impl AnsiblePlaybook {
    pub fn get_playbook_name(&self) -> String {
        match self {
            AnsiblePlaybook::Auditor => "auditor.yml".to_string(),
            AnsiblePlaybook::Build => "build.yml".to_string(),
            AnsiblePlaybook::Genesis => "genesis_node.yml".to_string(),
            AnsiblePlaybook::Faucet => "faucet.yml".to_string(),
            AnsiblePlaybook::Logs => "logs.yml".to_string(),
            AnsiblePlaybook::Logstash => "logstash.yml".to_string(),
            AnsiblePlaybook::NodeManagerInventory => "node_manager_inventory.yml".to_string(),
            AnsiblePlaybook::Nodes => "nodes.yml".to_string(),
            AnsiblePlaybook::RpcClient => "safenode_rpc_client.yml".to_string(),
            AnsiblePlaybook::StartNodes => "start_nodes.yml".to_string(),
            AnsiblePlaybook::Status => "node_status.yml".to_string(),
            AnsiblePlaybook::StopTelegraf => "stop_telegraf.yml".to_string(),
            AnsiblePlaybook::UpgradeFaucet => "upgrade_faucet.yml".to_string(),
            AnsiblePlaybook::UpgradeNodeManager => "upgrade_node_manager.yml".to_string(),
            AnsiblePlaybook::UpgradeNodes => "upgrade_nodes.yml".to_string(),
            AnsiblePlaybook::UpgradeTelegrafConfig => "upgrade_telegraf_config.yml".to_string(),
            AnsiblePlaybook::Uploaders => "uploaders.yml".to_string(),
        }
    }
}

/// Represents the inventory types that apply to our own domain.
#[derive(Clone)]
pub enum AnsibleInventoryType {
    // Use to run a playbook against the auditor.
    //
    // Only one machine will be returned in this inventory.
    Auditor,
    // Use to run a playbook against all bootstrap nodes.
    BootstrapNodes,
    // Use to run a playbook against the build machine.
    //
    // This is a larger machine that is used for building binaries from source.
    //
    // Only one machine will be returned in this inventory.
    Build,
    // Provide a static list of VMs to connect to.
    Custom,
    // Use to run a playbook against the genesis node.
    //
    // Only one machine will be returned in this inventory.
    Genesis,
    // Use to run a playbook against the Logstash servers.
    Logstash,
    // Use to run a playbook against all nodes except the genesis node.
    Nodes,
    // Use to run a playbook against all the uploader machines.
    Uploaders,
}

impl std::fmt::Display for AnsibleInventoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AnsibleInventoryType::Auditor => "Auditor",
            AnsibleInventoryType::BootstrapNodes => "BootstrapNodes",
            AnsibleInventoryType::Build => "Build",
            AnsibleInventoryType::Custom => "Custom",
            AnsibleInventoryType::Genesis => "Genesis",
            AnsibleInventoryType::Logstash => "Logstash",
            AnsibleInventoryType::Nodes => "Nodes",
            AnsibleInventoryType::Uploaders => "Uploaders",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone)]
pub struct AnsibleRunner {
    pub ansible_forks: usize,
    pub ansible_verbose_mode: bool,
    pub environment_name: String,
    pub provider: CloudProvider,
    pub ssh_sk_path: PathBuf,
    pub vault_password_file_path: PathBuf,
    pub working_directory_path: PathBuf,
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
        ansible_forks: usize,
        ansible_verbose_mode: bool,
        environment_name: &str,
        provider: CloudProvider,
        ssh_sk_path: PathBuf,
        vault_password_file_path: PathBuf,
        working_directory_path: PathBuf,
    ) -> Result<AnsibleRunner> {
        if environment_name.is_empty() {
            return Err(Error::EnvironmentNameRequired);
        }
        Ok(AnsibleRunner {
            ansible_forks,
            ansible_verbose_mode,
            environment_name: environment_name.to_string(),
            provider,
            working_directory_path,
            ssh_sk_path,
            vault_password_file_path,
        })
    }

    /// Runs Ansible's inventory command and returns a list of machine name and IP address pairs.
    pub async fn get_inventory(
        &self,
        inventory_type: AnsibleInventoryType,
        re_attempt: bool,
    ) -> Result<Vec<(String, IpAddr)>> {
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
                    self.get_inventory_path(&inventory_type)?
                        .to_string_lossy()
                        .to_string(),
                    "--list".to_string(),
                ],
                true,
                false,
            )?;

            debug!("Inventory list output:");
            debug!("{output:#?}");
            let mut output_string = output
                .into_iter()
                .skip_while(|line| !line.starts_with('{'))
                .collect::<Vec<String>>()
                .join("\n");
            if let Some(end_index) = output_string.rfind('}') {
                output_string.truncate(end_index + 1);
            }
            let parsed: Output = serde_json::from_str(&output_string)?;
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

        Ok(inventory)
    }

    pub fn run_playbook(
        &self,
        playbook: AnsiblePlaybook,
        inventory_type: AnsibleInventoryType,
        extra_vars_document: Option<String>,
    ) -> Result<()> {
        // Using `to_string_lossy` will suffice here. With `to_str` returning an `Option`, to avoid
        // unwrapping you would need to `ok_or_else` on every path, and maybe even introduce a new
        // error variant, which is very cumbersome. These paths are extremely unlikely to have any
        // unicode characters in them.
        let mut args = vec![
            "--inventory".to_string(),
            self.get_inventory_path(&inventory_type)?
                .to_string_lossy()
                .to_string(),
            "--private-key".to_string(),
            self.ssh_sk_path.to_string_lossy().to_string(),
            "--user".to_string(),
            self.provider.get_ssh_user(),
            "--vault-password-file".to_string(),
            self.vault_password_file_path.to_string_lossy().to_string(),
        ];
        if let Some(extra_vars) = extra_vars_document {
            args.push("--extra-vars".to_string());
            args.push(extra_vars);
        }
        if self.ansible_verbose_mode {
            args.push("-vvvvv".to_string());
        }
        args.push("--forks".to_string());
        args.push(self.ansible_forks.to_string());
        args.push(playbook.get_playbook_name());
        run_external_command(
            PathBuf::from(AnsibleBinary::AnsiblePlaybook.to_string()),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }

    fn get_inventory_path(&self, inventory_type: &AnsibleInventoryType) -> Result<PathBuf> {
        let provider = match self.provider {
            CloudProvider::Aws => "aws",
            CloudProvider::DigitalOcean => "digital_ocean",
        };
        let path = match inventory_type {
            AnsibleInventoryType::Auditor => PathBuf::from("inventory").join(format!(
                ".{}_auditor_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::BootstrapNodes => PathBuf::from("inventory").join(format!(
                ".{}_bootstrap_node_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Build => PathBuf::from("inventory").join(format!(
                ".{}_build_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Custom => PathBuf::from("inventory").join(format!(
                ".{}_custom_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Genesis => PathBuf::from("inventory").join(format!(
                ".{}_genesis_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Logstash => PathBuf::from("inventory").join(format!(
                ".{}_logstash_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Nodes => PathBuf::from("inventory").join(format!(
                ".{}_node_inventory_{}.yml",
                self.environment_name, provider
            )),
            AnsibleInventoryType::Uploaders => PathBuf::from("inventory").join(format!(
                ".{}_uploader_inventory_{}.yml",
                self.environment_name, provider
            )),
        };
        let path = self.working_directory_path.join(path);
        match path.exists() {
            true => Ok(path),
            false => Err(Error::EnvironmentDoesNotExist(
                self.environment_name.clone(),
            )),
        }
    }
}

/// Generate necessary inventory files for a given environment.
///
/// These files are based from a template in the base directory.
///
/// Returns a three-element tuple with the paths of the generated build, genesis, and node
/// inventories, respectively.
pub async fn generate_environment_inventory(
    environment_name: &str,
    base_inventory_path: &Path,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let mut generated_inventory_paths = vec![];
    let inventory_files = [
        "bootstrap_node",
        "build",
        "genesis",
        "node",
        "auditor",
        "uploader",
    ];
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

    Ok(())
}

pub fn generate_custom_environment_inventory(
    vm_list: &[VirtualMachine],
    environment_name: &str,
    output_inventory_dir_path: &Path,
) -> Result<()> {
    let dest_path = output_inventory_dir_path.join(format!(
        ".{}_custom_inventory_digital_ocean.yml",
        environment_name
    ));
    let file = File::create(&dest_path)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "[custom]")?;
    for (_, ip) in vm_list.iter() {
        writeln!(writer, "{}", ip)?;
    }

    debug!("Created custom inventory file at {dest_path:#?}");

    Ok(())
}
