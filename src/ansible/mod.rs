// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

pub mod extra_vars;
pub mod inventory;
pub mod provisioning;

use crate::{
    error::{Error, Result},
    is_binary_on_path, run_external_command, CloudProvider,
};
use inventory::AnsibleInventoryType;
use log::debug;
use std::path::PathBuf;

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
    /// This playbook will fund the uploaders using the faucet.
    FundUploaders,
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
    /// The NAT gateway playbook will setup the NAT gateway to enable NAT routing with randomization.
    /// It allows us to simulate a private node that is behind a NAT.
    ///
    /// Use in combination with `AnsibleInventoryType::NatGateway`.
    NatGateway,
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
    /// The private nodes playbook will setup the private nodes on the last node in the inventory.
    ///
    /// Use in combination with `AnsibleInventoryType::Nodes`.
    PrivateNodes,
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
    /// This playbook will start the faucet for the environment.
    StartFaucet,
    /// This playbook will start the Telegraf service on each machine.
    ///
    /// It can be necessary for running upgrades, since we will want to re-enable Telegraf after the
    /// upgrade.
    StartTelegraf,
    /// This playbook will start the uploaders on each machine.
    StartUploaders,
    /// This playbook will stop the faucet for the environment.
    StopFaucet,
    /// This playbook will stop the Telegraf service running on each machine.
    ///
    /// It can be necessary for running upgrades, since Telegraf will run `safenode-manager
    /// status`, which writes to the registry file and can interfere with an upgrade.
    StopTelegraf,
    /// This playbook will stop the uploaders on each machine.
    StopUploaders,
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
    /// Update the node Telegraf configuration to the latest version in the repository.
    UpgradeNodeTelegrafConfig,
    /// Upgrade the uploaders to the latest version of the safe client.
    UpgradeUploaders,
    /// Update the uploader Telegraf configuration to the latest version in the repository.
    UpgradeUploaderTelegrafConfig,
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
            AnsiblePlaybook::FundUploaders => "fund_uploaders.yml".to_string(),
            AnsiblePlaybook::Logs => "logs.yml".to_string(),
            AnsiblePlaybook::Logstash => "logstash.yml".to_string(),
            AnsiblePlaybook::NatGateway => "nat_gateway.yml".to_string(),
            AnsiblePlaybook::NodeManagerInventory => "node_manager_inventory.yml".to_string(),
            AnsiblePlaybook::Nodes => "nodes.yml".to_string(),
            AnsiblePlaybook::PrivateNodes => "private_nodes.yml".to_string(),
            AnsiblePlaybook::RpcClient => "safenode_rpc_client.yml".to_string(),
            AnsiblePlaybook::StartFaucet => "start_faucet.yml".to_string(),
            AnsiblePlaybook::StartNodes => "start_nodes.yml".to_string(),
            AnsiblePlaybook::StartTelegraf => "start_telegraf.yml".to_string(),
            AnsiblePlaybook::StartUploaders => "start_uploaders.yml".to_string(),
            AnsiblePlaybook::Status => "node_status.yml".to_string(),
            AnsiblePlaybook::StopFaucet => "stop_faucet.yml".to_string(),
            AnsiblePlaybook::StopTelegraf => "stop_telegraf.yml".to_string(),
            AnsiblePlaybook::StopUploaders => "stop_uploaders.yml".to_string(),
            AnsiblePlaybook::UpgradeFaucet => "upgrade_faucet.yml".to_string(),
            AnsiblePlaybook::UpgradeNodeManager => "upgrade_node_manager.yml".to_string(),
            AnsiblePlaybook::UpgradeNodes => "upgrade_nodes.yml".to_string(),
            AnsiblePlaybook::UpgradeNodeTelegrafConfig => {
                "upgrade_node_telegraf_config.yml".to_string()
            }
            AnsiblePlaybook::UpgradeUploaders => "upgrade_uploaders.yml".to_string(),
            AnsiblePlaybook::UpgradeUploaderTelegrafConfig => {
                "upgrade_uploader_telegraf_config.yml".to_string()
            }
            AnsiblePlaybook::Uploaders => "uploaders.yml".to_string(),
        }
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

    pub fn run_playbook(
        &self,
        playbook: AnsiblePlaybook,
        mut inventory_type: AnsibleInventoryType,
        extra_vars_document: Option<String>,
    ) -> Result<()> {
        // prioritize the static private node inventory if it exists. Else fall back to the dynamic one.
        if matches!(inventory_type, AnsibleInventoryType::PrivateNodes)
            && self
                .get_inventory_path(&AnsibleInventoryType::PrivateNodesStatic)
                .is_ok()
        {
            println!("Using static private node inventory to run playbook");
            inventory_type = AnsibleInventoryType::PrivateNodesStatic;
        }
        debug!(
            "Running playbook: {:?} on {inventory_type:?}",
            playbook.get_playbook_name()
        );

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
        let path = inventory_type.get_inventory_path(&self.environment_name, provider);
        let path = self.working_directory_path.join("inventory").join(path);
        match path.exists() {
            true => Ok(path),
            false => Err(Error::EnvironmentDoesNotExist(
                self.environment_name.clone(),
            )),
        }
    }
}
