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
const CLIENT: &str = "ant_client";
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

const SIZE: &str = "size";
const IMAGE: &str = "image";

#[derive(Clone, Debug)]
pub struct InfraRunOptions {
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub client_image_id: Option<String>,
    pub client_vm_count: Option<u16>,
    pub client_vm_size: Option<String>,
    pub enable_build_vm: bool,
    pub evm_node_count: Option<u16>,
    pub evm_node_vm_size: Option<String>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub evm_node_image_id: Option<String>,
    pub full_cone_vm_size: Option<String>,
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
    pub region: String,
    pub symmetric_nat_gateway_vm_size: Option<String>,
    pub symmetric_private_node_vm_count: Option<u16>,
    pub symmetric_private_node_volume_size: Option<u16>,
    pub tfvars_filenames: Option<Vec<String>>,
}

impl InfraRunOptions {
    /// Generate the options for an existing deployment.
    pub async fn generate_existing(
        name: &str,
        region: &str,
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
                    get_value_for_resource(&resources, PEER_CACHE_NODE_ATTACHED_VOLUME, SIZE)?;
                debug!("Peer cache node volume size: {volume_size:?}");
                let vm_size = get_value_for_resource(&resources, PEER_CACHE_NODE, SIZE)?;
                debug!("Peer cache node size: {vm_size:?}");
                let image_id = get_value_for_resource(&resources, PEER_CACHE_NODE, IMAGE)?;
                debug!("Peer cache node image id: {image_id:?}");

                (volume_size, vm_size, image_id)
            } else {
                (None, None, None)
            };

        let genesis_node_vm_count = resource_count(GENESIS_NODE);
        debug!("Genesis node count: {genesis_node_vm_count}");
        let genesis_node_volume_size = if genesis_node_vm_count > 0 {
            get_value_for_resource(&resources, GENESIS_NODE_ATTACHED_VOLUME, SIZE)?
        } else {
            None
        };
        debug!("Genesis node volume size: {genesis_node_volume_size:?}");

        let node_vm_count = resource_count(NODE);
        debug!("Node count: {node_vm_count}");
        let node_volume_size = if node_vm_count > 0 {
            get_value_for_resource(&resources, NODE_ATTACHED_VOLUME, SIZE)?
        } else {
            None
        };
        debug!("Node volume size: {node_volume_size:?}");

        let mut nat_gateway_image_id: Option<String> = None;
        let symmetric_private_node_vm_count = resource_count(SYMMETRIC_PRIVATE_NODE);
        debug!("Symmetric private node count: {symmetric_private_node_vm_count}");
        let (symmetric_private_node_volume_size, symmetric_nat_gateway_vm_size) =
            if symmetric_private_node_vm_count > 0 {
                let symmetric_private_node_volume_size = get_value_for_resource(
                    &resources,
                    SYMMETRIC_PRIVATE_NODE_ATTACHED_VOLUME,
                    SIZE,
                )?;
                debug!(
                    "Symmetric private node volume size: {symmetric_private_node_volume_size:?}"
                );
                // gateways should exists if private nodes exist
                let symmetric_nat_gateway_vm_size =
                    get_value_for_resource(&resources, SYMMETRIC_NAT_GATEWAY, SIZE)?;

                debug!("Symmetric nat gateway size: {symmetric_nat_gateway_vm_size:?}");

                nat_gateway_image_id =
                    get_value_for_resource(&resources, SYMMETRIC_NAT_GATEWAY, IMAGE)?;
                debug!("Nat gateway image: {nat_gateway_image_id:?}");

                (
                    symmetric_private_node_volume_size,
                    symmetric_nat_gateway_vm_size,
                )
            } else {
                (None, None)
            };

        let full_cone_private_node_vm_count = resource_count(FULL_CONE_PRIVATE_NODE);
        debug!("Full cone private node count: {full_cone_private_node_vm_count}");
        let (full_cone_private_node_volume_size, full_cone_vm_size) =
            if full_cone_private_node_vm_count > 0 {
                let full_cone_private_node_volume_size = get_value_for_resource(
                    &resources,
                    FULL_CONE_PRIVATE_NODE_ATTACHED_VOLUME,
                    SIZE,
                )?;
                debug!(
                    "Full cone private node volume size: {full_cone_private_node_volume_size:?}"
                );
                // gateways should exists if private nodes exist
                let full_cone_vm_size =
                    get_value_for_resource(&resources, FULL_CONE_NAT_GATEWAY, SIZE)?;
                debug!("Full cone nat gateway size: {full_cone_vm_size:?}");

                nat_gateway_image_id =
                    get_value_for_resource(&resources, FULL_CONE_NAT_GATEWAY, IMAGE)?;
                debug!("Nat gateway image: {nat_gateway_image_id:?}");

                (
                    full_cone_private_node_volume_size,
                    full_cone_vm_size,
                )
            } else {
                (None, None)
            };

        let client_vm_count = resource_count(CLIENT);
        debug!("Client count: {client_vm_count}");
        let (client_vm_size, client_image_id) = if client_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, CLIENT, SIZE)?;
            debug!("Client size: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, CLIENT, IMAGE)?;
            debug!("Client image id: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let build_vm_count = resource_count(BUILD_VM);
        debug!("Build VM count: {build_vm_count}");
        let enable_build_vm = build_vm_count > 0;

        // Node VM size var is re-used for nodes, evm nodes, symmetric and full cone private nodes
        let (node_vm_size, node_image_id) = if node_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, NODE, SIZE)?;
            debug!("Node size obtained from {NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, NODE, IMAGE)?;
            debug!("Node image id obtained from {NODE}: {image_id:?}");
            (vm_size, image_id)
        } else if symmetric_private_node_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, SYMMETRIC_PRIVATE_NODE, SIZE)?;
            debug!("Node size obtained from {SYMMETRIC_PRIVATE_NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, SYMMETRIC_PRIVATE_NODE, IMAGE)?;
            debug!("Node image id obtained from {SYMMETRIC_PRIVATE_NODE}: {image_id:?}");
            (vm_size, image_id)
        } else if full_cone_private_node_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, FULL_CONE_PRIVATE_NODE, SIZE)?;
            debug!("Node size obtained from {FULL_CONE_PRIVATE_NODE}: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, FULL_CONE_PRIVATE_NODE, IMAGE)?;
            debug!("Node image id obtained from {FULL_CONE_PRIVATE_NODE}: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let evm_node_count = resource_count(EVM_NODE);
        debug!("EVM node count: {evm_node_count}");
        let (evm_node_vm_size, evm_node_image_id) = if evm_node_count > 0 {
            let emv_node_vm_size = get_value_for_resource(&resources, EVM_NODE, SIZE)?;
            debug!("EVM node size: {emv_node_vm_size:?}");
            let evm_node_image_id = get_value_for_resource(&resources, EVM_NODE, IMAGE)?;
            debug!("EVM node image id: {evm_node_image_id:?}");
            (emv_node_vm_size, evm_node_image_id)
        } else {
            (None, None)
        };

        let options = Self {
            client_image_id,
            client_vm_count: Some(client_vm_count),
            client_vm_size,
            enable_build_vm,
            evm_node_count: Some(evm_node_count),
            evm_node_vm_size,
            evm_node_image_id,
            full_cone_vm_size,
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
            region: region.to_string(),
            symmetric_nat_gateway_vm_size,
            symmetric_private_node_vm_count: Some(symmetric_private_node_vm_count),
            symmetric_private_node_volume_size,
            tfvars_filenames: environment_details
                .map(|details| details.environment_type.get_tfvars_filenames(name, region)),
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
            .apply(args, options.tfvars_filenames.clone())?;
        print_duration(start.elapsed());
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ClientsInfraRunOptions {
    pub client_image_id: Option<String>,
    pub client_vm_count: Option<u16>,
    pub client_vm_size: Option<String>,
    /// Set to None for new deployments, as the value will be fetched from tfvars.
    pub enable_build_vm: bool,
    pub name: String,
    pub tfvars_filenames: Vec<String>,
}

impl ClientsInfraRunOptions {
    /// Generate the options for an existing Client deployment.
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

        let client_vm_count = resource_count(CLIENT);
        debug!("Client count: {client_vm_count}");
        let (client_vm_size, client_image_id) = if client_vm_count > 0 {
            let vm_size = get_value_for_resource(&resources, CLIENT, SIZE)?;
            debug!("Client size: {vm_size:?}");
            let image_id = get_value_for_resource(&resources, CLIENT, IMAGE)?;
            debug!("Client image id: {image_id:?}");
            (vm_size, image_id)
        } else {
            (None, None)
        };

        let build_vm_count = resource_count(BUILD_VM);
        debug!("Build VM count: {build_vm_count}");
        let enable_build_vm = build_vm_count > 0;

        let options = Self {
            client_image_id,
            client_vm_count: Some(client_vm_count),
            client_vm_size,
            enable_build_vm,
            name: name.to_string(),
            tfvars_filenames: environment_details
                .environment_type
                .get_tfvars_filenames(name, &environment_details.region),
        };

        Ok(options)
    }

    pub fn build_terraform_args(&self) -> Result<Vec<(String, String)>> {
        let mut args = Vec::new();

        if let Some(client_vm_count) = self.client_vm_count {
            args.push((
                "ant_client_vm_count".to_string(),
                client_vm_count.to_string(),
            ));
        }
        if let Some(client_vm_size) = &self.client_vm_size {
            args.push((
                "ant_client_droplet_size".to_string(),
                client_vm_size.clone(),
            ));
        }
        if let Some(client_image_id) = &self.client_image_id {
            args.push((
                "ant_client_droplet_image_id".to_string(),
                client_image_id.clone(),
            ));
        }

        args.push((
            "use_custom_bin".to_string(),
            self.enable_build_vm.to_string(),
        ));

        Ok(args)
    }
}

/// Build the terraform arguments from InfraRunOptions
pub fn build_terraform_args(options: &InfraRunOptions) -> Result<Vec<(String, String)>> {
    let mut args = Vec::new();

    args.push(("region".to_string(), options.region.clone()));

    if let Some(client_image_id) = &options.client_image_id {
        args.push((
            "ant_client_droplet_image_id".to_string(),
            client_image_id.clone(),
        ));
    }

    if let Some(client_vm_count) = options.client_vm_count {
        args.push((
            "ant_client_vm_count".to_string(),
            client_vm_count.to_string(),
        ));
    }

    if let Some(client_vm_size) = &options.client_vm_size {
        args.push((
            "ant_client_droplet_size".to_string(),
            client_vm_size.clone(),
        ));
    }

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

    if let Some(full_cone_vm_size) = &options.full_cone_vm_size {
        args.push((
            "full_cone_droplet_size".to_string(),
            full_cone_vm_size.clone(),
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

/// Extract a specific field value from terraform resources with proper type conversion.
fn get_value_for_resource<T>(
    resources: &[TerraformResource],
    resource_name: &str,
    field_name: &str,
) -> Result<Option<T>, Error>
where
    T: From<TerraformValue>,
{
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

    Ok(field_value.map(TerraformValue::from).map(T::from))
}

/// Wrapper for terraform values to ensure proper conversion
#[derive(Debug, Clone)]
enum TerraformValue {
    String(String),
    Number(u64),
    Bool(bool),
    Other(serde_json::Value),
}

impl From<serde_json::Value> for TerraformValue {
    fn from(value: serde_json::Value) -> Self {
        if value.is_string() {
            // Extract the inner string without quotes
            // Unwrap is safe here because we checked is_string above
            TerraformValue::String(value.as_str().unwrap().to_string())
        } else if value.is_u64() {
            // Unwrap is safe here because we checked is_u64 above
            TerraformValue::Number(value.as_u64().unwrap())
        } else if value.is_boolean() {
            // Unwrap is safe here because we checked is_boolean above
            TerraformValue::Bool(value.as_bool().unwrap())
        } else {
            TerraformValue::Other(value)
        }
    }
}

// Implement From<TerraformValue> for the types you need
impl From<TerraformValue> for String {
    fn from(value: TerraformValue) -> Self {
        match value {
            TerraformValue::String(s) => s,
            TerraformValue::Number(n) => n.to_string(),
            TerraformValue::Bool(b) => b.to_string(),
            TerraformValue::Other(v) => v.to_string(),
        }
    }
}

impl From<TerraformValue> for u16 {
    fn from(value: TerraformValue) -> Self {
        match value {
            TerraformValue::Number(n) => n as u16,
            TerraformValue::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }
}
