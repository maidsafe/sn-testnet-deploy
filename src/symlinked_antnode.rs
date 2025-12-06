// Copyright (c) 2023, MaidSafe.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        extra_vars::build_symlinked_antnode_extra_vars,
        inventory::{generate_environment_inventory, AnsibleInventoryType},
        provisioning::{AnsibleProvisioner, ProvisionOptions},
        AnsiblePlaybook, AnsibleRunner,
    },
    error::Result,
    is_binary_on_path,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
    BinaryOption, CloudProvider, EvmNetwork, LogFormat,
};
use std::{env, path::PathBuf, time::Duration};

pub struct SymlinkedAntnodeDeployer {
    ansible_provisioner: AnsibleProvisioner,
    cloud_provider: CloudProvider,
    name: String,
    terraform_runner: TerraformRunner,
    working_directory: PathBuf,
}

impl SymlinkedAntnodeDeployer {
    pub fn new(
        name: String,
        cloud_provider: CloudProvider,
        _s3_repository: S3Repository,
        ansible_verbose_mode: bool,
    ) -> Result<Self> {
        let working_directory = env::current_dir()?;

        let terraform_binary_path = if is_binary_on_path("tofu") {
            PathBuf::from("tofu")
        } else {
            PathBuf::from("terraform")
        };

        let state_bucket_name = env::var("TERRAFORM_STATE_BUCKET_NAME")?;
        let ssh_secret_key_path = PathBuf::from(env::var("SSH_KEY_PATH")?);
        let vault_password_path = PathBuf::from(env::var("ANSIBLE_VAULT_PASSWORD_PATH")?);

        let terraform_working_dir = match cloud_provider {
            CloudProvider::Aws => {
                working_directory.join("resources/terraform/symlinked-antnode/aws")
            }
            CloudProvider::DigitalOcean => {
                working_directory.join("resources/terraform/symlinked-antnode/digital-ocean")
            }
        };

        let terraform_runner = TerraformRunner::new(
            terraform_binary_path,
            terraform_working_dir,
            cloud_provider,
            &state_bucket_name,
        )?;

        let ansible_runner = AnsibleRunner::new(
            50,
            ansible_verbose_mode,
            &name,
            cloud_provider,
            ssh_secret_key_path.clone(),
            vault_password_path,
            working_directory.join("resources/ansible"),
        )?;

        let ssh_client = SshClient::new(ssh_secret_key_path);
        let ansible_provisioner =
            AnsibleProvisioner::new(ansible_runner, cloud_provider, ssh_client);

        Ok(Self {
            ansible_provisioner,
            cloud_provider,
            name,
            terraform_runner,
            working_directory,
        })
    }

    pub fn init(&self) -> Result<()> {
        self.terraform_runner.init()
    }

    pub async fn create_infrastructure(
        &self,
        region: &str,
        vm_size: Option<String>,
        volume_size: Option<u16>,
        use_custom_bin: bool,
    ) -> Result<()> {
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&self.name) {
            self.terraform_runner.workspace_new(&self.name)?;
        } else {
            println!("Workspace {} already exists", self.name);
            self.terraform_runner.workspace_select(&self.name)?;
        }

        let mut vars = vec![
            ("region".to_string(), region.to_string()),
            ("use_custom_bin".to_string(), use_custom_bin.to_string()),
        ];

        if let Some(size) = vm_size {
            vars.push(("droplet_size".to_string(), size));
        }

        if let Some(size) = volume_size {
            vars.push(("volume_size".to_string(), size.to_string()));
        }

        self.terraform_runner
            .apply(vars, Some(vec!["dev.tfvars".to_string()]))?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn provision(
        &self,
        binary_option: &BinaryOption,
        antnode_count: u16,
        rewards_address: &str,
        evm_network_type: EvmNetwork,
        evm_data_payments_address: Option<String>,
        evm_payment_token_address: Option<String>,
        evm_rpc_url: Option<String>,
        peer: Option<String>,
        network_contacts_url: Option<String>,
        network_id: Option<u8>,
    ) -> Result<()> {
        println!("Generating Ansible inventory...");
        let base_inventory_path = self
            .working_directory
            .join("resources/ansible/inventory/dev_inventory_digital_ocean.yml");
        let output_inventory_dir_path = self.working_directory.join("resources/ansible/inventory");
        generate_environment_inventory(
            &self.name,
            &base_inventory_path,
            &output_inventory_dir_path,
        )?;

        let build_custom_binaries = binary_option.should_provision_build_machine();
        if build_custom_binaries {
            println!("Building custom binaries...");
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");

            let output_inventory_dir_path =
                self.working_directory.join("resources/ansible/inventory");

            let provision_options = ProvisionOptions {
                ant_version: None,
                binary_option: binary_option.clone(),
                chunk_size: None,
                chunk_tracker_data_addresses: None,
                chunk_tracker_services: None,
                client_env_variables: None,
                delayed_verifier_batch_size: None,
                disable_nodes: false,
                delayed_verifier_quorum_value: None,
                start_delayed_verifier: false,
                enable_logging: true,
                enable_metrics: true,
                start_random_verifier: false,
                start_performance_verifier: false,
                start_uploaders: false,
                evm_data_payments_address: evm_data_payments_address.clone(),
                evm_merkle_payments_address: None,
                evm_network: evm_network_type.clone(),
                evm_payment_token_address: evm_payment_token_address.clone(),
                evm_rpc_url: evm_rpc_url.clone(),
                expected_hash: None,
                expected_size: None,
                file_address: None,
                full_cone_private_node_count: 0,
                funding_wallet_secret_key: None,
                gas_amount: None,
                interval: Some(Duration::from_secs(5)),
                log_format: Some(LogFormat::Default),
                max_archived_log_files: 5,
                max_log_files: 10,
                max_uploads: None,
                merkle: false,
                name: self.name.clone(),
                network_id,
                network_dashboard_branch: None,
                node_count: antnode_count,
                node_env_variables: None,
                output_inventory_dir_path: output_inventory_dir_path.clone(),
                peer_cache_node_count: 0,
                performance_verifier_batch_size: None,
                port_restricted_cone_private_node_count: 0,
                public_rpc: false,
                random_verifier_batch_size: None,
                repair_service_count: 0,
                data_retrieval_service_count: 0,
                rewards_address: Some(rewards_address.to_string()),
                scan_frequency: None,
                sleep_duration: None,
                single_node_payment: false,
                start_chunk_trackers: false,
                start_data_retrieval: false,
                symmetric_private_node_count: 0,
                token_amount: None,
                upload_batch_size: None,
                upload_size: None,
                upload_interval: None,
                uploaders_count: None,
                upnp_private_node_count: 0,
                wallet_secret_keys: None,
            };
            self.ansible_provisioner
                .build_autonomi_binaries(&provision_options, None)?;
        }

        let extra_vars = build_symlinked_antnode_extra_vars(
            &self.cloud_provider.to_string(),
            binary_option,
            antnode_count,
            rewards_address,
            evm_network_type,
            evm_data_payments_address,
            evm_payment_token_address,
            evm_rpc_url,
            peer,
            network_contacts_url,
            network_id,
            &self.name,
        )?;
        self.ansible_provisioner.ansible_runner.run_playbook(
            AnsiblePlaybook::SymlinkedNodes,
            AnsibleInventoryType::Nodes,
            Some(extra_vars),
        )?;

        Ok(())
    }

    pub async fn destroy(&self) -> Result<()> {
        self.terraform_runner.workspace_select(&self.name)?;
        self.terraform_runner.destroy(None, None)?;
        Ok(())
    }
}
