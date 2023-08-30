// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::digital_ocean::MockDigitalOceanClientInterface;
use crate::logstash::LogstashDeploy;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use crate::CloudProvider;
use assert_fs::prelude::*;
use color_eyre::Result;
use mockall::predicate::*;
use mockall::Sequence;

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

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.init("beta").await?;

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

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.init("alpha").await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_generate_ansible_inventory_for_digital_ocean_for_the_new_logstash() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let terraform_runner = setup_default_terraform_runner("alpha");

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.init("alpha").await?;

    let inventory_file = working_dir.child(format!(
        "ansible/inventory/.{}_logstash_inventory_digital_ocean.yml",
        "alpha"
    ));
    inventory_file.assert(predicates::path::is_file());

    let contents = std::fs::read_to_string(inventory_file.path())?;
    assert!(contents.contains("alpha"));
    assert!(contents.contains("logstash"));
    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_not_overwrite_generated_inventory() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
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

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.init("alpha").await?;
    logstash.init("alpha").await?; // this should be idempotent

    let inventory_file = working_dir.child(format!(
        "ansible/inventory/.{}_logstash_inventory_digital_ocean.yml",
        "alpha"
    ));
    inventory_file.assert(predicates::path::is_file());

    let contents = std::fs::read_to_string(inventory_file.path())?;
    assert!(contents.contains("alpha"));
    assert!(contents.contains("logstash"));
    drop(tmp_dir);
    Ok(())
}
