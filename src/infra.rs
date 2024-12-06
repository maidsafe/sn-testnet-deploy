// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    print_duration,
    terraform::TerraformRunner,
    EnvironmentDetails, TestnetDeployer,
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct InfraRunOptions {
    pub bootstrap_node_vm_count: Option<u16>,
    pub bootstrap_node_vm_size: Option<String>,
    pub bootstrap_node_volume_size: Option<u16>,
    pub enable_build_vm: bool,
    pub evm_node_count: Option<u16>,
    pub evm_node_vm_size: Option<String>,
    pub genesis_vm_count: Option<u16>,
    pub genesis_node_volume_size: Option<u16>,
    pub name: String,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    pub private_node_vm_count: Option<u16>,
    pub private_node_volume_size: Option<u16>,
    pub tfvars_filename: String,
    pub uploader_vm_count: Option<u16>,
    pub uploader_vm_size: Option<String>,
}

impl InfraRunOptions {
    /// Generate the options for an existing deployment.
    /// This does not set the vm_size fields, as they are obtained from the tfvars file.
    pub async fn generate_existing(
        name: &str,
        terraform_runner: &TerraformRunner,
        environment_details: &EnvironmentDetails,
    ) -> Result<Self> {
        let resources = terraform_runner.show(name)?;
        let get_value_for_a_resource = |resource_name: &str,
                                        field_name: &str|
         -> Result<serde_json::Value, Error> {
            let vm_size = resources
                .iter()
                .filter(|r| r.resource_name == resource_name)
                .try_fold(None, |vm_size: Option<serde_json::Value>, r| {
                    let Some(size) = r.values.get(field_name) else {
                        log::error!("Failed to obtain '{field_name}' value for {resource_name}");
                        return Err(Error::TerraformResourceFieldMissing(field_name.to_string()));
                    };
                    match vm_size {
                        Some(ref existing_size) if existing_size != size => {
                            log::error!("Expected value: {existing_size}, got value: {size}");
                            Err(Error::TerraformResourceValueMismatch {
                                expected: existing_size.to_string(),
                                actual: size.to_string(),
                            })
                        }
                        _ => Ok(Some(size.clone())),
                    }
                })?;

            vm_size.ok_or(Error::TerraformResourceFieldMissing(field_name.to_string()))
        };

        let resource_count = |resource_name: &str| -> u16 {
            resources
                .iter()
                .filter(|r| r.resource_name == resource_name)
                .count() as u16
        };

        let bootstrap_node_vm_count = resource_count("bootstrap_node");
        let bootstrap_node_volume_size = if bootstrap_node_vm_count > 0 {
            let volume_size = get_value_for_a_resource("bootstrap_node_attached_volume", "size")?
                .as_u64()
                .ok_or_else(|| {
                    log::error!(
                        "Failed to obtain u64 'size' value for bootstrap_node_attached_volume"
                    );
                    Error::TerraformResourceFieldMissing("size".to_string())
                })?;
            Some(volume_size as u16)
        } else {
            None
        };

        let genesis_vm_count = resource_count("genesis_bootstrap");
        let genesis_node_volume_size = if genesis_vm_count > 0 {
            let genesis_node_volume_size =
                get_value_for_a_resource("genesis_node_attached_volume", "size")?
                    .as_u64()
                    .ok_or_else(|| {
                        log::error!(
                            "Failed to obtain u64 'size' value for genesis_node_attached_volume"
                        );
                        Error::TerraformResourceFieldMissing("size".to_string())
                    })?;
            Some(genesis_node_volume_size as u16)
        } else {
            None
        };

        let node_vm_count = resource_count("node");
        let node_volume_size = if node_vm_count > 0 {
            let node_volume_size = get_value_for_a_resource("node_attached_volume", "size")?
                .as_u64()
                .ok_or_else(|| {
                    log::error!("Failed to obtain u64 'size' value for node_attached_volume");
                    Error::TerraformResourceFieldMissing("size".to_string())
                })?;
            Some(node_volume_size as u16)
        } else {
            None
        };

        let private_node_vm_count = resource_count("private_node");
        let private_node_volume_size = if private_node_vm_count > 0 {
            let private_node_volume_size =
                get_value_for_a_resource("private_node_attached_volume", "size")?
                    .as_u64()
                    .ok_or_else(|| {
                        log::error!(
                            "Failed to obtain u64 'size' value for private_node_attached_volume"
                        );
                        Error::TerraformResourceFieldMissing("size".to_string())
                    })?;
            Some(private_node_volume_size as u16)
        } else {
            None
        };

        let uploader_vm_count = Some(resource_count("uploader"));
        let evm_node_count = Some(resource_count("evm_node"));
        let build_vm_count = resource_count("build");
        let enable_build_vm = build_vm_count > 0;

        let options = Self {
            bootstrap_node_vm_count: Some(bootstrap_node_vm_count),
            bootstrap_node_vm_size: None, // vm_size is obtained from the tfvars file
            bootstrap_node_volume_size,
            enable_build_vm,
            evm_node_count,
            evm_node_vm_size: None, // vm_size is obtained from the tfvars file
            genesis_vm_count: Some(genesis_vm_count),
            genesis_node_volume_size,
            name: name.to_string(),
            node_vm_count: Some(node_vm_count),
            node_vm_size: None, // vm_size is obtained from the tfvars file
            node_volume_size,
            private_node_vm_count: Some(private_node_vm_count),
            private_node_volume_size,
            tfvars_filename: environment_details
                .environment_type
                .get_tfvars_filename(name),
            uploader_vm_count,
            uploader_vm_size: None, // vm_size is obtained from the tfvars file
        };

        Ok(options)
    }
}

impl TestnetDeployer {
    /// Create or update the infrastructure for a deployment.
    pub fn create_or_update_infra(&self, options: &InfraRunOptions) -> Result<()> {
        let start = Instant::now();
        println!("Selecting {} workspace...", options.name);
        self.terraform_runner.workspace_select(&options.name)?;

        let mut args = Vec::new();

        if let Some(genesis_vm_count) = options.genesis_vm_count {
            args.push(("genesis_vm_count".to_string(), genesis_vm_count.to_string()));
        }

        if let Some(bootstrap_node_vm_count) = options.bootstrap_node_vm_count {
            args.push((
                "bootstrap_node_vm_count".to_string(),
                bootstrap_node_vm_count.to_string(),
            ));
        }
        if let Some(node_vm_count) = options.node_vm_count {
            args.push(("node_vm_count".to_string(), node_vm_count.to_string()));
        }
        if let Some(private_node_vm_count) = options.private_node_vm_count {
            args.push((
                "private_node_vm_count".to_string(),
                private_node_vm_count.to_string(),
            ));
            args.push((
                "setup_nat_gateway".to_string(),
                (private_node_vm_count > 0).to_string(),
            ));
        }

        if let Some(evm_node_count) = options.evm_node_count {
            args.push(("evm_node_vm_count".to_string(), evm_node_count.to_string()));
        }

        if let Some(uploader_vm_count) = options.uploader_vm_count {
            args.push((
                "uploader_vm_count".to_string(),
                uploader_vm_count.to_string(),
            ));
        }

        args.push((
            "use_custom_bin".to_string(),
            options.enable_build_vm.to_string(),
        ));

        if let Some(node_vm_size) = &options.node_vm_size {
            args.push(("node_droplet_size".to_string(), node_vm_size.clone()));
        }

        if let Some(bootstrap_vm_size) = &options.bootstrap_node_vm_size {
            args.push((
                "bootstrap_droplet_size".to_string(),
                bootstrap_vm_size.clone(),
            ));
        }

        if let Some(uploader_vm_size) = &options.uploader_vm_size {
            args.push((
                "uploader_droplet_size".to_string(),
                uploader_vm_size.clone(),
            ));
        }

        if let Some(evm_node_vm_size) = &options.evm_node_vm_size {
            args.push((
                "evm_node_droplet_size".to_string(),
                evm_node_vm_size.clone(),
            ));
        }

        if let Some(bootstrap_node_volume_size) = options.bootstrap_node_volume_size {
            args.push((
                "bootstrap_node_volume_size".to_string(),
                bootstrap_node_volume_size.to_string(),
            ));
        }
        if let Some(genesis_node_volume_size) = options.genesis_node_volume_size {
            args.push((
                "genesis_node_volume_size".to_string(),
                genesis_node_volume_size.to_string(),
            ));
        }
        if let Some(node_volume_size) = options.node_volume_size {
            args.push(("node_volume_size".to_string(), node_volume_size.to_string()));
        }
        if let Some(private_node_volume_size) = options.private_node_volume_size {
            args.push((
                "private_node_volume_size".to_string(),
                private_node_volume_size.to_string(),
            ));
        }

        println!("Running terraform apply...");
        self.terraform_runner
            .apply(args, Some(options.tfvars_filename.clone()))?;
        print_duration(start.elapsed());
        Ok(())
    }
}
