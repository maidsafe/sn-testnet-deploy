// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{
        inventory::cleanup_environment_inventory,
        provisioning::{AnsibleProvisioner, ProvisionOptions},
        AnsibleRunner,
    },
    error::{Error, Result},
    get_environment_details,
    infra::ClientsInfraRunOptions,
    inventory::ClientsDeploymentInventory,
    print_duration,
    s3::S3Repository,
    ssh::SshClient,
    terraform::TerraformRunner,
    write_environment_details, BinaryOption, CloudProvider, DeploymentType, EnvironmentDetails,
    EnvironmentType, EvmDetails,
};
use alloy::primitives::U256;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Instant};

const ANSIBLE_DEFAULT_FORKS: usize = 50;

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientsDeployOptions {
    pub binary_option: BinaryOption,
    pub chunk_size: Option<u64>,
    pub chunk_tracker_data_addresses: Vec<String>,
    pub chunk_tracker_services: u16,
    pub client_env_variables: Option<Vec<(String, String)>>,
    pub client_vm_count: Option<u16>,
    pub client_vm_size: Option<String>,
    pub current_inventory: ClientsDeploymentInventory,
    pub delayed_verifier_batch_size: Option<u16>,
    pub delayed_verifier_quorum_value: Option<String>,
    pub enable_metrics: bool,
    pub environment_type: EnvironmentType,
    pub evm_details: EvmDetails,
    pub file_address: Option<String>,
    pub expected_hash: Option<String>,
    pub expected_size: Option<u64>,
    pub funding_wallet_secret_key: Option<String>,
    pub initial_gas: Option<U256>,
    pub initial_tokens: Option<U256>,
    pub max_archived_log_files: u16,
    pub max_log_files: u16,
    pub max_uploads: Option<u32>,
    pub merkle: bool,
    pub name: String,
    pub network_id: Option<u8>,
    pub network_contacts_url: Option<String>,
    pub output_inventory_dir_path: PathBuf,
    pub peer: Option<String>,
    pub performance_verifier_batch_size: Option<u16>,
    pub random_verifier_batch_size: Option<u16>,
    pub repair_service_count: u16,
    pub data_retrieval_service_count: u16,
    pub run_chunk_trackers_provision: bool,
    pub run_data_retrieval_provision: bool,
    pub run_downloaders_provision: bool,
    pub run_repair_files_provision: bool,
    pub run_scan_repair_provision: bool,
    pub run_uploaders_provision: bool,
    pub scan_frequency: Option<u64>,
    pub sleep_duration: Option<u16>,
    pub sleep_interval: Option<u64>,
    pub start_chunk_trackers: bool,
    pub start_data_retrieval: bool,
    pub start_delayed_verifier: bool,
    pub start_performance_verifier: bool,
    pub start_random_verifier: bool,
    pub start_repair_service: bool,
    pub start_uploaders: bool,
    pub uploaders_count: u16,
    pub upload_size: Option<u16>,
    pub upload_interval: u16,
    pub upload_batch_size: Option<u16>,
    pub wallet_secret_keys: Option<Vec<String>>,
}

#[derive(Default)]
pub struct ClientsDeployBuilder {
    ansible_forks: Option<usize>,
    ansible_verbose_mode: bool,
    deployment_type: EnvironmentType,
    environment_name: String,
    provider: Option<CloudProvider>,
    region: Option<String>,
    ssh_secret_key_path: Option<PathBuf>,
    state_bucket_name: Option<String>,
    terraform_binary_path: Option<PathBuf>,
    vault_password_path: Option<PathBuf>,
    working_directory_path: Option<PathBuf>,
}

impl ClientsDeployBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn ansible_verbose_mode(&mut self, ansible_verbose_mode: bool) -> &mut Self {
        self.ansible_verbose_mode = ansible_verbose_mode;
        self
    }

    pub fn ansible_forks(&mut self, ansible_forks: usize) -> &mut Self {
        self.ansible_forks = Some(ansible_forks);
        self
    }

    pub fn deployment_type(&mut self, deployment_type: EnvironmentType) -> &mut Self {
        self.deployment_type = deployment_type;
        self
    }

    pub fn environment_name(&mut self, name: &str) -> &mut Self {
        self.environment_name = name.to_string();
        self
    }

    pub fn provider(&mut self, provider: CloudProvider) -> &mut Self {
        self.provider = Some(provider);
        self
    }

    pub fn state_bucket_name(&mut self, state_bucket_name: String) -> &mut Self {
        self.state_bucket_name = Some(state_bucket_name);
        self
    }

    pub fn terraform_binary_path(&mut self, terraform_binary_path: PathBuf) -> &mut Self {
        self.terraform_binary_path = Some(terraform_binary_path);
        self
    }

    pub fn working_directory(&mut self, working_directory_path: PathBuf) -> &mut Self {
        self.working_directory_path = Some(working_directory_path);
        self
    }

    pub fn ssh_secret_key_path(&mut self, ssh_secret_key_path: PathBuf) -> &mut Self {
        self.ssh_secret_key_path = Some(ssh_secret_key_path);
        self
    }

    pub fn vault_password_path(&mut self, vault_password_path: PathBuf) -> &mut Self {
        self.vault_password_path = Some(vault_password_path);
        self
    }

    pub fn region(&mut self, region: String) -> &mut Self {
        self.region = Some(region);
        self
    }

    pub fn build(&self) -> Result<ClientsDeployer> {
        let provider = self.provider.unwrap_or(CloudProvider::DigitalOcean);
        match provider {
            CloudProvider::DigitalOcean => {
                let digital_ocean_pat = std::env::var("DO_PAT").map_err(|_| {
                    Error::CloudProviderCredentialsNotSupplied("DO_PAT".to_string())
                })?;
                // The DO_PAT variable is not actually read by either Terraform or Ansible.
                // Each tool uses a different variable, so instead we set each of those variables
                // to the value of DO_PAT. This means the user only needs to set one variable.
                std::env::set_var("DIGITALOCEAN_TOKEN", digital_ocean_pat.clone());
                std::env::set_var("DO_API_TOKEN", digital_ocean_pat);
            }
            _ => {
                return Err(Error::CloudProviderNotSupported(provider.to_string()));
            }
        }

        let state_bucket_name = match self.state_bucket_name {
            Some(ref bucket_name) => bucket_name.clone(),
            None => std::env::var("CLIENT_TERRAFORM_STATE_BUCKET_NAME")?,
        };

        let default_terraform_bin_path = PathBuf::from("terraform");
        let terraform_binary_path = self
            .terraform_binary_path
            .as_ref()
            .unwrap_or(&default_terraform_bin_path);

        let working_directory_path = match self.working_directory_path {
            Some(ref work_dir_path) => work_dir_path.clone(),
            None => std::env::current_dir()?.join("resources"),
        };

        let ssh_secret_key_path = match self.ssh_secret_key_path {
            Some(ref ssh_sk_path) => ssh_sk_path.clone(),
            None => PathBuf::from(std::env::var("SSH_KEY_PATH")?),
        };

        let vault_password_path = match self.vault_password_path {
            Some(ref vault_pw_path) => vault_pw_path.clone(),
            None => PathBuf::from(std::env::var("ANSIBLE_VAULT_PASSWORD_PATH")?),
        };

        let region = match self.region {
            Some(ref region) => region.clone(),
            None => "lon1".to_string(),
        };

        let terraform_runner = TerraformRunner::new(
            terraform_binary_path.to_path_buf(),
            working_directory_path
                .join("terraform")
                .join("clients")
                .join(provider.to_string()),
            provider,
            &state_bucket_name,
        )?;

        let ansible_runner = AnsibleRunner::new(
            self.ansible_forks.unwrap_or(ANSIBLE_DEFAULT_FORKS),
            self.ansible_verbose_mode,
            &self.environment_name,
            provider,
            ssh_secret_key_path.clone(),
            vault_password_path,
            working_directory_path.join("ansible"),
        )?;

        let ssh_client = SshClient::new(ssh_secret_key_path);
        let ansible_provisioner =
            AnsibleProvisioner::new(ansible_runner, provider, ssh_client.clone());

        let client_deployer = ClientsDeployer::new(
            ansible_provisioner,
            provider,
            self.deployment_type.clone(),
            &self.environment_name,
            S3Repository {},
            ssh_client,
            terraform_runner,
            working_directory_path,
            region,
        )?;

        Ok(client_deployer)
    }
}

#[derive(Clone)]
pub struct ClientsDeployer {
    pub ansible_provisioner: AnsibleProvisioner,
    pub cloud_provider: CloudProvider,
    pub deployment_type: EnvironmentType,
    pub environment_name: String,
    pub inventory_file_path: PathBuf,
    pub region: String,
    pub s3_repository: S3Repository,
    pub ssh_client: SshClient,
    pub terraform_runner: TerraformRunner,
    pub working_directory_path: PathBuf,
}

impl ClientsDeployer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ansible_provisioner: AnsibleProvisioner,
        cloud_provider: CloudProvider,
        deployment_type: EnvironmentType,
        environment_name: &str,
        s3_repository: S3Repository,
        ssh_client: SshClient,
        terraform_runner: TerraformRunner,
        working_directory_path: PathBuf,
        region: String,
    ) -> Result<ClientsDeployer> {
        if environment_name.is_empty() {
            return Err(Error::EnvironmentNameRequired);
        }
        let inventory_file_path = working_directory_path
            .join("ansible")
            .join("inventory")
            .join("dev_inventory_digital_ocean.yml");

        Ok(ClientsDeployer {
            ansible_provisioner,
            cloud_provider,
            deployment_type,
            environment_name: environment_name.to_string(),
            inventory_file_path,
            region,
            s3_repository,
            ssh_client,
            terraform_runner,
            working_directory_path,
        })
    }

    pub fn create_or_update_infra(&self, options: &ClientsInfraRunOptions) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;

        let args = options.build_terraform_args()?;

        println!("Running terraform apply...");
        self.terraform_runner
            .apply(args, Some(options.tfvars_filenames.clone()))?;
        print_duration(start.elapsed());
        Ok(())
    }

    pub async fn init(&self) -> Result<()> {
        self.terraform_runner.init()?;
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&self.environment_name) {
            self.terraform_runner
                .workspace_new(&self.environment_name)?;
        } else {
            println!("Workspace {} already exists", self.environment_name);
        }

        Ok(())
    }

    pub fn plan(&self, options: &ClientsInfraRunOptions) -> Result<()> {
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;

        let args = options.build_terraform_args()?;

        self.terraform_runner
            .plan(Some(args), Some(options.tfvars_filenames.clone()))?;
        Ok(())
    }

    pub async fn deploy(&self, options: ClientsDeployOptions) -> Result<()> {
        println!(
            "Deploying client for environment: {}",
            self.environment_name
        );

        let build_custom_binaries = options.binary_option.should_provision_build_machine();

        let start = Instant::now();
        println!("Initializing infrastructure...");

        let infra_options = ClientsInfraRunOptions {
            client_image_id: None,
            client_vm_count: options.client_vm_count,
            client_vm_size: options.client_vm_size.clone(),
            enable_build_vm: build_custom_binaries,
            name: options.name.clone(),
            tfvars_filenames: options.current_inventory.get_tfvars_filenames(),
        };

        self.create_or_update_infra(&infra_options)?;

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::Client,
                environment_type: options.environment_type.clone(),
                evm_details: EvmDetails {
                    network: options.evm_details.network.clone(),
                    data_payments_address: options.evm_details.data_payments_address.clone(),
                    merkle_payments_address: options.evm_details.merkle_payments_address.clone(),
                    payment_token_address: options.evm_details.payment_token_address.clone(),
                    rpc_url: options.evm_details.rpc_url.clone(),
                },
                funding_wallet_address: None,
                network_id: options.network_id,
                region: self.region.clone(),
                rewards_address: None,
            },
        )
        .await?;

        let provision_options = ProvisionOptions::from(options.clone());
        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");
            self.ansible_provisioner
                .build_autonomi_binaries(&provision_options, Some(vec!["ant".to_string()]))
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
        }

        if options.run_uploaders_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Uploaders");
            self.ansible_provisioner
                .provision_uploaders(
                    &provision_options,
                    options.peer.clone(),
                    options.network_contacts_url.clone(),
                )
                .await
                .map_err(|err| {
                    println!("Failed to provision Clients {err:?}");
                    err
                })?;
        }

        if options.run_downloaders_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Downloaders");
            self.ansible_provisioner
                .provision_downloaders(
                    &provision_options,
                    options.peer.clone(),
                    options.network_contacts_url.clone(),
                )
                .await
                .map_err(|err| {
                    println!("Failed to provision downloaders {err:?}");
                    err
                })?;
        }

        if options.run_chunk_trackers_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Chunk Trackers");
            self.ansible_provisioner
                .provision_chunk_trackers(
                    &provision_options,
                    options.peer.clone(),
                    options.network_contacts_url.clone(),
                )
                .await
                .map_err(|err| {
                    println!("Failed to provision chunk trackers {err:?}");
                    err
                })?;
        }

        if options.run_data_retrieval_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Data Retrieval Service");
            self.ansible_provisioner
                .provision_data_retrieval(&provision_options, options.network_contacts_url.clone())
                .await
                .map_err(|err| {
                    println!("Failed to provision data retrieval service {err:?}");
                    err
                })?;
        }

        if options.run_repair_files_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Repair Service");
            self.ansible_provisioner
                .provision_repair_files(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to provision repair files service {err:?}");
                    err
                })?;
        }

        if options.run_scan_repair_provision {
            self.ansible_provisioner
                .print_ansible_run_banner("Provision Scan Repair Service");
            self.ansible_provisioner
                .provision_scan_repair(&provision_options)
                .await
                .map_err(|err| {
                    println!("Failed to provision scan repair service {err:?}");
                    err
                })?;
        }

        println!("Deployment completed successfully in {:?}", start.elapsed());
        Ok(())
    }

    pub async fn deploy_static_downloaders(&self, options: ClientsDeployOptions) -> Result<()> {
        println!(
            "Deploying static downloaders for environment: {}",
            self.environment_name
        );

        let build_custom_binaries = options.binary_option.should_provision_build_machine();

        let start = Instant::now();
        println!("Initializing infrastructure...");

        let infra_options = ClientsInfraRunOptions {
            client_image_id: None,
            client_vm_count: options.client_vm_count,
            client_vm_size: options.client_vm_size.clone(),
            enable_build_vm: build_custom_binaries,
            name: options.name.clone(),
            tfvars_filenames: options.current_inventory.get_tfvars_filenames(),
        };

        self.create_or_update_infra(&infra_options)?;

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::Client,
                environment_type: options.environment_type.clone(),
                evm_details: EvmDetails {
                    network: options.evm_details.network.clone(),
                    data_payments_address: options.evm_details.data_payments_address.clone(),
                    merkle_payments_address: options.evm_details.merkle_payments_address.clone(),
                    payment_token_address: options.evm_details.payment_token_address.clone(),
                    rpc_url: options.evm_details.rpc_url.clone(),
                },
                funding_wallet_address: None,
                network_id: options.network_id,
                region: self.region.clone(),
                rewards_address: None,
            },
        )
        .await?;

        println!("Provisioning static downloaders with Ansible...");
        let provision_options = ProvisionOptions::from(options.clone());

        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");
            self.ansible_provisioner
                .build_autonomi_binaries(&provision_options, Some(vec!["ant".to_string()]))
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Static Downloaders");
        self.ansible_provisioner
            .provision_static_downloaders(
                &provision_options,
                options.peer.clone(),
                options.network_contacts_url.clone(),
            )
            .await
            .map_err(|err| {
                println!("Failed to provision static downloaders {err:?}");
                err
            })?;

        println!(
            "Static downloader deployment completed successfully in {:?}",
            start.elapsed()
        );
        Ok(())
    }

    pub async fn deploy_static_uploader(&self, options: ClientsDeployOptions) -> Result<()> {
        println!(
            "Deploying static uploader for environment: {}",
            self.environment_name
        );

        let build_custom_binaries = options.binary_option.should_provision_build_machine();

        let start = Instant::now();
        println!("Initializing infrastructure...");

        let infra_options = ClientsInfraRunOptions {
            client_image_id: None,
            client_vm_count: options.client_vm_count,
            client_vm_size: options.client_vm_size.clone(),
            enable_build_vm: build_custom_binaries,
            name: options.name.clone(),
            tfvars_filenames: options.current_inventory.get_tfvars_filenames(),
        };

        self.create_or_update_infra(&infra_options)?;

        write_environment_details(
            &self.s3_repository,
            &options.name,
            &EnvironmentDetails {
                deployment_type: DeploymentType::Client,
                environment_type: options.environment_type.clone(),
                evm_details: EvmDetails {
                    network: options.evm_details.network.clone(),
                    data_payments_address: options.evm_details.data_payments_address.clone(),
                    merkle_payments_address: options.evm_details.merkle_payments_address.clone(),
                    payment_token_address: options.evm_details.payment_token_address.clone(),
                    rpc_url: options.evm_details.rpc_url.clone(),
                },
                funding_wallet_address: None,
                network_id: options.network_id,
                region: self.region.clone(),
                rewards_address: None,
            },
        )
        .await?;

        println!("Provisioning static uploader with Ansible...");
        let provision_options = ProvisionOptions::from(options.clone());

        if build_custom_binaries {
            self.ansible_provisioner
                .print_ansible_run_banner("Build Custom Binaries");
            self.ansible_provisioner
                .build_autonomi_binaries(&provision_options, Some(vec!["ant".to_string()]))
                .map_err(|err| {
                    println!("Failed to build safe network binaries {err:?}");
                    err
                })?;
        }

        self.ansible_provisioner
            .print_ansible_run_banner("Provision Static Uploader");
        self.ansible_provisioner
            .provision_static_uploader(
                &provision_options,
                options.peer.clone(),
                options.network_contacts_url.clone(),
            )
            .await
            .map_err(|err| {
                println!("Failed to provision static uploader {err:?}");
                err
            })?;

        println!(
            "Static uploader deployment completed successfully in {:?}",
            start.elapsed()
        );
        Ok(())
    }

    async fn destroy_infra(&self, environment_details: &EnvironmentDetails) -> Result<()> {
        crate::infra::select_workspace(&self.terraform_runner, &self.environment_name)?;

        let options = ClientsInfraRunOptions::generate_existing(
            &self.environment_name,
            &self.terraform_runner,
            environment_details,
        )
        .await?;

        let mut args = Vec::new();
        if let Some(vm_count) = options.client_vm_count {
            args.push(("ant_client_vm_count".to_string(), vm_count.to_string()));
        }
        if let Some(vm_size) = &options.client_vm_size {
            args.push(("ant_client_droplet_size".to_string(), vm_size.clone()));
        }
        args.push((
            "use_custom_bin".to_string(),
            options.enable_build_vm.to_string(),
        ));

        self.terraform_runner
            .destroy(Some(args), Some(options.tfvars_filenames.clone()))?;

        crate::infra::delete_workspace(&self.terraform_runner, &self.environment_name)?;

        Ok(())
    }

    pub async fn clean(&self) -> Result<()> {
        let environment_details =
            get_environment_details(&self.environment_name, &self.s3_repository).await?;
        crate::funding::drain_funds(&self.ansible_provisioner, &environment_details).await?;

        self.destroy_infra(&environment_details).await?;

        cleanup_environment_inventory(
            &self.environment_name,
            &self
                .working_directory_path
                .join("ansible")
                .join("inventory"),
            None,
        )?;

        self.s3_repository
            .delete_object("sn-environment-type", &self.environment_name)
            .await?;
        Ok(())
    }
}
