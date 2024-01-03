// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    print_duration, TestnetDeploy,
};
use std::{net::SocketAddr, path::PathBuf, time::Instant};

pub struct DeployCmd {
    testnet_deploy: TestnetDeploy,
    name: String,
    node_count: u16,
    vm_count: u16,
    logstash_details: (String, Vec<SocketAddr>),
    // (repo_owner, branch)
    custom_branch_details: Option<(String, String)>,
    // (safe_version, safenode_version)
    custom_version_details: Option<(String, String)>,
}

impl DeployCmd {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        testnet_deploy: TestnetDeploy,
        name: String,
        node_count: u16,
        vm_count: u16,
        logstash_details: (String, Vec<SocketAddr>),
        custom_branch_details: Option<(String, String)>,
        custom_version_details: Option<(String, String)>,
    ) -> Self {
        Self {
            testnet_deploy,
            name,
            node_count,
            vm_count,
            logstash_details,
            custom_branch_details,
            custom_version_details,
        }
    }

    pub async fn deploy(self) -> Result<()> {
        self.create_infra(self.custom_branch_details.is_some())
            .await
            .map_err(|err| {
                println!("Failed to create infra {err:?}");
                err
            })?;
        if self.custom_branch_details.is_some() {
            self.build_safe_network_binaries().await.map_err(|err| {
                println!("Failed to build safe network binaries {err:?}");
                err
            })?;
        }

        self.provision_genesis_node().await.map_err(|err| {
            println!("Failed to provision genesis node {err:?}");
            err
        })?;

        let (multiaddr, _) = self
            .testnet_deploy
            .get_genesis_multiaddr(&self.name)
            .await
            .map_err(|err| {
                println!("Failed to get genesis multiaddr {err:?}");
                err
            })?;
        println!("Obtained multiaddr for genesis node: {multiaddr}");

        self.provision_remaining_nodes(&multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to provision remaining nodes {err:?}");
                err
            })?;

        self.provision_faucet(&multiaddr).await.map_err(|err| {
            println!("Failed to provision faucet {err:?}");
            err
        })?;

        self.provision_safenode_rpc_client(&multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to provision safenode rpc client {err:?}");
                err
            })?;

        self.testnet_deploy
            .list_inventory(
                &self.name,
                true,
                self.custom_branch_details,
                self.custom_version_details,
                Some(self.node_count),
            )
            .await
            .map_err(|err| {
                println!("Failed to list inventory {err:?}");
                err
            })?;

        Ok(())
    }

    async fn create_infra(&self, enable_build_vm: bool) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {} workspace...", self.name);
        self.testnet_deploy
            .terraform_runner
            .workspace_select(&self.name)?;
        let args = vec![
            ("node_count".to_string(), self.vm_count.to_string()),
            ("use_custom_bin".to_string(), enable_build_vm.to_string()),
        ];
        println!("Running terraform apply...");
        self.testnet_deploy.terraform_runner.apply(args)?;
        print_duration(start.elapsed());
        Ok(())
    }

    async fn build_safe_network_binaries(&self) -> Result<()> {
        let start = Instant::now();
        println!("Obtaining IP address for build VM...");
        let build_inventory = self.testnet_deploy.ansible_runner.inventory_list(
            PathBuf::from("inventory")
                .join(format!(".{}_build_inventory_digital_ocean.yml", self.name)),
        )?;
        let build_ip = build_inventory[0].1;
        self.testnet_deploy.ssh_client.wait_for_ssh_availability(
            &build_ip,
            &self.testnet_deploy.cloud_provider.get_ssh_user(),
        )?;

        println!("Running ansible against build VM...");
        let extra_vars = self.build_binaries_extra_vars_doc()?;
        self.testnet_deploy.ansible_runner.run_playbook(
            PathBuf::from("build.yml"),
            PathBuf::from("inventory")
                .join(format!(".{}_build_inventory_digital_ocean.yml", self.name)),
            self.testnet_deploy.cloud_provider.get_ssh_user(),
            Some(extra_vars),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_genesis_node(&self) -> Result<()> {
        let start = Instant::now();
        let genesis_inventory =
            self.testnet_deploy
                .ansible_runner
                .inventory_list(PathBuf::from("inventory").join(format!(
                    ".{}_genesis_inventory_digital_ocean.yml",
                    self.name
                )))?;
        let genesis_ip = genesis_inventory[0].1;
        self.testnet_deploy.ssh_client.wait_for_ssh_availability(
            &genesis_ip,
            &self.testnet_deploy.cloud_provider.get_ssh_user(),
        )?;
        println!("Running ansible against genesis node...");
        self.testnet_deploy.ansible_runner.run_playbook(
            PathBuf::from("genesis_node.yml"),
            PathBuf::from("inventory").join(format!(
                ".{}_genesis_inventory_digital_ocean.yml",
                self.name
            )),
            self.testnet_deploy.cloud_provider.get_ssh_user(),
            Some(self.build_node_extra_vars_doc(None, None)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_faucet(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy faucet...");
        self.testnet_deploy.ansible_runner.run_playbook(
            PathBuf::from("faucet.yml"),
            PathBuf::from("inventory").join(format!(
                ".{}_genesis_inventory_digital_ocean.yml",
                self.name
            )),
            self.testnet_deploy.cloud_provider.get_ssh_user(),
            Some(self.build_faucet_extra_vars_doc(genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_safenode_rpc_client(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy safenode_rpc_client...");
        self.testnet_deploy.ansible_runner.run_playbook(
            PathBuf::from("safenode_rpc_client.yml"),
            PathBuf::from("inventory").join(format!(
                ".{}_genesis_inventory_digital_ocean.yml",
                self.name
            )),
            self.testnet_deploy.cloud_provider.get_ssh_user(),
            Some(self.build_safenode_rpc_client_extra_vars_doc(genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_remaining_nodes(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against remaining nodes...");
        self.testnet_deploy.ansible_runner.run_playbook(
            PathBuf::from("nodes.yml"),
            PathBuf::from("inventory")
                .join(format!(".{}_node_inventory_digital_ocean.yml", self.name)),
            self.testnet_deploy.cloud_provider.get_ssh_user(),
            Some(self.build_node_extra_vars_doc(
                Some(genesis_multiaddr.to_string()),
                Some(self.node_count),
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    /// Helpers

    fn build_binaries_extra_vars_doc(&self) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");

        Self::add_value(&mut extra_vars, "custom_bin", "true");
        Self::add_value(&mut extra_vars, "testnet_name", &self.name);
        if let Some((repo_owner, branch)) = &self.custom_branch_details {
            Self::add_value(&mut extra_vars, "branch", branch);
            Self::add_value(&mut extra_vars, "org", repo_owner);
        }

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");

        Ok(extra_vars)
    }

    fn build_node_extra_vars_doc(
        &self,
        genesis_multiaddr: Option<String>,
        node_instance_count: Option<u16>,
    ) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");
        Self::add_value(
            &mut extra_vars,
            "provider",
            &self.testnet_deploy.cloud_provider.to_string(),
        );
        Self::add_value(&mut extra_vars, "testnet_name", &self.name);
        if genesis_multiaddr.is_some() {
            Self::add_value(
                &mut extra_vars,
                "genesis_multiaddr",
                &genesis_multiaddr.ok_or_else(|| Error::GenesisMultiAddrNotSupplied)?,
            );
        }
        if node_instance_count.is_some() {
            Self::add_value(
                &mut extra_vars,
                "node_instance_count",
                &node_instance_count.unwrap_or(20).to_string(),
            );
        }
        if let Some((repo_owner, branch)) = &self.custom_branch_details {
            Self::add_value(
            &mut extra_vars,
            "node_archive_url",
            &format!(
                "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/safenode-{}-x86_64-unknown-linux-musl.tar.gz",
                repo_owner,
                branch,
                self.name),
        );
        }
        if let Some((_, safenode_version)) = &self.custom_version_details {
            Self::add_value(
            &mut extra_vars,
            "node_archive_url",
            &format!(
                "https://github.com/maidsafe/safe_network/releases/download/sn_node-v{safenode_version}/safenode-{safenode_version}-x86_64-unknown-linux-musl.tar.gz",
            ),
        );
        }

        let (logstash_stack_name, logstash_hosts) = &self.logstash_details;
        Self::add_value(&mut extra_vars, "logstash_stack_name", logstash_stack_name);
        extra_vars.push_str("\"logstash_hosts\": [");
        for host in logstash_hosts.iter() {
            extra_vars.push_str(&format!("\"{}\", ", host));
        }
        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str("] }");
        Ok(extra_vars)
    }

    fn build_faucet_extra_vars_doc(&self, genesis_multiaddr: &str) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");
        Self::add_value(
            &mut extra_vars,
            "provider",
            &self.testnet_deploy.cloud_provider.to_string(),
        );
        Self::add_value(&mut extra_vars, "testnet_name", &self.name);
        Self::add_value(&mut extra_vars, "genesis_multiaddr", genesis_multiaddr);
        if let Some((repo_owner, branch)) = &self.custom_branch_details {
            Self::add_value(&mut extra_vars, "branch", branch);
            Self::add_value(&mut extra_vars, "org", repo_owner);
            Self::add_value(
            &mut extra_vars,
            "faucet_archive_url",
            &format!(
                "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/faucet-{}-x86_64-unknown-linux-musl.tar.gz",
                repo_owner,
                branch,
                &self.name),
        );
        } else {
            Self::add_value(
            &mut extra_vars,
            "faucet_archive_url",
            "https://sn-faucet.s3.eu-west-2.amazonaws.com/faucet-latest-x86_64-unknown-linux-musl.tar.gz",
        );
        }

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");
        Ok(extra_vars)
    }

    fn build_safenode_rpc_client_extra_vars_doc(&self, genesis_multiaddr: &str) -> Result<String> {
        let mut extra_vars = String::new();
        extra_vars.push_str("{ ");
        Self::add_value(
            &mut extra_vars,
            "provider",
            &self.testnet_deploy.cloud_provider.to_string(),
        );
        Self::add_value(&mut extra_vars, "testnet_name", &self.name);
        Self::add_value(&mut extra_vars, "genesis_multiaddr", genesis_multiaddr);
        if let Some((repo_owner, branch)) = &self.custom_branch_details {
            Self::add_value(&mut extra_vars, "branch", branch);
            Self::add_value(&mut extra_vars, "org", repo_owner);
            Self::add_value(
            &mut extra_vars,
            "safenode_rpc_client_archive_url",
            &format!(
                "https://sn-node.s3.eu-west-2.amazonaws.com/{}/{}/safenode_rpc_client-{}-x86_64-unknown-linux-musl.tar.gz",
                repo_owner,
                branch,
                &self.name),
        );
        } else {
            Self::add_value(
            &mut extra_vars,
            "safenode_rpc_client_archive_url",
            "https://sn-node-rpc-client.s3.eu-west-2.amazonaws.com/safenode_rpc_client-latest-x86_64-unknown-linux-musl.tar.gz",
        );
        }

        let mut extra_vars = extra_vars.strip_suffix(", ").unwrap().to_string();
        extra_vars.push_str(" }");
        Ok(extra_vars)
    }

    fn add_value(document: &mut String, name: &str, value: &str) {
        document.push_str(&format!("\"{name}\": \"{value}\", "))
    }
}
