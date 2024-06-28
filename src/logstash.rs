// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    ansible::{AnsibleInventoryType, AnsiblePlaybook, AnsibleRunner},
    digital_ocean::{DigitalOceanClient, DIGITAL_OCEAN_API_BASE_URL, DIGITAL_OCEAN_API_PAGE_SIZE},
    do_clean,
    error::{Error, Result},
    ssh::SshClient,
    terraform::TerraformRunner,
    CloudProvider, ANSIBLE_DEFAULT_FORKS,
};
use log::debug;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

pub const LOGSTASH_PORT: u16 = 5044;

#[derive(Default)]
pub struct LogstashDeployBuilder {
    environment_name: String,
    provider: Option<CloudProvider>,
    ssh_secret_key_path: Option<PathBuf>,
    state_bucket_name: Option<String>,
    terraform_binary_path: Option<PathBuf>,
    vault_password_path: Option<PathBuf>,
    working_directory_path: Option<PathBuf>,
}

impl LogstashDeployBuilder {
    pub fn new() -> Self {
        Default::default()
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

    pub fn build(&self) -> Result<LogstashDeploy> {
        let provider = self
            .provider
            .as_ref()
            .unwrap_or(&CloudProvider::DigitalOcean);
        let access_token = match provider {
            CloudProvider::DigitalOcean => {
                let digital_ocean_pat = std::env::var("DO_PAT").map_err(|_| {
                    Error::CloudProviderCredentialsNotSupplied("DO_PAT".to_string())
                })?;
                // The DO_PAT variable is not actually read by either Terraform or Ansible.
                // Each tool uses a different variable, so instead we set each of those variables
                // to the value of DO_PAT. This means the user only needs to set one variable.
                std::env::set_var("DIGITALOCEAN_TOKEN", digital_ocean_pat.clone());
                std::env::set_var("DO_API_TOKEN", digital_ocean_pat.clone());

                digital_ocean_pat
            }
            _ => {
                return Err(Error::CloudProviderNotSupported(provider.to_string()));
            }
        };

        let digital_ocean_client = DigitalOceanClient {
            base_url: DIGITAL_OCEAN_API_BASE_URL.to_string(),
            access_token,
            page_size: DIGITAL_OCEAN_API_PAGE_SIZE,
        };

        let state_bucket_name = match self.state_bucket_name {
            Some(ref bucket_name) => bucket_name.clone(),
            None => std::env::var("TERRAFORM_STATE_BUCKET_NAME")?,
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

        let terraform_runner = TerraformRunner::new(
            terraform_binary_path.to_path_buf(),
            working_directory_path
                .join("terraform")
                .join("logstash")
                .join(provider.to_string()),
            provider.clone(),
            &state_bucket_name,
        )?;
        let ansible_runner = AnsibleRunner::new(
            ANSIBLE_DEFAULT_FORKS,
            false,
            &self.environment_name,
            provider.clone(),
            ssh_secret_key_path.clone(),
            vault_password_path,
            working_directory_path.join("ansible"),
        )?;

        let logstash = LogstashDeploy::new(
            terraform_runner,
            ansible_runner,
            SshClient::new(ssh_secret_key_path),
            digital_ocean_client,
            working_directory_path,
            provider.clone(),
        );

        Ok(logstash)
    }
}

pub struct LogstashDeploy {
    pub terraform_runner: TerraformRunner,
    pub ansible_runner: AnsibleRunner,
    pub ssh_client: SshClient,
    pub digital_ocean_client: DigitalOceanClient,
    pub working_directory_path: PathBuf,
    pub cloud_provider: CloudProvider,
    pub inventory_file_path: PathBuf,
}

impl LogstashDeploy {
    pub fn new(
        terraform_runner: TerraformRunner,
        ansible_runner: AnsibleRunner,
        ssh_client: SshClient,
        digital_ocean_client: DigitalOceanClient,
        working_directory_path: PathBuf,
        cloud_provider: CloudProvider,
    ) -> LogstashDeploy {
        let inventory_file_path = working_directory_path
            .join("ansible")
            .join("inventory")
            .join("dev_inventory_digital_ocean.yml");
        LogstashDeploy {
            terraform_runner,
            ansible_runner,
            ssh_client,
            digital_ocean_client,
            working_directory_path,
            cloud_provider,
            inventory_file_path,
        }
    }

    pub async fn init(&self, name: &str) -> Result<()> {
        self.terraform_runner.init()?;
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&name.to_string()) {
            self.terraform_runner.workspace_new(name)?;
        } else {
            println!("Workspace {name} already exists")
        }

        let src_path = self.inventory_file_path.clone();
        let dest_path = self
            .working_directory_path
            .join("ansible")
            .join("inventory")
            .join(format!(".{}_logstash_inventory_digital_ocean.yml", name));
        if !dest_path.is_file() {
            let mut contents = std::fs::read_to_string(src_path)?;
            contents = contents.replace("env_value", name);
            contents = contents.replace("type_value", "logstash");
            std::fs::write(&dest_path, contents)?;
            debug!("Created inventory file at {dest_path:#?}");
        }

        Ok(())
    }

    pub async fn create_infra(&self, name: &str, vm_count: u16) -> Result<()> {
        println!("Selecting {name} workspace...");
        self.terraform_runner.workspace_select(name)?;
        println!("Running terraform apply...");
        self.terraform_runner
            .apply(vec![("node_count".to_string(), vm_count.to_string())])?;
        Ok(())
    }

    pub async fn provision(&self, name: &str) -> Result<()> {
        println!("Obtaining IP address for Logstash VM...");
        let logstash_inventory = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Logstash, false)
            .await?;
        let logstash_ip = logstash_inventory[0].1;
        self.ssh_client
            .wait_for_ssh_availability(&logstash_ip, &self.cloud_provider.get_ssh_user())?;
        self.ansible_runner.run_playbook(
            AnsiblePlaybook::Logstash,
            AnsibleInventoryType::Logstash,
            Some(format!(
                "{{ \"provider\": \"{}\", \"stack_name\": \"{name}\", \"logstash_host_ip_address\": \"{logstash_ip}\" }}",
                self.cloud_provider
            )),
        )?;
        Ok(())
    }

    pub async fn deploy(&self, name: &str, vm_count: u16) -> Result<()> {
        self.create_infra(name, vm_count).await?;
        self.provision(name).await?;
        Ok(())
    }

    pub async fn clean(&self, name: &str) -> Result<()> {
        do_clean(
            name,
            self.working_directory_path.clone(),
            &self.terraform_runner,
            vec!["logstash".to_string()],
        )
    }

    pub async fn get_stack_hosts(&self, name: &str) -> Result<Vec<SocketAddr>> {
        let droplets = self.digital_ocean_client.list_droplets(true).await?;
        let stack_hosts: Vec<SocketAddr> = droplets
            .iter()
            .filter(|x| x.name.starts_with(&format!("logstash-{}", name)))
            .map(|x| SocketAddr::new(IpAddr::V4(x.ip_address), LOGSTASH_PORT))
            .collect();
        println!("Obtained Logstash hosts:");
        for host in stack_hosts.iter() {
            println!("{host}");
        }
        Ok(stack_hosts)
    }
}
