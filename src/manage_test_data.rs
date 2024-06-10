// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    extract_archive, get_and_extract_archive_from_s3,
    s3::S3Repository,
    safe::{SafeBinaryRepository, SafeClient},
    BinaryOption, DeploymentInventory,
};
use color_eyre::{eyre::eyre, Help, Result};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const BASE_URL: &str = "https://github.com/maidsafe/safe_network/releases/download";

pub struct TestDataClient {
    pub working_directory_path: PathBuf,
    pub s3_repository: S3Repository,
    pub safe_client: SafeClient,
    pub safe_binary_repository: SafeBinaryRepository,
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
            S3Repository {},
            SafeClient::new(safe_binary_path, working_directory_path),
            SafeBinaryRepository {},
        );
        Ok(test_data_client)
    }
}

impl TestDataClient {
    pub fn new(
        working_directory_path: PathBuf,
        s3_repository: S3Repository,
        safe_client: SafeClient,
        safe_binary_repository: SafeBinaryRepository,
    ) -> TestDataClient {
        TestDataClient {
            working_directory_path,
            s3_repository,
            safe_client,
            safe_binary_repository,
        }
    }

    pub async fn smoke_test(
        &self,
        inventory: &mut DeploymentInventory,
        safe_version: Option<String>,
    ) -> Result<()> {
        match &inventory.binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                Self::download_and_extract_safe_client_from_s3(
                    &self.s3_repository,
                    &inventory.name,
                    &self.working_directory_path,
                    repo_owner,
                    branch,
                )
                .await?;
            }
            BinaryOption::Versioned { .. } => {
                if let Some(version) = safe_version {
                    Self::download_and_extract_safe_client_from_url(
                        &self.safe_binary_repository,
                        &version,
                        &self.working_directory_path,
                    )
                    .await?;
                } else {
                    return Err(eyre!(
                        "The '{}' environment was deployed using versioned binaries.",
                        inventory.name
                    )
                    .suggestion(
                        "For this kind of deployment, the --safe-version argument must be \
                            used to specify the client version.",
                    ));
                }
            }
        }

        let faucet_addr = inventory.faucet_address.clone().ok_or_else(|| {
            return eyre!("No faucet deployed for this inventory. (It was launched using existing bootstrap peers)")
        })?;

        let faucet_addr: SocketAddr = faucet_addr.parse()?;
        let random_peer = inventory.get_random_peer();
        self.safe_client
            .wallet_get_faucet(&random_peer, faucet_addr)?;
        // Generate 10 random files to be uploaded, increasing in size from 1 to 10k.
        // They will then be re-downloaded by `safe` and compared to make sure they are right.
        let mut uploaded_files = Vec::new();
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
            file_hash_map.insert(file_name.clone(), hash);

            let hex_address = self.safe_client.upload_file(&random_peer, &file_path)?;
            uploaded_files.push((hex_address, file_name))
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
                        return Err(eyre!("Hash mismatch for file {}", file_name));
                    }
                }
            }
        }

        inventory.add_uploaded_files(uploaded_files);

        Ok(())
    }

    pub async fn upload_test_data(
        &self,
        name: &str,
        peer_multiaddr: &str,
        binary_option: &BinaryOption,
        safe_version: Option<String>,
    ) -> Result<Vec<(String, String)>> {
        match binary_option {
            BinaryOption::BuildFromSource {
                repo_owner, branch, ..
            } => {
                Self::download_and_extract_safe_client_from_s3(
                    &self.s3_repository,
                    name,
                    &self.working_directory_path,
                    repo_owner,
                    branch,
                )
                .await?;
            }
            BinaryOption::Versioned { .. } => {
                if let Some(version) = safe_version {
                    Self::download_and_extract_safe_client_from_url(
                        &self.safe_binary_repository,
                        &version,
                        &self.working_directory_path,
                    )
                    .await?;
                } else {
                    return Err(eyre!(
                        "The '{}' environment was deployed using versioned binaries.",
                        name
                    )
                    .suggestion(
                        "For this kind of deployment, the --safe-version argument must be \
                            used to specify the client version.",
                    ));
                }
            }
        }

        println!("Downloading test data archive from S3...");
        let test_data_dir_path = &self.working_directory_path.join("test-data");
        if !test_data_dir_path.exists() {
            std::fs::create_dir_all(test_data_dir_path)?;
        }
        get_and_extract_archive_from_s3(
            &self.s3_repository,
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
                        .ok_or_else(|| eyre!("Could not retrieve file name from test data item"))?
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
            .ok_or_else(|| eyre!("Could not retrieve data directory"))?
            .join("safe")
            .join("client")
            .join("downloaded_files"))
    }

    async fn download_and_extract_safe_client_from_s3(
        s3_repository: &S3Repository,
        name: &str,
        working_directory_path: &Path,
        repo_owner: &str,
        branch: &str,
    ) -> Result<()> {
        let safe_client_path = working_directory_path.join("safe");
        if !safe_client_path.exists() {
            println!("Downloading the safe client from S3...");
            get_and_extract_archive_from_s3(
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

    async fn download_and_extract_safe_client_from_url(
        safe_binary_repository: &SafeBinaryRepository,
        version: &str,
        working_directory_path: &Path,
    ) -> Result<()> {
        let safe_client_path = working_directory_path.join("safe");
        if !safe_client_path.exists() {
            let archive_name = format!("safe-{version}-x86_64-unknown-linux-musl.tar.gz");
            let archive_path = working_directory_path.join(archive_name.clone());
            let url = format!("{BASE_URL}/sn_cli-v{version}/{archive_name}");
            println!("url = {url}");
            println!("archive_path = {archive_path:#?}");
            safe_binary_repository.download(&url, &archive_path).await?;
            extract_archive(&archive_path, working_directory_path).await?;

            let mut permissions = std::fs::metadata(&safe_client_path)?.permissions();
            permissions.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&safe_client_path, permissions)?;
        }
        Ok(())
    }
}
