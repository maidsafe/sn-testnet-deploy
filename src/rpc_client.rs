// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::Result;
use crate::run_external_command;
#[cfg(test)]
use mockall::automock;
use std::net::SocketAddr;
use std::path::PathBuf;

pub struct NodeInfo {
    pub endpoint: String,
    pub peer_id: String,
    pub logs_dir: PathBuf,
    pub pid: u16,
    pub safenode_version: String,
    pub last_restart: u32,
}

/// This trait exists for unit testing.
///
/// It allows us to return dummy node info during a test, without making a real RPC call.
#[cfg_attr(test, automock)]
pub trait RpcClientInterface {
    fn get_info(&self, rpc_address: SocketAddr) -> Result<NodeInfo>;
}

pub struct RpcClient {
    pub binary_path: PathBuf,
    pub working_directory_path: PathBuf,
}

impl RpcClient {
    pub fn new(binary_path: PathBuf, working_directory_path: PathBuf) -> RpcClient {
        RpcClient {
            binary_path,
            working_directory_path,
        }
    }
}

impl RpcClientInterface for RpcClient {
    fn get_info(&self, rpc_address: SocketAddr) -> Result<NodeInfo> {
        let output = run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![rpc_address.to_string(), "info".to_string()],
            false,
            false,
        )?;

        let endpoint = output
            .iter()
            .find(|line| line.starts_with("RPC endpoint:"))
            .map(|line| line.split(": ").nth(1).unwrap_or("").to_string())
            .unwrap_or_default();
        let peer_id = output
            .iter()
            .find(|line| line.starts_with("Peer Id:"))
            .map(|line| line.split(": ").nth(1).unwrap_or("").to_string())
            .unwrap_or_default();
        let logs_dir = output
            .iter()
            .find(|line| line.starts_with("Logs dir:"))
            .map(|line| line.split(": ").nth(1).unwrap_or("").to_string())
            .unwrap_or_default();
        let pid = output
            .iter()
            .find(|line| line.starts_with("PID:"))
            .map(|line| {
                line.split(": ")
                    .nth(1)
                    .unwrap_or("")
                    .parse::<u16>()
                    .unwrap_or(0)
            })
            .unwrap_or_default();
        let safenode_version = output
            .iter()
            .find(|line| line.starts_with("Binary version:"))
            .map(|line| line.split(": ").nth(1).unwrap_or("").to_string())
            .unwrap_or_default();
        let last_restart = output
            .iter()
            .find(|line| line.starts_with("Time since last restart:"))
            .map(|line| {
                line.split(": ")
                    .nth(1)
                    .unwrap_or("")
                    .trim_end_matches('s')
                    .parse::<u32>()
                    .unwrap_or(0)
            })
            .unwrap_or_default();

        Ok(NodeInfo {
            endpoint,
            peer_id,
            logs_dir: PathBuf::from(logs_dir),
            pid,
            safenode_version,
            last_restart,
        })
    }
}
