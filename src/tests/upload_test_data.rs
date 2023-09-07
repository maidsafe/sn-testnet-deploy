// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::setup::*;
use crate::manage_test_data::TestDataClient;
use crate::safe::MockSafeClientInterface;
use assert_fs::prelude::*;
use color_eyre::Result;
use mockall::predicate::*;
use std::os::unix::fs::PermissionsExt;

fn setup_default_safe_client() -> MockSafeClientInterface {
    let mut safe_client_repository = MockSafeClientInterface::new();
    safe_client_repository
        .expect_upload_file()
        .with(always(), always())
        .times(3)
        .returning(|_, _| Ok("hex address".to_string()));
    safe_client_repository
}

#[tokio::test]
async fn should_download_and_extract_the_safe_binary() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let downloaded_safe_archive = working_dir.child("safe-alpha-x86_64-unknown-linux-musl.tar.gz");
    let extracted_safe_bin = working_dir.child("safe");

    let s3_repository =
        setup_test_data_s3_repository("alpha", "jacderida", "custom_branch", &working_dir)?;
    let test_data_client = TestDataClient::new(
        working_dir.to_path_buf(),
        Box::new(s3_repository),
        Box::new(setup_default_safe_client()),
    );

    test_data_client
        .upload_test_data(
            "alpha",
            "/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr",
            ("jacderida".to_string(), "custom_branch".to_string()),
        )
        .await?;

    downloaded_safe_archive.assert(predicates::path::missing());
    extracted_safe_bin.assert(predicates::path::is_file());

    let metadata = std::fs::metadata(extracted_safe_bin.path())?;
    let permissions = metadata.permissions();
    assert!(permissions.mode() & 0o100 > 0, "File is not executable");

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_download_and_extract_the_test_data() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let test_data_dir = working_dir.child("test-data");
    test_data_dir.create_dir_all()?;
    let downloaded_test_data_archive = test_data_dir.child("test-data.tar.gz");
    let extracted_data_item_1 = test_data_dir.child("pexels-ahmed-ツ-13524648.jpg");
    let extracted_data_item_2 = test_data_dir.child("pexels-ahmed-ツ-14113084.jpg");
    let extracted_data_item_3 = test_data_dir.child("pexels-aidan-roof-11757330.jpg");

    let s3_repository = setup_test_data_s3_repository("alpha", "maidsafe", "main", &working_dir)?;
    let test_data_client = TestDataClient::new(
        working_dir.to_path_buf(),
        Box::new(s3_repository),
        Box::new(setup_default_safe_client()),
    );

    test_data_client
        .upload_test_data(
            "alpha",
            "/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr",
            ("maidsafe".to_string(), "main".to_string()),
        )
        .await?;

    downloaded_test_data_archive.assert(predicates::path::missing());
    extracted_data_item_1.assert(predicates::path::is_file());
    extracted_data_item_2.assert(predicates::path::is_file());
    extracted_data_item_3.assert(predicates::path::is_file());

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_upload_test_data_files() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let test_data_dir = working_dir.child("test-data");
    test_data_dir.create_dir_all()?;
    let extracted_data_item_1 = test_data_dir.child("pexels-ahmed-ツ-13524648.jpg");
    let extracted_data_item_2 = test_data_dir.child("pexels-ahmed-ツ-14113084.jpg");
    let extracted_data_item_3 = test_data_dir.child("pexels-aidan-roof-11757330.jpg");

    let mut safe_client_repository = MockSafeClientInterface::new();
    safe_client_repository
        .expect_upload_file()
        .with(
            eq("/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr"),
            eq(extracted_data_item_1.to_path_buf()),
        )
        .times(1)
        .returning(|_, _| {
            Ok("58bc58f3d10f3cb1f9824f970d9d0bee3bbcb039b203ae8c4003caa93fc645aa".to_string())
        });
    safe_client_repository
        .expect_upload_file()
        .with(
            eq("/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr"),
            eq(extracted_data_item_2.to_path_buf()),
        )
        .times(1)
        .returning(|_, _| {
            Ok("1d6cd408c8961940aefd06a1e736c45f703a8617d919ee9791122de39d547ca2".to_string())
        });
    safe_client_repository
        .expect_upload_file()
        .with(
            eq("/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr"),
            eq(extracted_data_item_3.to_path_buf()),
        )
        .times(1)
        .returning(|_, _| {
            Ok("d27d605b1d4f94934530d3bce0c1c9f8db9bdf74294df3f0139ad22125a54967".to_string())
        });

    let s3_repository = setup_test_data_s3_repository("alpha", "maidsafe", "main", &working_dir)?;
    let test_data_client = TestDataClient::new(
        working_dir.to_path_buf(),
        Box::new(s3_repository),
        Box::new(safe_client_repository),
    );

    let download_links = test_data_client
        .upload_test_data(
            "alpha",
            "/ip4/10.0.0.1/tcp/43627/p2p/12D3KooWAsY69M1HYAsvwsrF9BkQRywM6CWDvM78m1k92CPco7qr",
            ("maidsafe".to_string(), "main".to_string()),
        )
        .await?;

    // Avoid assuming the order of the items in the list; they don't get returned in the same order
    // on every machine.
    assert_eq!(
        download_links
            .iter()
            .find(|x| x.0 == "pexels-ahmed-ツ-13524648.jpg"),
        Some((
            "pexels-ahmed-ツ-13524648.jpg".to_string(),
            "58bc58f3d10f3cb1f9824f970d9d0bee3bbcb039b203ae8c4003caa93fc645aa".to_string()
        ))
        .as_ref()
    );
    assert_eq!(
        download_links
            .iter()
            .find(|x| x.0 == "pexels-ahmed-ツ-14113084.jpg"),
        Some((
            "pexels-ahmed-ツ-14113084.jpg".to_string(),
            "1d6cd408c8961940aefd06a1e736c45f703a8617d919ee9791122de39d547ca2".to_string()
        ))
        .as_ref()
    );
    assert_eq!(
        download_links
            .iter()
            .find(|x| x.0 == "pexels-aidan-roof-11757330.jpg"),
        Some((
            "pexels-aidan-roof-11757330.jpg".to_string(),
            "d27d605b1d4f94934530d3bce0c1c9f8db9bdf74294df3f0139ad22125a54967".to_string()
        ))
        .as_ref()
    );

    drop(tmp_dir);
    Ok(())
}
