use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use color_eyre::{eyre::eyre, Result};
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn should_run_ansible_against_genesis() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_inventory_list()
        .times(1)
        .with(eq(
            PathBuf::from("inventory").join(".beta_genesis_inventory_digital_ocean.yml")
        ))
        .returning(|_| Ok(vec![("beta-genesis".to_string(), "10.0.0.10".to_string())]));
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("genesis_node.yml")),
            eq(PathBuf::from("inventory").join(".beta_genesis_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"provider\": \"digital-ocean\", \"testnet_name\": \"beta\" }".to_string(),
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let mut ssh_client = MockSshClientInterface::new();
    ssh_client
        .expect_wait_for_ssh_availability()
        .times(1)
        .with(eq("10.0.0.10"), eq("root"))
        .returning(|_, _| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(setup_default_terraform_runner("beta")),
        Box::new(ansible_runner),
        Box::new(MockRpcClientInterface::new()),
        Box::new(ssh_client),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.init("beta").await?;
    testnet.provision_genesis_node("beta", None, None).await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_ensure_branch_is_set_if_repo_owner_is_used() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;

    let testnet = TestnetDeploy::new(
        Box::new(MockTerraformRunnerInterface::new()),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    let result = testnet
        .provision_genesis_node("beta", Some("maidsafe".to_string()), None)
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

    let testnet = TestnetDeploy::new(
        Box::new(MockTerraformRunnerInterface::new()),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    let result = testnet
        .provision_genesis_node("beta", None, Some("custom_branch".to_string()))
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
