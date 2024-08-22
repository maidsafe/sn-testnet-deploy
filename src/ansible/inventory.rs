// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::{AnsibleInventoryType, AnsibleRunner};
use crate::{
    ansible::AnsibleBinary, error::Error, inventory::VirtualMachine, run_external_command, Result,
};
use log::{debug, warn};
use serde::Deserialize;
use std::{collections::HashMap, net::IpAddr, time::Duration};

impl AnsibleRunner {
    /// Runs Ansible's inventory command and returns a list of VirtualMachines.
    pub async fn get_inventory(
        &self,
        inventory_type: AnsibleInventoryType,
        re_attempt: bool,
    ) -> Result<Vec<VirtualMachine>> {
        let retry_count = if re_attempt { 3 } else { 0 };
        let mut count = 0;
        let mut inventory = Vec::new();

        while count <= retry_count {
            debug!("Running inventory list. retry attempts {count}/{retry_count}");
            let output = run_external_command(
                AnsibleBinary::AnsibleInventory.get_binary_path()?,
                self.working_directory_path.clone(),
                vec![
                    "--inventory".to_string(),
                    self.get_inventory_path(&inventory_type)?
                        .to_string_lossy()
                        .to_string(),
                    "--list".to_string(),
                ],
                true,
                false,
            )?;

            debug!("Inventory list output:");
            debug!("{output:#?}");
            let mut output_string = output
                .into_iter()
                .skip_while(|line| !line.starts_with('{'))
                .collect::<Vec<String>>()
                .join("\n");
            if let Some(end_index) = output_string.rfind('}') {
                output_string.truncate(end_index + 1);
            }
            let parsed: Output = serde_json::from_str(&output_string)?;

            for host in parsed._meta.hostvars.values() {
                let public_ip_details = host
                    .do_networks
                    .v4
                    .iter()
                    .find(|&ip| ip.ip_type == IpType::Public)
                    .ok_or_else(|| Error::IpDetailsNotObtained)?;

                let private_ip_details = host
                    .do_networks
                    .v4
                    .iter()
                    .find(|&ip| ip.ip_type == IpType::Private)
                    .ok_or_else(|| Error::IpDetailsNotObtained)?;

                inventory.push(VirtualMachine {
                    id: host.do_id,
                    name: host.do_name.clone(),
                    public_ip_addr: public_ip_details.ip_address,
                    private_ip_addr: private_ip_details.ip_address,
                });
            }

            count += 1;
            if !inventory.is_empty() {
                break;
            }
            debug!("Inventory list is empty, re-running after a few seconds.");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        if inventory.is_empty() {
            warn!("Inventory list is empty after {retry_count} retries");
        }

        Ok(inventory)
    }
}

// The following three structs are utilities that are used to parse the output of the
// `ansible-inventory` command.
#[derive(Debug, Deserialize, Clone, PartialEq)]
enum IpType {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "private")]
    Private,
}

#[derive(Debug, Deserialize, Clone)]
struct IpDetails {
    ip_address: IpAddr,
    #[serde(rename = "type")]
    ip_type: IpType,
}

#[derive(Debug, Deserialize)]
struct DoNetworks {
    v4: Vec<IpDetails>,
}

#[derive(Debug, Deserialize)]
struct HostVar {
    do_id: u64,
    do_name: String,
    do_networks: DoNetworks,
}
#[derive(Debug, Deserialize)]
struct Meta {
    hostvars: HashMap<String, HostVar>,
}
#[derive(Debug, Deserialize)]
struct Output {
    _meta: Meta,
}
