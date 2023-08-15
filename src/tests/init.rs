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
use crate::s3::S3AssetRepository;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use assert_fs::prelude::*;
use color_eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use httpmock::prelude::*;
use mockall::predicate::*;
use mockall::Sequence;
use std::fs::File;
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
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
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

    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );
    testnet.init("alpha").await?;
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_download_and_extract_the_rpc_client() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let temp_archive_dir = working_dir.child("setup_archive");

    // Create an archive containing a fake rpc client exe, to be returned by the mock HTTP
    // server.
    let rpc_client_archive = temp_archive_dir.child("rpc_client.tar.gz");
    let fake_rpc_client_bin = temp_archive_dir.child(RPC_CLIENT_BIN_NAME);
    fake_rpc_client_bin.write_binary(b"fake code")?;
    let mut fake_rpc_client_bin_file = File::open(fake_rpc_client_bin.path())?;
    let gz_encoder = GzEncoder::new(
        File::create(rpc_client_archive.path())?,
        Compression::default(),
    );
    let mut builder = tar::Builder::new(gz_encoder);
    builder.append_file(RPC_CLIENT_BIN_NAME, &mut fake_rpc_client_bin_file)?;
    builder.into_inner()?;
    let rpc_client_archive_metadata = std::fs::metadata(rpc_client_archive.path())?;

    let asset_server = MockServer::start();
    asset_server.mock(|when, then| {
        when.method(GET)
            .path("/rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
        then.status(200)
            .header(
                "Content-Length",
                rpc_client_archive_metadata.len().to_string(),
            )
            .header("Content-Type", "application/gzip")
            .body_from_file(rpc_client_archive.path().to_str().unwrap());
    });
    let downloaded_safe_archive =
        working_dir.child("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");

    let extracted_rpc_client_bin = working_dir.child(RPC_CLIENT_BIN_NAME);
    let s3_repository = S3AssetRepository::new(&asset_server.base_url());
    let terraform_runner = setup_default_terraform_runner("alpha");
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
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

    let (rpc_client_archive, rpc_client_archive_metadata) =
        create_fake_rpc_client_archive(&working_dir)?;
    let asset_server = MockServer::start();
    let mock = asset_server.mock(|when, then| {
        when.method(GET)
            .path("/rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
        then.status(200)
            .header(
                "Content-Length",
                rpc_client_archive_metadata.len().to_string(),
            )
            .header("Content-Type", "application/gzip")
            .body_from_file(rpc_client_archive.path().to_str().unwrap());
    });
    let s3_repository = S3AssetRepository::new(&asset_server.base_url());

    let terraform_runner = setup_default_terraform_runner("alpha");
    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );
    testnet.init("alpha").await?;

    mock.assert_hits(0);
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_generate_ansible_inventory_for_digital_ocean_for_the_new_testnet() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let terraform_runner = setup_default_terraform_runner("alpha");

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
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
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    let mut seq = Sequence::new();

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
        s3_repository,
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
