// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    print_duration,
    terraform::{TerraformResource, TerraformRunner},
    DeploymentType, EnvironmentDetails, TestnetDeployer,
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct InfraRunOptions {
    pub enable_build_vm: bool,
    pub evm_node_count: Option<u16>,
    pub evm_node_vm_size: Option<String>,
    pub full_cone_nat_gateway_vm_size: Option<String>,
    pub full_cone_private_node_vm_count: Option<u16>,
    pub full_cone_private_node_volume_size: Option<u16>,
    pub genesis_vm_count: Option<u16>,
    pub genesis_node_volume_size: Option<u16>,
    pub name: String,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    pub peer_cache_node_vm_count: Option<u16>,
    pub peer_cache_node_vm_size: Option<String>,
    pub peer_cache_node_volume_size: Option<u16>,
    pub symmetric_nat_gateway_vm_size: Option<String>,
    pub symmetric_private_node_vm_count: Option<u16>,
    pub symmetric_private_node_volume_size: Option<u16>,
    pub tfvars_filename: String,
    pub uploader_vm_count: Option<u16>,
    pub uploader_vm_size: Option<String>,
}

impl InfraRunOptions {
    fn get_value_for_resource(
        resources: &[TerraformResource],
        resource_name: &str,
        field_name: &str,
    ) -> Result<serde_json::Value, Error> {
        let field_value = resources
            .iter()
            .filter(|r| r.resource_name == resource_name)
            .try_fold(None, |acc_value: Option<serde_json::Value>, r| {
                let Some(value) = r.values.get(field_name) else {
                    log::error!("Failed to obtain '{field_name}' value for {resource_name}");
                    return Err(Error::TerraformResourceFieldMissing(field_name.to_string()));
                };
                match acc_value {
                    Some(ref existing_value) if existing_value != value => {
                        log::error!("Expected value: {existing_value}, got value: {value}");
                        Err(Error::TerraformResourceValueMismatch {
                            expected: existing_value.to_string(),
                            actual: value.to_string(),
                        })
                    }
                    _ => Ok(Some(value.clone())),
                }
            })?;

        field_value.ok_or(Error::TerraformResourceFieldMissing(field_name.to_string()))
    }

    /// Generate the options for an existing deployment.
    pub async fn generate_existing(
        name: &str,
        terraform_runner: &TerraformRunner,
        environment_details: &EnvironmentDetails,
    ) -> Result<Self> {
        let resources = terraform_runner.show(name)?;

        let resource_count = |resource_name: &str| -> u16 {
            resources
                .iter()
                .filter(|r| r.resource_name == resource_name)
                .count() as u16
        };

        let peer_cache_node_vm_count = resource_count("peer_cache_node");
        let peer_cache_node_volume_size = if peer_cache_node_vm_count > 0 {
            let volume_size = Self::get_value_for_resource(
                &resources,
                "peer_cache_node_attached_volume",
                "size",
            )?
            .as_u64()
            .ok_or_else(|| {
                log::error!(
                    "Failed to obtain u64 'size' value for peer_cache_node_attached_volume"
                );
                Error::TerraformResourceFieldMissing("size".to_string())
            })?;
            Some(volume_size as u16)
        } else {
            None
        };
        let peer_cache_node_vm_size =
            Self::get_value_for_resource(&resources, "peer_cache_node", "size")?;
        let peer_cache_node_vm_size = peer_cache_node_vm_size.as_str().ok_or_else(|| {
            log::error!("Failed to obtain str 'size' value for peer_cache_node");
            Error::TerraformResourceFieldMissing("size".to_string())
        })?;

        // There will always be a genesis node in a new deployment, but none in a bootstrap deployment.
        let genesis_vm_count = match environment_details.deployment_type {
            DeploymentType::New => 1,
            DeploymentType::Bootstrap => 0,
        };
        let genesis_node_volume_size = if genesis_vm_count > 0 {
            let genesis_node_volume_size =
                Self::get_value_for_resource(&resources, "genesis_node_attached_volume", "size")?
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
            let node_volume_size =
                Self::get_value_for_resource(&resources, "node_attached_volume", "size")?
                    .as_u64()
                    .ok_or_else(|| {
                        log::error!("Failed to obtain u64 'size' value for node_attached_volume");
                        Error::TerraformResourceFieldMissing("size".to_string())
                    })?;
            Some(node_volume_size as u16)
        } else {
            None
        };
        let node_vm_size = Self::get_value_for_resource(&resources, "node", "size")?;
        let node_vm_size = node_vm_size.as_str().ok_or_else(|| {
            log::error!("Failed to obtain str 'size' value for node");
            Error::TerraformResourceFieldMissing("size".to_string())
        })?;

        let symmetric_private_node_vm_count = resource_count("symmetric_private_node");
        let symmetric_private_node_volume_size = if symmetric_private_node_vm_count > 0 {
            let symmetric_private_node_volume_size = Self::get_value_for_resource(
                &resources,
                "symmetric_private_node_attached_volume",
                "size",
            )?
            .as_u64()
            .ok_or_else(|| {
                log::error!(
                    "Failed to obtain u64 'size' value for symmetric_private_node_attached_volume"
                );
                Error::TerraformResourceFieldMissing("size".to_string())
            })?;
            Some(symmetric_private_node_volume_size as u16)
        } else {
            None
        };
        let full_cone_private_node_vm_count = resource_count("full_cone_private_node");
        let full_cone_private_node_volume_size = if full_cone_private_node_vm_count > 0 {
            let full_cone_private_node_volume_size = Self::get_value_for_resource(
                &resources,
                "full_cone_private_node_attached_volume",
                "size",
            )?
            .as_u64()
            .ok_or_else(|| {
                log::error!(
                    "Failed to obtain u64 'size' value for full_cone_private_node_attached_volume"
                );
                Error::TerraformResourceFieldMissing("size".to_string())
            })?;
            Some(full_cone_private_node_volume_size as u16)
        } else {
            None
        };

        let uploader_vm_count = Some(resource_count("uploader"));
        let uploader_vm_size = Self::get_value_for_resource(&resources, "uploader", "size")?;
        let uploader_vm_size = uploader_vm_size.as_str().ok_or_else(|| {
            log::error!("Failed to obtain str 'size' value for uploader");
            Error::TerraformResourceFieldMissing("size".to_string())
        })?;

        let evm_node_count = Some(resource_count("evm_node"));
        let build_vm_count = resource_count("build");
        let enable_build_vm = build_vm_count > 0;

        let full_cone_nat_gateway_vm_size =
            Self::get_value_for_resource(&resources, "full_cone_nat_gateway", "size")?;
        let full_cone_nat_gateway_vm_size =
            full_cone_nat_gateway_vm_size.as_str().ok_or_else(|| {
                log::error!("Failed to obtain str 'size' value for full_cone_nat_gateway");
                Error::TerraformResourceFieldMissing("size".to_string())
            })?;

        let symmetric_nat_gateway_vm_size =
            Self::get_value_for_resource(&resources, "symmetric_nat_gateway", "size")?;
        let symmetric_nat_gateway_vm_size =
            symmetric_nat_gateway_vm_size.as_str().ok_or_else(|| {
                log::error!("Failed to obtain str 'size' value for symmetric_nat_gateway");
                Error::TerraformResourceFieldMissing("size".to_string())
            })?;

        let options = Self {
            enable_build_vm,
            evm_node_count,
            // The EVM node size never needs to change so it will be obtained from the tfvars file
            evm_node_vm_size: None,
            full_cone_nat_gateway_vm_size: Some(full_cone_nat_gateway_vm_size.to_string()),
            full_cone_private_node_vm_count: Some(full_cone_private_node_vm_count),
            full_cone_private_node_volume_size,
            genesis_vm_count: Some(genesis_vm_count),
            genesis_node_volume_size,
            name: name.to_string(),
            node_vm_count: Some(node_vm_count),
            node_vm_size: Some(node_vm_size.to_string()),
            node_volume_size,
            peer_cache_node_vm_count: Some(peer_cache_node_vm_count),
            peer_cache_node_vm_size: Some(peer_cache_node_vm_size.to_string()),
            peer_cache_node_volume_size,
            symmetric_nat_gateway_vm_size: Some(symmetric_nat_gateway_vm_size.to_string()),
            symmetric_private_node_vm_count: Some(symmetric_private_node_vm_count),
            symmetric_private_node_volume_size,
            tfvars_filename: environment_details
                .environment_type
                .get_tfvars_filename(name),
            uploader_vm_count,
            uploader_vm_size: Some(uploader_vm_size.to_string()),
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

        let args = build_terraform_args(options)?;

        println!("Running terraform apply...");
        self.terraform_runner
            .apply(args, Some(options.tfvars_filename.clone()))?;
        print_duration(start.elapsed());
        Ok(())
    }
}

/// Build the terraform arguments from InfraRunOptions
pub fn build_terraform_args(options: &InfraRunOptions) -> Result<Vec<(String, String)>> {
    let mut args = Vec::new();

    if let Some(reserved_ips) = crate::reserved_ip::get_reserved_ips_args(&options.name) {
        args.push(("peer_cache_reserved_ips".to_string(), reserved_ips));
    }

    if let Some(genesis_vm_count) = options.genesis_vm_count {
        args.push(("genesis_vm_count".to_string(), genesis_vm_count.to_string()));
    }

    if let Some(peer_cache_node_vm_count) = options.peer_cache_node_vm_count {
        args.push((
            "peer_cache_node_vm_count".to_string(),
            peer_cache_node_vm_count.to_string(),
        ));
    }
    if let Some(node_vm_count) = options.node_vm_count {
        args.push(("node_vm_count".to_string(), node_vm_count.to_string()));
    }

    if let Some(symmetric_private_node_vm_count) = options.symmetric_private_node_vm_count {
        args.push((
            "symmetric_private_node_vm_count".to_string(),
            symmetric_private_node_vm_count.to_string(),
        ));
    }
    if let Some(full_cone_private_node_vm_count) = options.full_cone_private_node_vm_count {
        args.push((
            "full_cone_private_node_vm_count".to_string(),
            full_cone_private_node_vm_count.to_string(),
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

    if let Some(peer_cache_vm_size) = &options.peer_cache_node_vm_size {
        args.push((
            "peer_cache_droplet_size".to_string(),
            peer_cache_vm_size.clone(),
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

    if let Some(peer_cache_node_volume_size) = options.peer_cache_node_volume_size {
        args.push((
            "peer_cache_node_volume_size".to_string(),
            peer_cache_node_volume_size.to_string(),
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

    if let Some(full_cone_gateway_vm_size) = &options.full_cone_nat_gateway_vm_size {
        args.push((
            "full_cone_nat_gateway_droplet_size".to_string(),
            full_cone_gateway_vm_size.clone(),
        ));
    }
    if let Some(full_cone_private_node_volume_size) = options.full_cone_private_node_volume_size {
        args.push((
            "full_cone_private_node_volume_size".to_string(),
            full_cone_private_node_volume_size.to_string(),
        ));
    }

    if let Some(nat_gateway_vm_size) = &options.symmetric_nat_gateway_vm_size {
        args.push((
            "symmetric_nat_gateway_droplet_size".to_string(),
            nat_gateway_vm_size.clone(),
        ));
    }
    if let Some(symmetric_private_node_volume_size) = options.symmetric_private_node_volume_size {
        args.push((
            "symmetric_private_node_volume_size".to_string(),
            symmetric_private_node_volume_size.to_string(),
        ));
    }

    Ok(args)
}
