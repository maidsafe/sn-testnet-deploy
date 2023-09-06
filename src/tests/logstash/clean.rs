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
use color_eyre::{eyre::eyre, Result};
use mockall::predicate::*;
use mockall::Sequence;

#[tokio::test]
async fn should_run_terraform_destroy_and_delete_workspace_and_delete_inventory_files() -> Result<()>
{
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = setup_default_terraform_runner("alpha");
    let mut seq = Sequence::new();
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
        .expect_workspace_select()
        .times(1)
        .in_sequence(&mut seq)
        .with(eq("alpha".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_destroy()
        .times(1)
        .returning(|| Ok(()));
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .in_sequence(&mut seq)
        .with(eq("dev".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_workspace_delete()
        .times(1)
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

    // Calling init will create the Ansible inventory files, which we want to be removed by
    // the clean operation.
    logstash.init("alpha").await?;
    logstash.clean("alpha").await?;

    let inventory_file = working_dir.child(format!(
        "ansible/inventory/.{}_logstash_inventory_digital_ocean.yml",
        "alpha"
    ));
    inventory_file.assert(predicates::path::missing());

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_return_an_error_when_invalid_name_is_supplied() -> Result<()> {
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

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    let result = logstash.clean("beta").await;
    match result {
        Ok(()) => {
            drop(tmp_dir);
            Err(eyre!("deploy should have returned an error"))
        }
        Err(e) => {
            assert_eq!(e.to_string(), "The 'beta' environment does not exist");
            drop(tmp_dir);
            Ok(())
        }
    }
}

#[tokio::test]
async fn should_not_error_when_inventory_does_not_exist() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut seq = Sequence::new();
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
        .expect_workspace_select()
        .times(1)
        .in_sequence(&mut seq)
        .with(eq("alpha".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_destroy()
        .times(1)
        .returning(|| Ok(()));
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .in_sequence(&mut seq)
        .with(eq("dev".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_workspace_delete()
        .times(1)
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

    // Do not call the `init` command, which will be the case in the remote GHA workflow
    // environment. In this case, the process should still complete without an error. It should not
    // attempt to remove inventory files that don't exist.
    let result = logstash.clean("alpha").await;
    match result {
        Ok(()) => {
            drop(tmp_dir);
            Ok(())
        }
        Err(_) => {
            drop(tmp_dir);
            Err(eyre!("clean should run without error"))
        }
    }
}
