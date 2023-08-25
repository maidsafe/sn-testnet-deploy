// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::super::{CloudProvider, TestnetDeploy};
use super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::rpc_client::MockRpcClientInterface;
use crate::s3::MockS3RepositoryInterface;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use color_eyre::Result;
use mockall::predicate::*;

/// This module used to have two tests, related to the `use_custom_bin` flag being set to either
/// true or false. However, now that we've added the faucet into the deploy process, we always need
/// the build machine, so we just have one test.
#[tokio::test]
async fn should_run_terraform_apply_with_custom_bin_set() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut s3_repository = MockS3RepositoryInterface::new();
    s3_repository.expect_download_object().times(0);
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
        Box::new(MockSshClientInterface::new()),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
        Box::new(s3_repository),
    );

    testnet.create_infra("beta", 30, true).await?;

    drop(tmp_dir);
    Ok(())
}
