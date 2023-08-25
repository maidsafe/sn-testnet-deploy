// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::ssh::MockSshClientInterface;
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
            eq(PathBuf::from("faucet.yml")),
            eq(PathBuf::from("inventory").join(".beta_genesis_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"provider\": \"digital-ocean\", \"testnet_name\": \"beta\", \"genesis_multiaddr\": \"/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded\" }".to_string(),
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(setup_default_terraform_runner("beta")),
        Box::new(ansible_runner),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.init("beta").await?;
    testnet
        .provision_faucet(
            "beta",
            "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded",
            None,
            None,
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_run_ansible_against_the_remaining_nodes_with_a_custom_binary() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository(&working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("faucet.yml")),
            eq(PathBuf::from("inventory").join(".beta_genesis_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                "{ \"provider\": \"digital-ocean\", \"testnet_name\": \"beta\", \"genesis_multiaddr\": \"/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded\", \"branch\": \"custom_branch\", \"org\": \"maidsafe\" }".to_string()
            )),
        )
        .returning(|_, _, _, _| Ok(()));

    let testnet = TestnetDeploy::new(
        Box::new(setup_default_terraform_runner("beta")),
        Box::new(ansible_runner),
        Box::new(MockRpcClientInterface::new()),
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.init("beta").await?;
    testnet
        .provision_faucet(
            "beta",
            "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded",
            Some("maidsafe".to_string()),
            Some("custom_branch".to_string()),
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}
