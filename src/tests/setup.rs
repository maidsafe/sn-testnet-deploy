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

pub fn create_fake_rpc_client_archive(working_dir: &ChildPath) -> Result<ChildPath> {
    let temp_archive_dir = working_dir.child("setup_archive");
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

    Ok(rpc_client_archive)
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

pub fn setup_default_s3_repository(working_dir: &ChildPath) -> Result<MockS3RepositoryInterface> {
    let saved_archive_path = working_dir
        .to_path_buf()
        .join("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
    let rpc_client_archive_path = create_fake_rpc_client_archive(working_dir)?;
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
    Ok(s3_repository)
}
