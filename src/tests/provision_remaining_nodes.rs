// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::super::logstash::LOGSTASH_PORT;
use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::ssh::MockSshClientInterface;
use color_eyre::Result;
use mockall::predicate::*;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

const CUSTOM_BIN_URL: &str = "https://sn-node.s3.eu-west-2.amazonaws.com/maidsafe/custom_branch/safenode-beta-x86_64-unknown-linux-musl.tar.gz";

#[tokio::test]
async fn should_run_ansible_against_the_remaining_nodes() -> Result<()> {
    let extra_vars_doc = r#"{ "provider": "digital-ocean", "testnet_name": "beta", "genesis_multiaddr": "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded", "node_instance_count": "30", "logstash_stack_name": "main", "logstash_hosts": ["10.0.0.1:5044", "10.0.0.2:5044"] }"#;
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository("beta", &working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("nodes.yml")),
            eq(PathBuf::from("inventory").join(".beta_node_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(extra_vars_doc.to_string())),
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
        .provision_remaining_nodes(
            "beta",
            (
                "main",
                &[
                    SocketAddr::new(IpAddr::V4("10.0.0.1".parse()?), LOGSTASH_PORT),
                    SocketAddr::new(IpAddr::V4("10.0.0.2".parse()?), LOGSTASH_PORT),
                ],
            ),
            "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded"
                .to_string(),
            30,
            None,
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn should_run_ansible_against_the_remaining_nodes_with_a_custom_binary() -> Result<()> {
    let extra_vars_doc = r#"{ "provider": "digital-ocean", "testnet_name": "beta", "genesis_multiaddr": "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded", "node_instance_count": "30", "node_archive_url": "CUSTOM_BIN_URL", "logstash_stack_name": "main", "logstash_hosts": ["10.0.0.1:5044", "10.0.0.2:5044"] }"#;
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let s3_repository = setup_default_s3_repository("beta", &working_dir)?;
    let mut ansible_runner = MockAnsibleRunnerInterface::new();
    ansible_runner
        .expect_run_playbook()
        .times(1)
        .with(
            eq(PathBuf::from("nodes.yml")),
            eq(PathBuf::from("inventory").join(".beta_node_inventory_digital_ocean.yml")),
            eq("root".to_string()),
            eq(Some(
                extra_vars_doc.replace("CUSTOM_BIN_URL", CUSTOM_BIN_URL),
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
        .provision_remaining_nodes(
            "beta",
            (
                "main",
                &[
                    SocketAddr::new(IpAddr::V4("10.0.0.1".parse()?), LOGSTASH_PORT),
                    SocketAddr::new(IpAddr::V4("10.0.0.2".parse()?), LOGSTASH_PORT),
                ],
            ),
            "/ip4/10.0.0.10/tcp/12000/p2p/12D3KooWLvmkUDQRthtZv9CrzozRLk9ZVEHXgmx6UxVMiho5aded"
                .to_string(),
            30,
            Some(("maidsafe".to_string(), "custom_branch".to_string())),
        )
        .await?;

    drop(tmp_dir);
    Ok(())
}
