use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use color_eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn should_run_ansible_against_the_remaining_nodes() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("nodes.yml")),
            eq(PathBuf::from("inventory").join(".beta_node_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"genesis_multiaddr\": \"/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded\", \"provider\": \"digital-ocean\", \"testnet_name\": \"beta\" }".to_string(),
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(setup_default_terraform_runner("beta")),
        Box::new(ansible_runner),
        Box::new(MockRpcClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        s3_repository,
    );

    testnet.init("beta").await?;
    testnet
        .provision_remaining_nodes(
            "beta",
            "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded"
                .to_string(),
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}
