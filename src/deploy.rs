// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{AnsibleInventoryType, AnsiblePlaybook, ExtraVarsDocBuilder},
    error::{Error, Result},
    get_genesis_multiaddr, print_duration, BinaryOption, TestnetDeploy,
};
use colored::Colorize;
use std::{net::SocketAddr, time::Instant};

pub struct DeployCmd {
    testnet_deploy: TestnetDeploy,
    name: String,
    node_count: u16,
    vm_count: u16,
    public_rpc: bool,
    logstash_details: Option<(String, Vec<SocketAddr>)>,
    binary_option: BinaryOption,
    env_variables: Option<Vec<(String, String)>>,
}

impl DeployCmd {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        testnet_deploy: TestnetDeploy,
        name: String,
        node_count: u16,
        vm_count: u16,
        public_rpc: bool,
        logstash_details: Option<(String, Vec<SocketAddr>)>,
        binary_option: BinaryOption,
        env_variables: Option<Vec<(String, String)>>,
    ) -> Self {
        Self {
            testnet_deploy,
            name,
            node_count,
            vm_count,
            public_rpc,
            logstash_details,
            binary_option,
            env_variables,
        }
    }

    pub async fn execute(self) -> Result<()> {
        let build_custom_binaries = {
            match &self.binary_option {
                BinaryOption::BuildFromSource { .. } => true,
                BinaryOption::Versioned { .. } => false,
            }
        };
        self.create_infra(build_custom_binaries)
            .await
            .map_err(|err| {
                println!("Failed to create infra {err:?}");
                err
            })?;

        let mut n = 1;
        let total = if build_custom_binaries { 5 } else { 4 };
        if build_custom_binaries {
            self.print_ansible_run_banner(n, total, "Build Custom Binaries");
            self.build_safe_network_binaries().await.map_err(|err| {
                println!("Failed to build safe network binaries {err:?}");
                err
            })?;
            n += 1;
        }

        self.print_ansible_run_banner(n, total, "Provision Genesis Node");
        self.provision_genesis_node().await.map_err(|err| {
            println!("Failed to provision genesis node {err:?}");
            err
        })?;
        n += 1;

        let (genesis_multiaddr, _) = get_genesis_multiaddr(
            &self.name,
            &self.testnet_deploy.ansible_runner,
            &self.testnet_deploy.ssh_client,
        )
        .await
        .map_err(|err| {
            println!("Failed to get genesis multiaddr {err:?}");
            err
        })?;
        println!("Obtained multiaddr for genesis node: {genesis_multiaddr}");

        let mut node_provision_failed = false;
        self.print_ansible_run_banner(n, total, "Provision Remaining Nodes");
        let result = self.provision_remaining_nodes(&genesis_multiaddr).await;
        match result {
            Ok(()) => {
                println!("Provisioned all remaining nodes");
            }
            Err(_) => {
                node_provision_failed = true;
            }
        }
        n += 1;

        self.print_ansible_run_banner(n, total, "Deploy Faucet");
        self.provision_faucet(&genesis_multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to provision faucet {err:?}");
                err
            })?;
        n += 1;

        self.print_ansible_run_banner(n, total, "Provision RPC Client on Genesis Node");
        self.provision_safenode_rpc_client(&genesis_multiaddr)
            .await
            .map_err(|err| {
                println!("Failed to provision safenode rpc client {err:?}");
                err
            })?;

        if node_provision_failed {
            println!();
            println!("{}", "WARNING!".yellow());
            println!("Some nodes failed to provision without error.");
            println!("This usually means a small number of nodes failed to start on a few VMs.");
            println!("However, most of the time the deployment will still be usable.");
            println!("See the output from Ansible to determine which VMs had failures.");
        }

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
        let build_inventory = self
            .testnet_deploy
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Build, true)
            .await?;
        let build_ip = build_inventory[0].1;
        self.testnet_deploy.ssh_client.wait_for_ssh_availability(
            &build_ip,
            &self.testnet_deploy.cloud_provider.get_ssh_user(),
        )?;

        println!("Running ansible against build VM...");
        let extra_vars = self.build_binaries_extra_vars_doc()?;
        self.testnet_deploy.ansible_runner.run_playbook(
            AnsiblePlaybook::Build,
            AnsibleInventoryType::Build,
            Some(extra_vars),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_genesis_node(&self) -> Result<()> {
        let start = Instant::now();
        let genesis_inventory = self
            .testnet_deploy
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Genesis, true)
            .await?;
        let genesis_ip = genesis_inventory[0].1;
        self.testnet_deploy.ssh_client.wait_for_ssh_availability(
            &genesis_ip,
            &self.testnet_deploy.cloud_provider.get_ssh_user(),
        )?;
        self.testnet_deploy.ansible_runner.run_playbook(
            AnsiblePlaybook::Genesis,
            AnsibleInventoryType::Genesis,
            Some(self.build_node_extra_vars_doc(None, None)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_faucet(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to deploy faucet...");
        self.testnet_deploy.ansible_runner.run_playbook(
            AnsiblePlaybook::Faucet,
            AnsibleInventoryType::Genesis,
            Some(self.build_faucet_extra_vars_doc(genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_safenode_rpc_client(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        println!("Running ansible against genesis node to start safenode_rpc_client service...");
        self.testnet_deploy.ansible_runner.run_playbook(
            AnsiblePlaybook::RpcClient,
            AnsibleInventoryType::Genesis,
            Some(self.build_safenode_rpc_client_extra_vars_doc(genesis_multiaddr)?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn provision_remaining_nodes(&self, genesis_multiaddr: &str) -> Result<()> {
        let start = Instant::now();
        self.testnet_deploy.ansible_runner.run_playbook(
            AnsiblePlaybook::Nodes,
            AnsibleInventoryType::Nodes,
            Some(self.build_node_extra_vars_doc(
                Some(genesis_multiaddr.to_string()),
                Some(self.node_count),
            )?),
        )?;
        print_duration(start.elapsed());
        Ok(())
    }

    /// Helpers

    fn print_ansible_run_banner(&self, n: usize, total: usize, s: &str) {
        let ansible_run_msg = format!("Ansible Run {} of {}: ", n, total);
        let line = "=".repeat(s.len() + ansible_run_msg.len());
        println!("{}\n{}{}\n{}", line, ansible_run_msg, s, line);
    }

    fn build_binaries_extra_vars_doc(&self) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_build_variables(&self.name, &self.binary_option);
        Ok(extra_vars.build())
    }

    fn build_node_extra_vars_doc(
        &self,
        genesis_multiaddr: Option<String>,
        node_instance_count: Option<u16>,
    ) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.testnet_deploy.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &self.name);
        if genesis_multiaddr.is_some() {
            extra_vars.add_variable(
                "genesis_multiaddr",
                &genesis_multiaddr.ok_or_else(|| Error::GenesisMultiAddrNotSupplied)?,
            );
        }
        if node_instance_count.is_some() {
            extra_vars.add_variable(
                "node_instance_count",
                &node_instance_count.unwrap_or(20).to_string(),
            );
        }
        if self.public_rpc {
            extra_vars.add_variable("public_rpc", "true");
        }

        extra_vars.add_node_url_or_version(&self.name, &self.binary_option);
        extra_vars.add_node_manager_url(&self.name, &self.binary_option);
        extra_vars.add_node_manager_daemon_url(&self.name, &self.binary_option);

        if let Some(env_vars) = &self.env_variables {
            extra_vars.add_env_variable_list("env_variables", env_vars.clone());
        }

        if let Some((logstash_stack_name, logstash_hosts)) = &self.logstash_details {
            extra_vars.add_variable("logstash_stack_name", logstash_stack_name);
            extra_vars.add_list_variable(
                "logstash_hosts",
                logstash_hosts
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            );
        }

        Ok(extra_vars.build())
    }

    fn build_faucet_extra_vars_doc(&self, genesis_multiaddr: &str) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.testnet_deploy.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &self.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_faucet_url_or_version(&self.name, &self.binary_option);
        Ok(extra_vars.build())
    }

    fn build_safenode_rpc_client_extra_vars_doc(&self, genesis_multiaddr: &str) -> Result<String> {
        let mut extra_vars = ExtraVarsDocBuilder::default();
        extra_vars.add_variable("provider", &self.testnet_deploy.cloud_provider.to_string());
        extra_vars.add_variable("testnet_name", &self.name);
        extra_vars.add_variable("genesis_multiaddr", genesis_multiaddr);
        extra_vars.add_rpc_client_url_or_version(&self.name, &self.binary_option);
        Ok(extra_vars.build())
    }
}
