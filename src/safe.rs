// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::run_external_command;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use regex::Regex;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::fs::File as TokioFile;
use tokio::io::AsyncWriteExt;

/// Provides an interface for using the `safe` client.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// safe process.
#[cfg_attr(test, automock)]
pub trait SafeClientInterface {
    fn wallet_get_faucet(&self, peer_multiaddr: &str, faucet_addr: SocketAddr) -> Result<()>;
    fn download_files(&self, peer_multiaddr: &str) -> Result<()>;
    fn upload_file(&self, peer_multiaddr: &str, path: &Path) -> Result<String>;
}

pub struct SafeClient {
    pub binary_path: PathBuf,
    pub working_directory_path: PathBuf,
}
impl SafeClient {
    pub fn new(binary_path: PathBuf, working_directory_path: PathBuf) -> SafeClient {
        SafeClient {
            binary_path,
            working_directory_path,
        }
    }
}

impl SafeClientInterface for SafeClient {
    fn download_files(&self, peer_multiaddr: &str) -> Result<()> {
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "--peer".to_string(),
                peer_multiaddr.to_string(),
                "files".to_string(),
                "download".to_string(),
            ],
            false,
        )?;
        Ok(())
    }

    fn upload_file(&self, peer_multiaddr: &str, path: &Path) -> Result<String> {
        let output = run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "--peer".to_string(),
                peer_multiaddr.to_string(),
                "files".to_string(),
                "upload".to_string(),
                path.to_string_lossy().to_string(),
            ],
            false,
        )?;

        let re = Regex::new(r"Uploaded .+ to ([a-fA-F0-9]+)")?;
        for line in &output {
            if let Some(captures) = re.captures(line) {
                return Ok(captures[1].to_string());
            }
        }

        Err(Error::SafeCmdError(
            "could not obtain hex address of uploaded file".to_string(),
        ))
    }

    fn wallet_get_faucet(&self, peer_multiaddr: &str, faucet_addr: SocketAddr) -> Result<()> {
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "--peer".to_string(),
                peer_multiaddr.to_string(),
                "wallet".to_string(),
                "get-faucet".to_string(),
                faucet_addr.to_string(),
            ],
            false,
        )?;
        Ok(())
    }
}

/// Provides an interface for downloading release binaries for safe or safenode.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// safe process.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait SafeBinaryRepositoryInterface {
    async fn download(&self, binary_archive_url: &str, dest_path: &Path) -> Result<()>;
}

pub struct SafeBinaryRepository;

#[async_trait]
impl SafeBinaryRepositoryInterface for SafeBinaryRepository {
    async fn download(&self, binary_archive_url: &str, dest_path: &Path) -> Result<()> {
        let response = reqwest::get(binary_archive_url).await?;

        if !response.status().is_success() {
            return Err(Error::SafeBinaryDownloadError);
        }

        let mut dest = TokioFile::create(dest_path).await?;
        let content = response.bytes().await?;
        dest.write_all(&content).await?;

        Ok(())
    }
}
