// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::*;
use crate::s3::MockS3RepositoryInterface;
use crate::terraform::MockTerraformRunnerInterface;
use assert_fs::fixture::ChildPath;
use assert_fs::prelude::*;
use assert_fs::TempDir;
use color_eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use mockall::predicate::*;
use std::fs::File;

pub fn setup_working_directory() -> Result<(TempDir, ChildPath)> {
    let tmp_dir = assert_fs::TempDir::new()?;
    let working_dir = tmp_dir.child("work");
    working_dir.create_dir_all()?;
    working_dir.copy_from("resources/", &["**"])?;
    Ok((tmp_dir, working_dir))
}

pub fn create_fake_bin_archive(
    working_dir: &ChildPath,
    archive_name: &str,
    bin_name: &str,
) -> Result<ChildPath> {
    let temp_archive_dir = working_dir.child("setup_archive");
    let archive = temp_archive_dir.child(archive_name);
    let fake_bin = temp_archive_dir.child(bin_name);
    fake_bin.write_binary(b"fake code")?;

    let mut fake_bin_file = File::open(fake_bin.path())?;
    let gz_encoder = GzEncoder::new(File::create(archive.path())?, Compression::default());
    let mut builder = tar::Builder::new(gz_encoder);
    builder.append_file(bin_name, &mut fake_bin_file)?;
    builder.into_inner()?;

    Ok(archive)
}

pub fn create_fake_test_data_archive(
    working_dir: &ChildPath,
    archive_name: &str,
) -> Result<ChildPath> {
    let image_file_names = [
        "pexels-ahmed-ツ-13524648.jpg",
        "pexels-ahmed-ツ-14113084.jpg",
        "pexels-aidan-roof-11757330.jpg",
    ];
    let temp_archive_dir = working_dir.child("test-data-setup-archive");
    let archive = temp_archive_dir.child(archive_name);
    for image_file_name in image_file_names.iter() {
        let fake_image = temp_archive_dir.child(image_file_name);
        fake_image.write_binary(b"fake image data")?;
    }

    let gz_encoder = GzEncoder::new(File::create(archive.path())?, Compression::default());
    let mut builder = tar::Builder::new(gz_encoder);
    for image_file_name in image_file_names.iter() {
        let mut fake_file = File::open(temp_archive_dir.path().join(image_file_name))?;
        builder.append_file(image_file_name, &mut fake_file)?;
    }
    builder.into_inner()?;

    Ok(archive)
}

pub fn setup_default_terraform_runner(name: &str) -> MockTerraformRunnerInterface {
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner.expect_init().times(1).returning(|| Ok(()));
    terraform_runner
        .expect_workspace_list()
        .times(1)
        .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
    terraform_runner
        .expect_workspace_new()
        .times(1)
        .with(eq(name.to_string()))
        .returning(|_| Ok(()));
    terraform_runner
}

pub fn setup_deploy_s3_repository(
    env_name: &str,
    working_dir: &ChildPath,
) -> Result<MockS3RepositoryInterface> {
    // For an explanation of what's happening here, see the comments on the function below. This
    // function is the same, except it only deals with one archive.
    let saved_archive_path = working_dir
        .to_path_buf()
        .join("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
    let rpc_client_archive_path =
        create_fake_bin_archive(working_dir, "rpc_client.tar.gz", RPC_CLIENT_BIN_NAME)?;
    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository
        .expect_download_object()
        .with(
            eq("sn-testnet"),
            eq("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz"),
            eq(saved_archive_path),
        )
        .times(1)
        .returning(move |_bucket_name, _object_path, archive_path| {
            std::fs::copy(&rpc_client_archive_path, archive_path)?;
            Ok(())
        });
    s3_repository
        .expect_folder_exists()
        .with(eq("sn-testnet"), eq(format!("testnet-logs/{env_name}")))
        .times(1)
        .returning(|_, _| Ok(false));
    Ok(s3_repository)
}

pub fn setup_test_data_s3_repository(
    env_name: &str,
    repo_owner: &str,
    branch_name: &str,
    working_dir: &ChildPath,
) -> Result<MockS3RepositoryInterface> {
    // This function can be a little hard to understand.
    //
    // Two tar.gz archives are created: one with a fake `safe` binary and the other with three fake
    // images that get used for test data. The system under test will actually extract these fake
    // archives. In the real code, the archives will be fetched from S3, and that's where the mock
    // comes in. In the test setup, the archives are stored in a temporary directory.
    //
    // The mock declares that it should expect two invocations of its `download_object` function,
    // one for each archive. The mock's `returning` function defines a side effect, which copies
    // the fake archive from its temporary directory to the location where the real archive would
    // be downloaded, and the code then operates on the fake archive.
    let saved_safe_archive_file_name = format!("safe-{env_name}-x86_64-unknown-linux-musl.tar.gz");
    let safe_s3_object_path = format!("{repo_owner}/{branch_name}/{saved_safe_archive_file_name}");
    let saved_safe_archive_path = working_dir
        .to_path_buf()
        .join(saved_safe_archive_file_name.clone());
    let fake_safe_client_archive_path =
        create_fake_bin_archive(working_dir, "safe_client.tar.gz", "safe")?;

    let saved_test_data_archive_file_name = "test-data.tar.gz";
    let saved_test_data_archive_path = working_dir
        .join("test-data")
        .join(saved_test_data_archive_file_name);
    let fake_test_data_archive_path =
        create_fake_test_data_archive(working_dir, "test-data.tar.gz")?;

    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository
        .expect_download_object()
        .with(
            eq("sn-node"),
            eq(safe_s3_object_path),
            eq(saved_safe_archive_path),
        )
        .times(1)
        .returning(move |_bucket_name, _object_path, archive_path| {
            std::fs::copy(&fake_safe_client_archive_path, archive_path)?;
            Ok(())
        });

    s3_repository
        .expect_download_object()
        .with(
            eq("sn-testnet"),
            eq(saved_test_data_archive_file_name),
            eq(saved_test_data_archive_path),
        )
        .times(1)
        .returning(move |_bucket_name, _object_path, archive_path| {
            std::fs::copy(&fake_test_data_archive_path, archive_path)?;
            Ok(())
        });
    Ok(s3_repository)
}
