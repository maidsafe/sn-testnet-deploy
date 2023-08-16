// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::run_external_command;
#[cfg(test)]
use mockall::automock;
use std::path::PathBuf;

/// Provides an interface for using the SSH client.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// ssh process.
#[cfg_attr(test, automock)]
pub trait SshClientInterface {
    fn wait_for_ssh_availability(&self, ip_address: &str, user: &str) -> Result<()>;
    fn run_command(&self, ip_address: &str, user: &str, command: &str) -> Result<()>;
}

pub struct SshClient {
    pub private_key_path: PathBuf,
}
impl SshClient {
    pub fn new(private_key_path: PathBuf) -> SshClient {
        SshClient { private_key_path }
    }
}
impl SshClientInterface for SshClient {
    fn wait_for_ssh_availability(&self, ip_address: &str, user: &str) -> Result<()> {
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

    fn run_command(&self, ip_address: &str, user: &str, command: &str) -> Result<()> {
        println!(
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
            "ConnectTimeout=5".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            format!("{}@{}", user, ip_address),
        ];
        args.extend(command_args);

        let result =
            run_external_command(PathBuf::from("ssh"), std::env::current_dir()?, args, false);
        match result {
            Ok(_) => {
                println!("Successfully executed command via SSH.");
                Ok(())
            }
            Err(e) => {
                println!("Failed to execute command: {:?}", e);
                Err(Error::SshCommandFailed(command.to_string()))
            }
        }
    }
}
