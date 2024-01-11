// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    run_external_command,
};
use regex::Regex;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tokio::{fs::File as TokioFile, io::AsyncWriteExt};

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

    pub fn download_files(&self, peer_multiaddr: &str) -> Result<()> {
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
            false,
        )?;
        Ok(())
    }

    pub fn upload_file(&self, peer_multiaddr: &str, path: &Path) -> Result<String> {
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

    pub fn wallet_get_faucet(&self, peer_multiaddr: &str, faucet_addr: SocketAddr) -> Result<()> {
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
            false,
        )?;
        Ok(())
    }
}

pub struct SafeBinaryRepository;

impl SafeBinaryRepository {
    pub async fn download(&self, binary_archive_url: &str, dest_path: &Path) -> Result<()> {
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
