// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::run_external_command;
use log::debug;
#[cfg(test)]
use mockall::automock;
use std::net::IpAddr;
use std::path::PathBuf;

/// Provides an interface for using the SSH client.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// ssh process.
#[cfg_attr(test, automock)]
pub trait SshClientInterface: Send + Sync {
    fn get_private_key_path(&self) -> PathBuf;
    fn wait_for_ssh_availability(&self, ip_address: &IpAddr, user: &str) -> Result<()>;
    fn run_command(
        &self,
        ip_address: &IpAddr,
        user: &str,
        command: &str,
        suppress_output: bool,
    ) -> Result<Vec<String>>;
    fn run_script(
        &self,
        ip_address: IpAddr,
        user: &str,
        script: PathBuf,
        suppress_output: bool,
    ) -> Result<Vec<String>>;
    fn clone_box(&self) -> Box<dyn SshClientInterface>;
}

#[derive(Clone)]
pub struct SshClient {
    pub private_key_path: PathBuf,
}
impl SshClient {
    pub fn new(private_key_path: PathBuf) -> SshClient {
        SshClient { private_key_path }
    }
}
impl SshClientInterface for SshClient {
    fn get_private_key_path(&self) -> PathBuf {
        self.private_key_path.clone()
    }

    fn wait_for_ssh_availability(&self, ip_address: &IpAddr, user: &str) -> Result<()> {
        println!("Checking for SSH availability at {ip_address}...");
        let mut retries = 0;
        let max_retries = 10;
        while retries < max_retries {
            let result = run_external_command(
                PathBuf::from("ssh"),
                std::env::current_dir()?,
                vec![
                    "-i".to_string(),
                    self.private_key_path.to_string_lossy().to_string(),
                    "-q".to_string(),
                    "-o".to_string(),
                    "BatchMode=yes".to_string(),
                    "-o".to_string(),
                    "ConnectTimeout=5".to_string(),
                    "-o".to_string(),
                    "StrictHostKeyChecking=no".to_string(),
                    format!("{user}@{ip_address}"),
                    "bash".to_string(),
                    "--version".to_string(),
                ],
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

    fn run_command(
        &self,
        ip_address: &IpAddr,
        user: &str,
        command: &str,
        suppress_output: bool,
    ) -> Result<Vec<String>> {
        debug!(
            "Running command '{}' on {}@{}...",
            command, user, ip_address
        );

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
            format!("{}@{}", user, ip_address),
        ];
        args.extend(command_args);

        let output = run_external_command(
            PathBuf::from("ssh"),
            std::env::current_dir()?,
            args,
            suppress_output,
        )
        .map_err(|_| Error::SshCommandFailed(command.to_string()))?;
        Ok(output)
    }

    fn run_script(
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
        )
        .map_err(|e| {
            Error::SshCommandFailed(format!("Failed to execute command on remote host: {e}"))
        })?;
        Ok(output)
    }

    fn clone_box(&self) -> Box<dyn SshClientInterface> {
        Box::new(self.clone())
    }
}
