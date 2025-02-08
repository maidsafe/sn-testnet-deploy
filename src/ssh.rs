// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::provisioning::PrivateNodeProvisionInventory,
    error::{Error, Result},
    inventory::VirtualMachine,
    run_external_command,
};
use log::debug;
use std::{
    collections::HashMap,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Clone, Debug)]
pub struct RoutedVms {
    full_cone_private_node_nat_gateway_ip_map: HashMap<VirtualMachine, IpAddr>,
    symmetric_private_node_nat_gateway_ip_map: HashMap<VirtualMachine, IpAddr>,
}

impl RoutedVms {
    fn find_symmetric_nat_routed_node(
        &self,
        ip_address: &IpAddr,
    ) -> Option<(&VirtualMachine, &IpAddr)> {
        self.symmetric_private_node_nat_gateway_ip_map
            .iter()
            .find_map(|(private_vm, gateway_ip)| {
                if &private_vm.public_ip_addr == ip_address {
                    Some((private_vm, gateway_ip))
                } else {
                    None
                }
            })
    }

    fn find_full_cone_nat_routed_node(
        &self,
        ip_address: &IpAddr,
    ) -> Option<(&VirtualMachine, &IpAddr)> {
        self.full_cone_private_node_nat_gateway_ip_map
            .iter()
            .find_map(|(private_vm, gateway_ip)| {
                if &private_vm.public_ip_addr == ip_address {
                    Some((private_vm, gateway_ip))
                } else {
                    None
                }
            })
    }
}

#[derive(Clone)]
pub struct SshClient {
    pub private_key_path: PathBuf,
    /// The list of VMs that are routed through a gateway.
    pub routed_vms: Arc<RwLock<Option<RoutedVms>>>,
}
impl SshClient {
    pub fn new(private_key_path: PathBuf) -> SshClient {
        SshClient {
            private_key_path,
            routed_vms: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the list of VMs that are routed through a Full Cone NAT Gateway.
    /// This updates all the copies of the `SshClient` that have been cloned.
    pub fn set_full_cone_nat_routed_vms(
        &self,
        private_node_vms: &[VirtualMachine],
        nat_gateway_vms: &[VirtualMachine],
    ) -> Result<()> {
        let private_node_nat_gateway_map =
            PrivateNodeProvisionInventory::match_private_node_vm_and_gateway_vm(
                private_node_vms,
                nat_gateway_vms,
            )?;
        let full_cone_private_node_nat_gateway_ip_map = private_node_nat_gateway_map
            .into_iter()
            .map(|(private_node_vm, nat_gateway_vm)| {
                (private_node_vm, nat_gateway_vm.public_ip_addr)
            })
            .collect::<HashMap<_, _>>();
        if let Some(routed_vms) = self
            .routed_vms
            .write()
            .map_err(|err| {
                log::error!("Failed to set routed VMs: {err}");
                Error::SshSettingsRwLockError
            })?
            .as_mut()
        {
            routed_vms.full_cone_private_node_nat_gateway_ip_map =
                full_cone_private_node_nat_gateway_ip_map;
        }

        debug!("Full Cone Private Routed VMs have been set.");

        Ok(())
    }

    /// Set the list of VMs that are routed through a Symmetric NAT Gateway.
    /// This updates all the copies of the `SshClient` that have been cloned.
    pub fn set_symmetric_nat_routed_vms(
        &self,
        private_node_vms: &[VirtualMachine],
        nat_gateway_vms: &[VirtualMachine],
    ) -> Result<()> {
        let private_node_nat_gateway_map =
            PrivateNodeProvisionInventory::match_private_node_vm_and_gateway_vm(
                private_node_vms,
                nat_gateway_vms,
            )?;
        let symmetric_private_node_nat_gateway_ip_map = private_node_nat_gateway_map
            .into_iter()
            .map(|(private_node_vm, nat_gateway_vm)| {
                (private_node_vm, nat_gateway_vm.public_ip_addr)
            })
            .collect::<HashMap<_, _>>();
        if let Some(routed_vms) = self
            .routed_vms
            .write()
            .map_err(|err| {
                log::error!("Failed to set routed VMs: {err}");
                Error::SshSettingsRwLockError
            })?
            .as_mut()
        {
            routed_vms.symmetric_private_node_nat_gateway_ip_map =
                symmetric_private_node_nat_gateway_ip_map;
        }

        debug!("Symmetric Private node Routed VMs have been set.");

        Ok(())
    }

    pub fn get_private_key_path(&self) -> PathBuf {
        self.private_key_path.clone()
    }

    pub fn wait_for_ssh_availability(&self, ip_address: &IpAddr, user: &str) -> Result<()> {
        let mut args = vec![
            "-i".to_string(),
            self.private_key_path.to_string_lossy().to_string(),
            "-q".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=5".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
        ];
        let routed_vm_read = self.routed_vms.read().map_err(|err| {
            log::error!("Failed to read routed VMs: {err}");
            Error::SshSettingsRwLockError
        })?;
        if let Some((vm, gateway_ip)) = routed_vm_read
            .as_ref()
            .and_then(|routed_vms| routed_vms.find_symmetric_nat_routed_node(ip_address))
        {
            println!(
                "Checking for SSH availability at {} ({ip_address}) via symmetric NAT gateway {gateway_ip}...",
                vm.private_ip_addr
            );
            args.push("-o".to_string());
            args.push(format!(
                "ProxyCommand=ssh -i {} -W %h:%p {}@{}",
                self.private_key_path.to_string_lossy(),
                user,
                gateway_ip
            ));
            args.push(format!("{user}@{}", vm.private_ip_addr));
        } else if let Some((vm, gateway_ip)) = routed_vm_read
            .as_ref()
            .and_then(|routed_vms| routed_vms.find_full_cone_nat_routed_node(ip_address))
        {
            println!(
                "Checking for SSH availability at {} ({ip_address}) via Full Cone NAT gateway {gateway_ip}...",
                vm.private_ip_addr,
            );
            args.push(format!("{user}@{gateway_ip}"));
        } else {
            println!("Checking for SSH availability at {ip_address}...");
            args.push(format!("{user}@{ip_address}"));
        }
        args.push("bash".to_string());
        args.push("--version".to_string());

        let mut retries = 0;
        let max_retries = 10;
        while retries < max_retries {
            let result = run_external_command(
                PathBuf::from("ssh"),
                std::env::current_dir()?,
                args.clone(),
                false,
                false,
            );
            if result.is_ok() {
                println!("SSH is available.");
                return Ok(());
            } else {
                retries += 1;
                println!("SSH is still unavailable after {retries} attempts.");
                println!("Will sleep for 5 seconds then retry.");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }

        println!("The maximum number of connection retry attempts has been exceeded.");
        Err(Error::SshUnavailable)
    }

    pub fn run_command(
        &self,
        ip_address: &IpAddr,
        user: &str,
        command: &str,
        suppress_output: bool,
    ) -> Result<Vec<String>> {
        let command_args: Vec<String> = command.split_whitespace().map(String::from).collect();
        let mut args = vec![
            "-i".to_string(),
            self.private_key_path.to_string_lossy().to_string(),
            "-q".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=30".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
        ];
        let routed_vm_read = self.routed_vms.read().map_err(|err| {
            log::error!("Failed to read routed VMs: {err}");
            Error::SshSettingsRwLockError
        })?;

        if let Some((vm, gateway)) = routed_vm_read
            .as_ref()
            .and_then(|routed_vms| routed_vms.find_symmetric_nat_routed_node(ip_address))
        {
            debug!(
                "Running command '{}' on {} ({ip_address}) via symmetric NAT gateway {gateway}...",
                command, vm.private_ip_addr
            );
            args.push("-o".to_string());
            args.push(format!(
                "ProxyCommand=ssh -i {} -W %h:%p {user}@{gateway}",
                self.private_key_path.to_string_lossy(),
            ));
            args.push(format!("{user}@{}", vm.private_ip_addr));
        } else if let Some((vm, gateway)) = routed_vm_read
            .as_ref()
            .and_then(|routed_vms| routed_vms.find_full_cone_nat_routed_node(ip_address))
        {
            debug!(
                "Running command '{}' on {} ({ip_address}) via full cone NAT gateway {gateway}...",
                command, vm.private_ip_addr
            );
            args.push(format!("{user}@{gateway}"));
        } else {
            debug!(
                "Running command '{}' on {}@{}...",
                command, user, ip_address
            );
            args.push(format!("{user}@{ip_address}"));
        }
        args.extend(command_args);

        let output = run_external_command(
            PathBuf::from("ssh"),
            std::env::current_dir()?,
            args,
            suppress_output,
            false,
        )?;
        Ok(output)
    }

    pub fn run_script(
        &self,
        ip_address: IpAddr,
        user: &str,
        script: PathBuf,
        suppress_output: bool,
    ) -> Result<Vec<String>> {
        let file_name = script
            .file_name()
            .ok_or_else(|| {
                Error::SshCommandFailed("Could not obtain file name from script path".to_string())
            })?
            .to_string_lossy()
            .to_string();
        let args = vec![
            "-i".to_string(),
            self.private_key_path.to_string_lossy().to_string(),
            "-q".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=30".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            script.to_string_lossy().to_string(),
            format!("{}@{}:/tmp/{}", user, ip_address, file_name),
        ];
        run_external_command(
            PathBuf::from("scp"),
            std::env::current_dir()?,
            args,
            suppress_output,
            false,
        )
        .map_err(|e| {
            Error::SshCommandFailed(format!(
                "Failed to copy script file to remote host {ip_address:?}: {e}"
            ))
        })?;

        let args = vec![
            "-i".to_string(),
            self.private_key_path.to_string_lossy().to_string(),
            "-q".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=30".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            format!("{user}@{ip_address}"),
            "bash".to_string(),
            format!("/tmp/{file_name}"),
        ];
        let output = run_external_command(
            PathBuf::from("ssh"),
            std::env::current_dir()?,
            args,
            suppress_output,
            false,
        )
        .map_err(|e| {
            Error::SshCommandFailed(format!("Failed to execute command on remote host: {e}"))
        })?;
        Ok(output)
    }
}
