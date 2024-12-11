// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

/// These Static IPs are applied to PeerCache VMs in the PROD environment.
/// If they're already assigned to an environment, re-running the deploy on a new
/// environment will deallocate them from the previous environment and assign them to the new one.
const PROD_RESERVED_IPS: [&str; 6] = [
    "159.89.251.80",
    "159.65.210.89",
    "159.223.246.45",
    "139.59.201.153",
    "139.59.200.27",
    "139.59.198.251",
];

const STG_01_RESERVED_IPS: [&str; 3] = [
    "46.101.64.144",
    "209.38.170.72",
    "209.38.170.69",
    // "209.38.170.58",
    // "209.38.168.56",
    // "209.38.160.11",
];

const STG_02_RESERVED_IPS: [&str; 3] = ["188.166.136.212", "157.245.28.60", "138.68.117.227"];

/// Get the reserved IPs for a given environment.
pub fn get_reserved_ips_args(name: &str) -> Option<String> {
    if name.starts_with("PROD") {
        return Some(
            serde_json::to_string(&PROD_RESERVED_IPS).expect("Failed to serialize static IPs"),
        );
    }
    match name {
        "STG-01" => Some(
            serde_json::to_string(&STG_01_RESERVED_IPS).expect("Failed to serialize static IPs"),
        ),
        "STG-02" => Some(
            serde_json::to_string(&STG_02_RESERVED_IPS).expect("Failed to serialize static IPs"),
        ),
        _ => None,
    }
}
