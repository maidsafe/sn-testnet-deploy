// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use super::RPC_CLIENT_BIN_NAME;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::s3::MockS3RepositoryInterface;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use assert_fs::prelude::*;
use color_eyre::{eyre::eyre, Result};
use mockall::predicate::*;
use mockall::Sequence;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn should_create_a_new_workspace() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner.expect_init().times(1).returning(|| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(1)
        .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
    terraform_runner
        .expect_workspace_new()
        .times(1)
        .with(eq("beta".to_string()))
        .returning(|_| Ok(()));
    let s3_repository = setup_default_s3_repository("beta", &working_dir)?;
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );
    testnet.init("beta").await?;
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_not_create_a_new_workspace_when_one_with_the_same_name_exists() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner.expect_init().times(1).returning(|| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(1)
        .returning(|| {
            Ok(vec![
                "alpha".to_string(),
                "default".to_string(),
                "dev".to_string(),
            ])
        });
    terraform_runner
        .expect_workspace_new()
        .times(0)
        .with(eq("alpha".to_string()))
        .returning(|_| Ok(()));

    let s3_repository = setup_default_s3_repository("alpha", &working_dir)?;
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );
    testnet.init("alpha").await?;
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_download_and_extract_the_rpc_client() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let downloaded_safe_archive =
        working_dir.child("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");

    let extracted_rpc_client_bin = working_dir.child(RPC_CLIENT_BIN_NAME);
    let s3_repository = setup_default_s3_repository("alpha", &working_dir)?;
    let terraform_runner = setup_default_terraform_runner("alpha");
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.init("alpha").await?;

    downloaded_safe_archive.assert(predicates::path::missing());
    extracted_rpc_client_bin.assert(predicates::path::is_file());

    let metadata = std::fs::metadata(extracted_rpc_client_bin.path())?;
    let permissions = metadata.permissions();
    assert!(permissions.mode() & 0o100 > 0, "File is not executable");
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_not_download_the_rpc_client_if_it_already_exists() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let fake_rpc_client_bin = working_dir.child(RPC_CLIENT_BIN_NAME);
    fake_rpc_client_bin.write_binary(b"fake code")?;

    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository
        .expect_folder_exists()
        .with(eq("testnet-logs/alpha".to_string()))
        .times(1)
        .returning(|_| Ok(false));
    s3_repository.expect_download_object().times(0);

    let terraform_runner = setup_default_terraform_runner("alpha");
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );
    testnet.init("alpha").await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_generate_ansible_inventory_for_digital_ocean_for_the_new_testnet() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository("alpha", &working_dir)?;
    let terraform_runner = setup_default_terraform_runner("alpha");

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.init("alpha").await?;

    let inventory_files = ["build", "genesis", "node"];
    for inventory_type in inventory_files.iter() {
        let inventory_file = working_dir.child(format!(
            "ansible/inventory/.{}_{}_inventory_digital_ocean.yml",
            "alpha", inventory_type
        ));
        inventory_file.assert(predicates::path::is_file());

        let contents = std::fs::read_to_string(inventory_file.path())?;
        assert!(contents.contains("alpha"));
        assert!(contents.contains(inventory_type));
    }
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_not_overwrite_generated_inventory() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    let mut seq = Sequence::new();

    let saved_archive_path = working_dir
        .to_path_buf()
        .join("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
    let rpc_client_archive_path = create_fake_rpc_client_archive(&working_dir)?;
    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository
        .expect_download_object()
        .with(
            eq("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz"),
            eq(saved_archive_path),
        )
        .times(1)
        .returning(move |_object_path, archive_path| {
            std::fs::copy(&rpc_client_archive_path, archive_path)?;
            Ok(())
        });
    s3_repository
        .expect_folder_exists()
        .with(eq("testnet-logs/alpha".to_string()))
        .times(2)
        .returning(|_| Ok(false));

    terraform_runner.expect_init().times(2).returning(|| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(1)
        .in_sequence(&mut seq)
        .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
    terraform_runner
        .expect_workspace_new()
        .times(1)
        .with(eq("alpha".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(1)
        .in_sequence(&mut seq)
        .returning(|| {
            Ok(vec![
                "alpha".to_string(),
                "default".to_string(),
                "dev".to_string(),
            ])
        });

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.init("alpha").await?;
    testnet.init("alpha").await?; // this should be idempotent

    let inventory_files = ["build", "genesis", "node"];
    for inventory_type in inventory_files.iter() {
        let inventory_file = working_dir.child(format!(
            "ansible/inventory/.{}_{}_inventory_digital_ocean.yml",
            "alpha", inventory_type
        ));
        inventory_file.assert(predicates::path::is_file());

        let contents = std::fs::read_to_string(inventory_file.path())?;
        assert!(contents.contains("alpha"));
        assert!(contents.contains(inventory_type));
    }
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_return_an_error_if_logs_already_exist_for_environment() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner.expect_init().times(0).returning(|| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(0)
        .returning(|| {
            Ok(vec![
                "alpha".to_string(),
                "default".to_string(),
                "dev".to_string(),
            ])
        });
    terraform_runner
        .expect_workspace_new()
        .times(0)
        .with(eq("alpha".to_string()))
        .returning(|_| Ok(()));

    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository
        .expect_folder_exists()
        .with(eq("testnet-logs/alpha"))
        .times(1)
        .returning(|_| Ok(true));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    let result = testnet.init("alpha").await;
    match result {
        Ok(()) => {
            drop(tmp_dir);
            Err(eyre!("init should have returned an error"))
        }
        Err(e) => {
            assert_eq!(e.to_string(), "Logs for a 'alpha' testnet already exist");
            drop(tmp_dir);
            Ok(())
        }
    }
}
