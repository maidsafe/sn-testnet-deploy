use super::*;
use crate::s3::S3AssetRepository;
use crate::terraform::MockTerraformRunnerInterface;
use assert_fs::fixture::ChildPath;
use assert_fs::prelude::*;
use assert_fs::TempDir;
use color_eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use httpmock::prelude::*;
use mockall::predicate::*;
use std::fs::{File, Metadata};

pub fn setup_working_directory() -> Result<(TempDir, ChildPath)> {
    let tmp_dir = assert_fs::TempDir::new()?;
    let working_dir = tmp_dir.child("work");
    working_dir.create_dir_all()?;
    working_dir.copy_from("resources/", &["**"])?;
    Ok((tmp_dir, working_dir))
}

pub fn create_fake_rpc_client_archive(working_dir: &ChildPath) -> Result<(ChildPath, Metadata)> {
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
    let rpc_client_archive_metadata = std::fs::metadata(rpc_client_archive.path())?;

    Ok((rpc_client_archive, rpc_client_archive_metadata))
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

pub fn setup_default_s3_repository(working_dir: &ChildPath) -> Result<S3AssetRepository> {
    let (rpc_client_archive, rpc_client_archive_metadata) =
        create_fake_rpc_client_archive(working_dir)?;
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

    let s3_repository = S3AssetRepository::new(&asset_server.base_url());
    Ok(s3_repository)
}
