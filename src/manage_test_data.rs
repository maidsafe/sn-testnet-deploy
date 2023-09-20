// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::get_and_extract_archive;
use crate::s3::{S3Repository, S3RepositoryInterface};
use crate::safe::{SafeClient, SafeClientInterface};
use crate::DeploymentInventory;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct TestDataClient {
    pub working_directory_path: PathBuf,
    pub s3_repository: Box<dyn S3RepositoryInterface>,
    pub safe_client: Box<dyn SafeClientInterface>,
}

#[derive(Default)]
pub struct TestDataClientBuilder {
    working_directory_path: Option<PathBuf>,
    safe_binary_path: Option<PathBuf>,
}

impl TestDataClientBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn working_directory(&mut self, working_directory_path: PathBuf) -> &mut Self {
        self.working_directory_path = Some(working_directory_path);
        self
    }

    pub fn safe_binary_path(&mut self, safe_binary_path: PathBuf) -> &mut Self {
        self.safe_binary_path = Some(safe_binary_path);
        self
    }

    pub fn build(&self) -> Result<TestDataClient> {
        let working_directory_path = match self.working_directory_path {
            Some(ref work_dir_path) => work_dir_path.clone(),
            None => std::env::current_dir()?.join("resources"),
        };
        let safe_binary_path = match self.safe_binary_path {
            Some(ref safe_bin_path) => safe_bin_path.clone(),
            None => working_directory_path.join("safe"),
        };
        let test_data_client = TestDataClient::new(
            working_directory_path.clone(),
            Box::new(S3Repository {}),
            Box::new(SafeClient::new(safe_binary_path, working_directory_path)),
        );
        Ok(test_data_client)
    }
}

impl TestDataClient {
    pub fn new(
        working_directory_path: PathBuf,
        s3_repository: Box<dyn S3RepositoryInterface>,
        safe_client: Box<dyn SafeClientInterface>,
    ) -> TestDataClient {
        TestDataClient {
            working_directory_path,
            s3_repository,
            safe_client,
        }
    }

    pub async fn smoke_test(&self, inventory: DeploymentInventory) -> Result<()> {
        Self::download_and_extract_safe_client(
            &*self.s3_repository,
            &inventory.name,
            &self.working_directory_path,
            &inventory.branch_info.0,
            &inventory.branch_info.1,
        )
        .await?;

        let faucet_addr: SocketAddr = inventory.faucet_address.parse()?;
        let random_peer = inventory.get_random_peer();
        self.safe_client
            .wallet_get_faucet(&random_peer, faucet_addr)?;

        // Generate 10 random files to be uploaded, increasing in size from 1 to 10k.
        // They will then be re-downloaded by `safe` and compared to make sure they are right.
        let mut file_hash_map = HashMap::new();
        let temp_dir_path = tempfile::tempdir()?.into_path();
        for i in 1..=10 {
            let file_size = i * 1024;
            let mut rng = rand::thread_rng();
            let content: Vec<u8> = (0..file_size).map(|_| rng.gen()).collect();

            let mut hasher = Sha256::new();
            hasher.update(&content);
            let hash = format!("{:x}", hasher.finalize());

            let file_path = temp_dir_path.join(format!("file_{}.bin", i));
            let mut file = File::create(&file_path)?;
            file.write_all(&content)?;
            let file_name = file_path
                .clone()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            file_hash_map.insert(file_name, hash);

            self.safe_client.upload_file(&random_peer, &file_path)?;
        }

        self.safe_client.download_files(&random_peer)?;

        let downloaded_files_path = Self::get_downloaded_files_dir_path()?;
        for entry in std::fs::read_dir(downloaded_files_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let mut file = File::open(&path)?;
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;

                let mut hasher = Sha256::new();
                hasher.update(&content);
                let hash = format!("{:x}", hasher.finalize());
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();

                if let Some(stored_hash) = file_hash_map.get(&file_name) {
                    if *stored_hash == hash {
                        println!("Hash match for file {}", file_name);
                    } else {
                        return Err(Error::SmokeTestFailed(format!(
                            "Hash mismatch for file {}",
                            file_name
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn upload_test_data(
        &self,
        name: &str,
        peer_multiaddr: &str,
        branch_info: (String, String),
    ) -> Result<Vec<(String, String)>> {
        let (repo_owner, branch) = branch_info;
        Self::download_and_extract_safe_client(
            &*self.s3_repository,
            name,
            &self.working_directory_path,
            &repo_owner,
            &branch,
        )
        .await?;

        println!("Downloading test data archive from S3...");
        let test_data_dir_path = &self.working_directory_path.join("test-data");
        if !test_data_dir_path.exists() {
            std::fs::create_dir_all(test_data_dir_path)?;
        }
        get_and_extract_archive(
            &*self.s3_repository,
            "sn-testnet",
            "test-data.tar.gz",
            test_data_dir_path,
        )
        .await?;

        let mut uploaded_files = Vec::new();
        let entries = std::fs::read_dir(test_data_dir_path)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                println!("Uploading file with safe: {:?}", path);
                let hex_address = self.safe_client.upload_file(peer_multiaddr, &path)?;
                uploaded_files.push((
                    path.file_name()
                        .ok_or_else(|| {
                            Error::UploadTestDataError(
                                "Could not retrieve file name from test data item".to_string(),
                            )
                        })?
                        .to_string_lossy()
                        .to_string(),
                    hex_address,
                ));
                println!("Successfully uploaded {:?}", path);
            }
        }

        Ok(uploaded_files)
    }

    fn get_downloaded_files_dir_path() -> Result<PathBuf> {
        Ok(dirs_next::data_dir()
            .ok_or_else(|| Error::CouldNotRetrieveDataDirectory)?
            .join("safe")
            .join("client")
            .join("downloaded_files"))
    }

    async fn download_and_extract_safe_client(
        s3_repository: &dyn S3RepositoryInterface,
        name: &str,
        working_directory_path: &Path,
        repo_owner: &str,
        branch: &str,
    ) -> Result<()> {
        let safe_client_path = working_directory_path.join("safe");
        if !safe_client_path.exists() {
            println!("Downloading the safe client from S3...");
            get_and_extract_archive(
                s3_repository,
                "sn-node",
                &format!("{repo_owner}/{branch}/safe-{name}-x86_64-unknown-linux-musl.tar.gz"),
                working_directory_path,
            )
            .await?;
            let mut permissions = std::fs::metadata(&safe_client_path)?.permissions();
            permissions.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&safe_client_path, permissions)?;
        }
        Ok(())
    }
}
