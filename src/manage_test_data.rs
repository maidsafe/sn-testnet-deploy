// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::get_and_extract_archive;
use crate::s3::{S3Repository, S3RepositoryInterface};
use crate::safe::{SafeClient, SafeClientInterface};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

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

    pub async fn upload_test_data(
        &self,
        name: &str,
        peer_multiaddr: &str,
        branch_info: (String, String),
    ) -> Result<Vec<(String, String)>> {
        let (repo_owner, branch) = branch_info;
        println!("Downloading the safe client from S3...");
        get_and_extract_archive(
            &*self.s3_repository,
            "sn-node",
            &format!("{repo_owner}/{branch}/safe-{name}-x86_64-unknown-linux-musl.tar.gz"),
            &self.working_directory_path,
        )
        .await?;
        let safe_client_path = self.working_directory_path.join("safe");
        let mut permissions = std::fs::metadata(&safe_client_path)?.permissions();
        permissions.set_mode(0o755); // rwxr-xr-x
        std::fs::set_permissions(&safe_client_path, permissions)?;

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
}
