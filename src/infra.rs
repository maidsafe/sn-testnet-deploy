// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    print_duration,
    terraform::{TerraformResource, TerraformRunner},
    EnvironmentDetails, TestnetDeployer,
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
    pub tfvars_filename: Option<String>,
    pub uploader_vm_count: Option<u16>,
    pub uploader_vm_size: Option<String>,
}

impl InfraRunOptions {
    /// Generate the options for an existing deployment.
    pub async fn generate_existing(
        name: &str,
        terraform_runner: &TerraformRunner,
        environment_details: Option<&EnvironmentDetails>,
    ) -> Result<Self> {
        let resources = terraform_runner.show(name)?;

        let resource_count = |resource_name: &str| -> u16 {
            resources
                .iter()
                .filter(|r| r.resource_name == resource_name)
                .count() as u16
        };

        let peer_cache_node_vm_count = resource_count("peer_cache_node");
        println!("Peer cache node count: {}", peer_cache_node_vm_count);
        let (peer_cache_node_volume_size, peer_cache_node_vm_size) = if peer_cache_node_vm_count > 0
        {
            let volume_size =
                get_value_for_resource(&resources, "peer_cache_node_attached_volume", "size")?
                    .and_then(|size| size.as_u64())
                    .map(|size| size as u16);
            let vm_size = get_value_for_resource(&resources, "peer_cache_node", "size")?
                .map(|size| size.to_string());

            (volume_size, vm_size)
        } else {
            (None, None)
        };

        let genesis_node_vm_count = resource_count("genesis_bootstrap");
        let genesis_node_volume_size = if genesis_node_vm_count > 0 {
            get_value_for_resource(&resources, "genesis_node_attached_volume", "size")?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16)
        } else {
            None
        };

        let node_vm_count = resource_count("node");
        let node_volume_size = if node_vm_count > 0 {
            get_value_for_resource(&resources, "node_attached_volume", "size")?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16)
        } else {
            None
        };

        let symmetric_private_node_vm_count = resource_count("symmetric_private_node");
        let (symmetric_private_node_volume_size, symmetric_nat_gateway_vm_size) =
            if symmetric_private_node_vm_count > 0 {
                let symmetric_private_node_volume_size = get_value_for_resource(
                    &resources,
                    "symmetric_private_node_attached_volume",
                    "size",
                )?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16);
                // gateways should exists if private nodes exist
                let symmetric_nat_gateway_vm_size =
                    get_value_for_resource(&resources, "symmetric_nat_gateway", "size")?
                        .map(|size| size.to_string());

                (
                    symmetric_private_node_volume_size,
                    symmetric_nat_gateway_vm_size,
                )
            } else {
                (None, None)
            };
        let full_cone_private_node_vm_count = resource_count("full_cone_private_node");
        let (full_cone_private_node_volume_size, full_cone_nat_gateway_vm_size) =
            if full_cone_private_node_vm_count > 0 {
                let full_cone_private_node_volume_size = get_value_for_resource(
                    &resources,
                    "full_cone_private_node_attached_volume",
                    "size",
                )?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16);
                // gateways should exists if private nodes exist
                let full_cone_nat_gateway_vm_size =
                    get_value_for_resource(&resources, "full_cone_nat_gateway", "size")?
                        .map(|size| size.to_string());

                (
                    full_cone_private_node_volume_size,
                    full_cone_nat_gateway_vm_size,
                )
            } else {
                (None, None)
            };

        let uploader_vm_count = resource_count("uploader");
        let uploader_vm_size = if uploader_vm_count > 0 {
            get_value_for_resource(&resources, "uploader", "size")?.map(|size| size.to_string())
        } else {
            None
        };

        let evm_node_count = resource_count("evm_node");
        let build_vm_count = resource_count("build");
        let enable_build_vm = build_vm_count > 0;

        // Node VM size var is re-used for nodes, evm nodes, symmetric and full cone private nodes
        let node_vm_size = if node_vm_count > 0 {
            get_value_for_resource(&resources, "node", "size")?.map(|size| size.to_string())
        } else if symmetric_private_node_vm_count > 0 {
            get_value_for_resource(&resources, "symmetric_private_node", "size")?
                .map(|size| size.to_string())
        } else if full_cone_private_node_vm_count > 0 {
            get_value_for_resource(&resources, "full_cone_private_node", "size")?
                .map(|size| size.to_string())
        } else if evm_node_count > 0 {
            get_value_for_resource(&resources, "evm_node", "size")?.map(|size| size.to_string())
        } else {
            None
        };

        let options = Self {
            enable_build_vm,
            evm_node_count: Some(evm_node_count),
            // The EVM node size never needs to change so it will be obtained from the tfvars file
            evm_node_vm_size: None,
            full_cone_nat_gateway_vm_size,
            full_cone_private_node_vm_count: Some(full_cone_private_node_vm_count),
            full_cone_private_node_volume_size,
            genesis_vm_count: Some(genesis_node_vm_count),
            genesis_node_volume_size,
            name: name.to_string(),
            node_vm_count: Some(node_vm_count),
            node_vm_size,
            node_volume_size,
            peer_cache_node_vm_count: Some(peer_cache_node_vm_count),
            peer_cache_node_vm_size,
            peer_cache_node_volume_size,
            symmetric_nat_gateway_vm_size,
            symmetric_private_node_vm_count: Some(symmetric_private_node_vm_count),
            symmetric_private_node_volume_size,
            tfvars_filename: environment_details
                .map(|details| details.environment_type.get_tfvars_filename(name)),
            uploader_vm_count: Some(uploader_vm_count),
            uploader_vm_size,
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
            .apply(args, options.tfvars_filename.clone())?;
        print_duration(start.elapsed());
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct UploaderInfraRunOptions {
    pub enable_build_vm: bool,
    pub name: String,
    pub tfvars_filename: String,
    pub uploader_vm_count: Option<u16>,
    pub uploader_vm_size: Option<String>,
}

impl UploaderInfraRunOptions {
    /// Generate the options for an existing uploader deployment.
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

        let uploader_vm_count = resource_count("uploader");
        let uploader_vm_size = if uploader_vm_count > 0 {
            get_value_for_resource(&resources, "uploader", "size")?.map(|size| size.to_string())
        } else {
            None
        };

        let build_vm_count = resource_count("build");
        let enable_build_vm = build_vm_count > 0;

        let options = Self {
            enable_build_vm,
            name: name.to_string(),
            tfvars_filename: environment_details
                .environment_type
                .get_tfvars_filename(name),
            uploader_vm_count: Some(uploader_vm_count),
            uploader_vm_size,
        };

        Ok(options)
    }

    pub fn build_terraform_args(&self) -> Result<Vec<(String, String)>> {
        let mut args = Vec::new();

        args.push((
            "use_custom_bin".to_string(),
            self.enable_build_vm.to_string(),
        ));

        if let Some(uploader_vm_count) = self.uploader_vm_count {
            args.push((
                "uploader_vm_count".to_string(),
                uploader_vm_count.to_string(),
            ));
        }
        if let Some(uploader_vm_size) = &self.uploader_vm_size {
            args.push((
                "uploader_droplet_size".to_string(),
                uploader_vm_size.clone(),
            ));
        }

        Ok(args)
    }
}

/// Extract a specific field value from terraform resources.
fn get_value_for_resource(
    resources: &[TerraformResource],
    resource_name: &str,
    field_name: &str,
) -> Result<Option<serde_json::Value>, Error> {
    let field_value = resources
        .iter()
        .filter(|r| r.resource_name == resource_name)
        .try_fold(None, |acc_value: Option<serde_json::Value>, r| {
            if let Some(value) = r.values.get(field_name) {
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
            } else {
                Ok(acc_value)
            }
        })?;

    Ok(field_value)
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

/// Select a Terraform workspace for an environment.
/// Returns an error if the environment doesn't exist.
pub fn select_workspace(terraform_runner: &TerraformRunner, name: &str) -> Result<()> {
    terraform_runner.init()?;
    let workspaces = terraform_runner.workspace_list()?;
    if !workspaces.contains(&name.to_string()) {
        return Err(Error::EnvironmentDoesNotExist(name.to_string()));
    }
    terraform_runner.workspace_select(name)?;
    println!("Selected {name} workspace");
    Ok(())
}

pub fn delete_workspace(terraform_runner: &TerraformRunner, name: &str) -> Result<()> {
    // The 'dev' workspace is one we always expect to exist, for admin purposes.
    // You can't delete a workspace while it is selected, so we select 'dev' before we delete
    // the current workspace.
    terraform_runner.workspace_select("dev")?;
    terraform_runner.workspace_delete(name)?;
    println!("Deleted {name} workspace");
    Ok(())
}
