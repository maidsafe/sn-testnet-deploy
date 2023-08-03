use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use color_eyre::Result;
use mockall::predicate::*;

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
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.create_infra("beta", 30, false).await?;

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
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockRpcClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.create_infra("beta", 30, true).await?;

    drop(tmp_dir);
    Ok(())
}
