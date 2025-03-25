// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use log::debug;

use crate::{
    error::{Error, Result},
    print_duration,
    terraform::{TerraformResource, TerraformRunner},
    EnvironmentDetails, TestnetDeployer,
};
use std::time::Instant;

const BUILD_VM: &str = "build";
const EVM_NODE: &str = "evm_node";
const FULL_CONE_NAT_GATEWAY: &str = "full_cone_nat_gateway";
const FULL_CONE_PRIVATE_NODE: &str = "full_cone_private_node";
const FULL_CONE_PRIVATE_NODE_ATTACHED_VOLUME: &str = "full_cone_private_node_attached_volume";
const GENESIS_NODE: &str = "genesis_bootstrap";
const GENESIS_NODE_ATTACHED_VOLUME: &str = "genesis_node_attached_volume";
const NODE: &str = "node";
const NODE_ATTACHED_VOLUME: &str = "node_attached_volume";
const PEER_CACHE_NODE: &str = "peer_cache_node";
const PEER_CACHE_NODE_ATTACHED_VOLUME: &str = "peer_cache_node_attached_volume";
const SYMMETRIC_NAT_GATEWAY: &str = "symmetric_nat_gateway";
const SYMMETRIC_PRIVATE_NODE: &str = "symmetric_private_node";
const SYMMETRIC_PRIVATE_NODE_ATTACHED_VOLUME: &str = "symmetric_private_node_attached_volume";
const UPLOADER: &str = "uploader";

const SIZE: &str = "size";
const IMAGE: &str = "image";

#[derive(Clone, Debug)]
pub struct InfraRunOptions {
    pub enable_build_vm: bool,
    pub evm_node_count: Option<u16>,
    pub evm_node_vm_size: Option<String>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub evm_node_image_id: Option<String>,
    pub full_cone_nat_gateway_vm_size: Option<String>,
    pub full_cone_private_node_vm_count: Option<u16>,
    pub full_cone_private_node_volume_size: Option<u16>,
    pub genesis_vm_count: Option<u16>,
    pub genesis_node_volume_size: Option<u16>,
    pub name: String,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub nat_gateway_image_id: Option<String>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub node_image_id: Option<String>,
    pub node_vm_count: Option<u16>,
    pub node_vm_size: Option<String>,
    pub node_volume_size: Option<u16>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub peer_cache_image_id: Option<String>,
    pub peer_cache_node_vm_count: Option<u16>,
    pub peer_cache_node_vm_size: Option<String>,
    pub peer_cache_node_volume_size: Option<u16>,
    pub symmetric_nat_gateway_vm_size: Option<String>,
    pub symmetric_private_node_vm_count: Option<u16>,
    pub symmetric_private_node_volume_size: Option<u16>,
    pub tfvars_filename: Option<String>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub uploader_image_id: Option<String>,
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

        let peer_cache_node_vm_count = resource_count(PEER_CACHE_NODE);
        debug!("Peer cache node count: {peer_cache_node_vm_count}");
        let (peer_cache_node_volume_size, peer_cache_node_vm_size, peer_cache_image_id) =
            if peer_cache_node_vm_count > 0 {
                let volume_size =
                    get_value_for_resource(&resources, PEER_CACHE_NODE_ATTACHED_VOLUME, SIZE)?
                        .and_then(|size| size.as_u64())
                        .map(|size| size as u16);
                debug!("Peer cache node volume size: {volume_size:?}");
                let vm_size = get_value_for_resource(&resources, PEER_CACHE_NODE, SIZE)?
                    .map(|size| size.to_string());
                debug!("Peer cache node size: {vm_size:?}");
                let image_id = get_value_for_resource(&resources, PEER_CACHE_NODE, IMAGE)?
                    .map(|image_id| image_id.to_string());
                debug!("Peer cache node image id: {image_id:?}");

                (volume_size, vm_size, image_id)
            } else {
                (None, None, None)
            };

        let genesis_node_vm_count = resource_count(GENESIS_NODE);
        debug!("Genesis node count: {genesis_node_vm_count}");
        let genesis_node_volume_size = if genesis_node_vm_count > 0 {
            get_value_for_resource(&resources, GENESIS_NODE_ATTACHED_VOLUME, SIZE)?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16)
        } else {
            None
        };
        debug!("Genesis node volume size: {genesis_node_volume_size:?}");

        let node_vm_count = resource_count(NODE);
        debug!("Node count: {node_vm_count}");
        let node_volume_size = if node_vm_count > 0 {
            get_value_for_resource(&resources, NODE_ATTACHED_VOLUME, SIZE)?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16)
        } else {
            None
        };
        debug!("Node volume size: {node_volume_size:?}");

        let mut nat_gateway_image_id = None;
        let symmetric_private_node_vm_count = resource_count(SYMMETRIC_PRIVATE_NODE);
        debug!("Symmetric private node count: {symmetric_private_node_vm_count}");
        let (symmetric_private_node_volume_size, symmetric_nat_gateway_vm_size) =
            if symmetric_private_node_vm_count > 0 {
                let symmetric_private_node_volume_size = get_value_for_resource(
                    &resources,
                    SYMMETRIC_PRIVATE_NODE_ATTACHED_VOLUME,
                    SIZE,
                )?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16);
                debug!(
                    "Symmetric private node volume size: {symmetric_private_node_volume_size:?}"
                );
                // gateways should exists if private nodes exist
                let symmetric_nat_gateway_vm_size =
                    get_value_for_resource(&resources, SYMMETRIC_NAT_GATEWAY, SIZE)?
                        .map(|size| size.to_string());

                debug!("Symmetric nat gateway size: {symmetric_nat_gateway_vm_size:?}");

                if let Some(nat_gateway_image) =
                    get_value_for_resource(&resources, SYMMETRIC_NAT_GATEWAY, IMAGE)?
                {
                    debug!("Nat gateway image: {nat_gateway_image}");
                    nat_gateway_image_id = Some(nat_gateway_image.to_string());
                }

                (
                    symmetric_private_node_volume_size,
                    symmetric_nat_gateway_vm_size,
                )
            } else {
                (None, None)
            };

        let full_cone_private_node_vm_count = resource_count(FULL_CONE_PRIVATE_NODE);
        debug!("Full cone private node count: {full_cone_private_node_vm_count}");
        let (full_cone_private_node_volume_size, full_cone_nat_gateway_vm_size) =
            if full_cone_private_node_vm_count > 0 {
                let full_cone_private_node_volume_size = get_value_for_resource(
                    &resources,
                    FULL_CONE_PRIVATE_NODE_ATTACHED_VOLUME,
                    SIZE,
                )?
                .and_then(|size| size.as_u64())
                .map(|size| size as u16);
                debug!(
                    "Full cone private node volume size: {full_cone_private_node_volume_size:?}"
                );
                // gateways should exists if private nodes exist
                let full_cone_nat_gateway_vm_size =
                    get_value_for_resource(&resources, FULL_CONE_NAT_GATEWAY, SIZE)?
                        .map(|size| size.to_string());
                debug!("Full cone nat gateway size: {full_cone_nat_gateway_vm_size:?}");

                if let Some(nat_gateway_image) =
                    get_value_for_resource(&resources, FULL_CONE_NAT_GATEWAY, IMAGE)?
                {
                    debug!("Nat gateway image: {nat_gateway_image}");
                    nat_gateway_image_id = Some(nat_gateway_image.to_string());
                }

                (
                    full_cone_private_node_volume_size,
                    full_cone_nat_gateway_vm_size,
                )
            } else {
                (None, None)
            };

        let uploader_vm_count = resource_count(UPLOADER);
        debug!("Uploader count: {uploader_vm_count}");
        let (uploader_vm_size, uploader_image_id) = if uploader_vm_count > 0 {
            let vm_size =
                get_value_for_resource(&resources, UPLOADER, SIZE)?.map(|size| size.to_string());
            debug!("Uploader size: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, UPLOADER, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("Uploader image id: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let build_vm_count = resource_count(BUILD_VM);
        debug!("Build VM count: {build_vm_count}");
        let enable_build_vm = build_vm_count > 0;

        // Node VM size var is re-used for nodes, evm nodes, symmetric and full cone private nodes
        let (node_vm_size, node_image_id) = if node_vm_count > 0 {
            let vm_size =
                get_value_for_resource(&resources, NODE, SIZE)?.map(|size| size.to_string());
            debug!("Node size obtained from {NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, NODE, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("Node image id obtained from {NODE}: {image_id:?}");
            (vm_size, image_id)
        } else if symmetric_private_node_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, SYMMETRIC_PRIVATE_NODE, SIZE)?
                .map(|size| size.to_string());
            debug!("Node size obtained from {SYMMETRIC_PRIVATE_NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, SYMMETRIC_PRIVATE_NODE, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("Node image id obtained from {SYMMETRIC_PRIVATE_NODE}: {image_id:?}");
            (vm_size, image_id)
        } else if full_cone_private_node_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, FULL_CONE_PRIVATE_NODE, SIZE)?
                .map(|size| size.to_string());
            debug!("Node size obtained from {FULL_CONE_PRIVATE_NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, FULL_CONE_PRIVATE_NODE, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("Node image id obtained from {FULL_CONE_PRIVATE_NODE}: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let evm_node_count = resource_count(EVM_NODE);
        debug!("EVM node count: {evm_node_count}");
        let (evm_node_vm_size, evm_node_image_id) = if evm_node_count > 0 {
            let emv_node_vm_size =
                get_value_for_resource(&resources, EVM_NODE, SIZE)?.map(|size| size.to_string());
            debug!("EVM node size: {emv_node_vm_size:?}");
            let evm_node_image_id = get_value_for_resource(&resources, EVM_NODE, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("EVM node image id: {evm_node_image_id:?}");
            (emv_node_vm_size, evm_node_image_id)
        } else {
            (None, None)
        };

        let options = Self {
            enable_build_vm,
            evm_node_count: Some(evm_node_count),
            evm_node_vm_size,
            evm_node_image_id,
            full_cone_nat_gateway_vm_size,
            full_cone_private_node_vm_count: Some(full_cone_private_node_vm_count),
            full_cone_private_node_volume_size,
            genesis_vm_count: Some(genesis_node_vm_count),
            genesis_node_volume_size,
            name: name.to_string(),
            nat_gateway_image_id,
            node_image_id,
            node_vm_count: Some(node_vm_count),
            node_vm_size,
            node_volume_size,
            peer_cache_image_id,
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
            uploader_image_id,
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
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub uploader_image_id: Option<String>,
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

        let uploader_vm_count = resource_count(UPLOADER);
        debug!("Uploader count: {uploader_vm_count}");
        let (uploader_vm_size, uploader_image_id) = if uploader_vm_count > 0 {
            let vm_size =
                get_value_for_resource(&resources, UPLOADER, SIZE)?.map(|size| size.to_string());
            debug!("Uploader size: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, UPLOADER, IMAGE)?
                .map(|image_id| image_id.to_string());
            debug!("Uploader image id: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let build_vm_count = resource_count(BUILD_VM);
        debug!("Build VM count: {build_vm_count}");
        let enable_build_vm = build_vm_count > 0;

        let options = Self {
            enable_build_vm,
            name: name.to_string(),
            tfvars_filename: environment_details
                .environment_type
                .get_tfvars_filename(name),
            uploader_vm_count: Some(uploader_vm_count),
            uploader_vm_size,
            uploader_image_id,
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
        if let Some(uploader_image_id) = &self.uploader_image_id {
            args.push((
                "uploader_droplet_image_id".to_string(),
                uploader_image_id.clone(),
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

    args.push((
        "use_custom_bin".to_string(),
        options.enable_build_vm.to_string(),
    ));

    if let Some(evm_node_count) = options.evm_node_count {
        args.push(("evm_node_vm_count".to_string(), evm_node_count.to_string()));
    }

    if let Some(evm_node_vm_size) = &options.evm_node_vm_size {
        args.push((
            "evm_node_droplet_size".to_string(),
            evm_node_vm_size.clone(),
        ));
    }

    if let Some(emv_node_image_id) = &options.evm_node_image_id {
        args.push((
            "evm_node_droplet_image_id".to_string(),
            emv_node_image_id.clone(),
        ));
    }

    if let Some(full_cone_gateway_vm_size) = &options.full_cone_nat_gateway_vm_size {
        args.push((
            "full_cone_nat_gateway_droplet_size".to_string(),
            full_cone_gateway_vm_size.clone(),
        ));
    }

    if let Some(full_cone_private_node_vm_count) = options.full_cone_private_node_vm_count {
        args.push((
            "full_cone_private_node_vm_count".to_string(),
            full_cone_private_node_vm_count.to_string(),
        ));
    }

    if let Some(full_cone_private_node_volume_size) = options.full_cone_private_node_volume_size {
        args.push((
            "full_cone_private_node_volume_size".to_string(),
            full_cone_private_node_volume_size.to_string(),
        ));
    }

    if let Some(genesis_vm_count) = options.genesis_vm_count {
        args.push(("genesis_vm_count".to_string(), genesis_vm_count.to_string()));
    }

    if let Some(genesis_node_volume_size) = options.genesis_node_volume_size {
        args.push((
            "genesis_node_volume_size".to_string(),
            genesis_node_volume_size.to_string(),
        ));
    }

    if let Some(nat_gateway_image_id) = &options.nat_gateway_image_id {
        args.push((
            "nat_gateway_droplet_image_id".to_string(),
            nat_gateway_image_id.clone(),
        ));
    }

    if let Some(node_image_id) = &options.node_image_id {
        args.push(("node_droplet_image_id".to_string(), node_image_id.clone()));
    }

    if let Some(node_vm_count) = options.node_vm_count {
        args.push(("node_vm_count".to_string(), node_vm_count.to_string()));
    }

    if let Some(node_vm_size) = &options.node_vm_size {
        args.push(("node_droplet_size".to_string(), node_vm_size.clone()));
    }

    if let Some(node_volume_size) = options.node_volume_size {
        args.push(("node_volume_size".to_string(), node_volume_size.to_string()));
    }

    if let Some(peer_cache_image_id) = &options.peer_cache_image_id {
        args.push((
            "peer_cache_droplet_image_id".to_string(),
            peer_cache_image_id.clone(),
        ));
    }

    if let Some(peer_cache_node_vm_count) = options.peer_cache_node_vm_count {
        args.push((
            "peer_cache_node_vm_count".to_string(),
            peer_cache_node_vm_count.to_string(),
        ));
    }

    if let Some(peer_cache_vm_size) = &options.peer_cache_node_vm_size {
        args.push((
            "peer_cache_droplet_size".to_string(),
            peer_cache_vm_size.clone(),
        ));
    }

    if let Some(reserved_ips) = crate::reserved_ip::get_reserved_ips_args(&options.name) {
        args.push(("peer_cache_reserved_ips".to_string(), reserved_ips));
    }

    if let Some(peer_cache_node_volume_size) = options.peer_cache_node_volume_size {
        args.push((
            "peer_cache_node_volume_size".to_string(),
            peer_cache_node_volume_size.to_string(),
        ));
    }

    if let Some(nat_gateway_vm_size) = &options.symmetric_nat_gateway_vm_size {
        args.push((
            "symmetric_nat_gateway_droplet_size".to_string(),
            nat_gateway_vm_size.clone(),
        ));
    }

    if let Some(symmetric_private_node_vm_count) = options.symmetric_private_node_vm_count {
        args.push((
            "symmetric_private_node_vm_count".to_string(),
            symmetric_private_node_vm_count.to_string(),
        ));
    }

    if let Some(symmetric_private_node_volume_size) = options.symmetric_private_node_volume_size {
        args.push((
            "symmetric_private_node_volume_size".to_string(),
            symmetric_private_node_volume_size.to_string(),
        ));
    }

    if let Some(uploader_image_id) = &options.uploader_image_id {
        args.push((
            "uploader_droplet_image_id".to_string(),
            uploader_image_id.clone(),
        ));
    }

    if let Some(uploader_vm_count) = options.uploader_vm_count {
        args.push((
            "uploader_vm_count".to_string(),
            uploader_vm_count.to_string(),
        ));
    }

    if let Some(uploader_vm_size) = &options.uploader_vm_size {
        args.push((
            "uploader_droplet_size".to_string(),
            uploader_vm_size.clone(),
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
