use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::terraform::MockTerraformRunnerInterface;
use color_eyre::{eyre::eyre, Result};
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn should_run_terraform_apply() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .with(eq("beta".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(1)
        .with(eq(vec![
            ("node_count".to_string(), "30".to_string()),
            ("use_custom_bin".to_string(), "false".to_string()),
        ]))
        .returning(|_| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(setup_default_ansible_runner("beta")),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.deploy("beta", 30, None, None).await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_run_terraform_apply_with_custom_bin_set_when_repo_is_supplied() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .with(eq("beta".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(1)
        .with(eq(vec![
            ("node_count".to_string(), "30".to_string()),
            ("use_custom_bin".to_string(), "true".to_string()),
        ]))
        .returning(|_| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(setup_default_ansible_runner("beta")),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet
        .deploy(
            "beta",
            30,
            Some("maidsafe".to_string()),
            Some("custom_branch".to_string()),
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_ensure_branch_is_set_if_repo_owner_is_used() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner
        .expect_workspace_select()
        .times(0)
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(0)
        .returning(|_| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    let result = testnet
        .deploy("beta", 30, Some("maidsafe".to_string()), None)
        .await;

    match result {
        Ok(()) => {
            drop(tmp_dir);
            Err(eyre!("deploy should have returned an error"))
        }
        Err(e) => {
            assert_eq!(
                e.to_string(),
                "Both the repository owner and branch name must be supplied if either are used"
            );
            drop(tmp_dir);
            Ok(())
        }
    }
}

#[tokio::test]
async fn should_ensure_repo_owner_is_set_if_branch_is_used() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner
        .expect_workspace_select()
        .times(0)
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(0)
        .returning(|_| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    let result = testnet
        .deploy("beta", 30, None, Some("custom_branch".to_string()))
        .await;

    match result {
        Ok(()) => {
            drop(tmp_dir);
            Err(eyre!("deploy should have returned an error"))
        }
        Err(e) => {
            assert_eq!(
                e.to_string(),
                "Both the repository owner and branch name must be supplied if either are used"
            );
            drop(tmp_dir);
            Ok(())
        }
    }
}

#[tokio::test]
async fn should_run_ansible_against_genesis_then_remaining_nodes() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut terraform_runner = setup_default_terraform_runner("beta");
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .with(eq("beta".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(1)
        .with(eq(vec![
            ("node_count".to_string(), "30".to_string()),
            ("use_custom_bin".to_string(), "false".to_string()),
        ]))
        .returning(|_| Ok(()));
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("genesis_node.yml")),
            eq(PathBuf::from("inventory").join(".beta_genesis_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"is_genesis\": \"true\", \"provider\": \"digital-ocean\", \"testnet_name\": \"beta\" }".to_string(),
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(terraform_runner),
        Box::new(ansible_runner),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.init("beta").await?;
    testnet.deploy("beta", 30, None, None).await?;

    drop(tmp_dir);
    Ok(())
}
