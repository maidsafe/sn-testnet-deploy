use super::super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::digital_ocean::MockDigitalOceanClientInterface;
use crate::logstash::LogstashDeploy;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use crate::CloudProvider;
use color_eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn should_run_terraform_apply() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut terraform_runner = MockTerraformRunnerInterface::new();
    terraform_runner
        .expect_workspace_select()
        .times(1)
        .with(eq("beta".to_string()))
        .returning(|_| Ok(()));
    terraform_runner
        .expect_apply()
        .times(1)
        .with(eq(vec![("node_count".to_string(), "2".to_string())]))
        .returning(|_| Ok(()));

    let logstash = LogstashDeploy::new(
        Box::new(terraform_runner),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.create_infra("beta", 2).await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_run_ansible_to_provision_the_logstash_nodes() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_inventory_list()
        .times(1)
        .with(eq(
            PathBuf::from("inventory").join(".beta_logstash_inventory_digital_ocean.yml")
        ))
        .returning(|_| {
            Ok(vec![(
                "beta-logstash-1".to_string(),
                "10.0.0.10".to_string(),
            )])
        });
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("logstash.yml")),
            eq(PathBuf::from("inventory").join(".beta_logstash_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"provider\": \"digital-ocean\", \"stack_name\": \"beta\" }".to_string(),
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let mut ssh_runner = MockSshClientInterface::new();
    ssh_runner
        .expect_wait_for_ssh_availability()
        .times(1)
        .with(eq("10.0.0.10"), eq("root"))
        .returning(|_, _| Ok(()));

    let logstash = LogstashDeploy::new(
        Box::new(MockTerraformRunnerInterface::new()),
        Box::new(ansible_runner),
        Box::new(ssh_runner),
        Box::new(MockDigitalOceanClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    logstash.provision("beta").await?;

    drop(tmp_dir);
    Ok(())
}
