use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::ssh::MockSshClientInterface;
use color_eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn should_run_ansible_against_build_vm() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_inventory_list()
        .times(1)
        .with(eq(
            PathBuf::from("inventory").join(".beta_build_inventory_digital_ocean.yml")
        ))
        .returning(|_| Ok(vec![("beta-build".to_string(), "10.0.0.10".to_string())]));
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("build.yml")),
            eq(PathBuf::from("inventory").join(".beta_build_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"branch\": \"custom_branch\", \"org\": \"maidsafe\", \"provider\": \"digital-ocean\" }".to_string(),
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
    testnet
        .build_custom_safenode("beta", "maidsafe", "custom_branch")
        .await?;

    drop(tmp_dir);
    Ok(())
}
