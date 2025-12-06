// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::Result;
use crate::{
    ansible::{inventory::AnsibleInventoryType, provisioning::AnsibleProvisioner},
    error::Error,
    inventory::VirtualMachine,
    EnvironmentDetails, EvmNetwork,
};
use alloy::primitives::Address;
use alloy::{network::EthereumWallet, signers::local::PrivateKeySigner};
use evmlib::{common::U256, wallet::Wallet, Network};
use log::{debug, error, warn};
use std::collections::HashMap;
use std::str::FromStr;

/// 100 token (1e20)
const DEFAULT_TOKEN_AMOUNT: &str = "100_000_000_000_000_000_000";
/// 0.1 ETH (1e17)
const DEFAULT_GAS_AMOUNT: &str = "100_000_000_000_000_000";

pub struct FundingOptions {
    pub evm_network: EvmNetwork,
    /// For custom network
    pub evm_data_payments_address: Option<String>,
    /// For custom network
    pub evm_merkle_payments_address: Option<String>,
    /// For custom network
    pub evm_payment_token_address: Option<String>,
    /// For custom network
    pub evm_rpc_url: Option<String>,
    pub funding_wallet_secret_key: Option<String>,
    /// The amount of gas tokens to transfer to each ant instance
    /// Defaults to 0.1 ETH i.e, 100_000_000_000_000_000
    pub gas_amount: Option<U256>,
    /// The amount of tokens to transfer to each ant instance.
    /// Defaults to 100 token, i.e., 100_000_000_000_000_000_000
    pub token_amount: Option<U256>,
    /// Have to specify during upscale and deploy
    pub uploaders_count: Option<u16>,
}

impl AnsibleProvisioner {
    /// Retrieve the Ant secret keys from all the Client VMs.
    pub fn get_client_secret_keys(&self) -> Result<HashMap<VirtualMachine, Vec<PrivateKeySigner>>> {
        let ant_instance_count = self.get_current_ant_instance_count()?;

        debug!("Fetching ANT secret keys");
        let mut ant_secret_keys = HashMap::new();

        if ant_instance_count.is_empty() {
            debug!("No Client VMs found");
            return Err(Error::EmptyInventory(AnsibleInventoryType::Clients));
        }

        for (vm, count) in ant_instance_count {
            if count == 0 {
                warn!("No ANT instances found for {:?}, ", vm.name);
                ant_secret_keys.insert(vm.clone(), Vec::new());
            } else {
                let sks = self.get_ant_secret_key_per_vm(&vm, count)?;
                ant_secret_keys.insert(vm.clone(), sks);
            }
        }

        Ok(ant_secret_keys)
    }

    /// Deposit funds from the funding_wallet_secret_key to the ant wallets
    /// If FundingOptions::ant_uploader_count is provided, it will generate the missing secret keys.
    /// If not provided, we'll just fund the existing ant wallets
    pub async fn deposit_funds_to_clients(
        &self,
        options: &FundingOptions,
    ) -> Result<HashMap<VirtualMachine, Vec<PrivateKeySigner>>> {
        debug!(
            "Funding secret key: {:?}",
            options.funding_wallet_secret_key
        );
        debug!("Funding all the ant wallets");
        let mut ant_secret_keys = self.get_client_secret_keys()?;

        for (vm, keys) in ant_secret_keys.iter_mut() {
            if let Some(provided_count) = options.uploaders_count {
                if provided_count < keys.len() as u16 {
                    error!("Provided {provided_count} is less than the existing {} ant uploader count for {}", keys.len(), vm.name);
                    return Err(Error::InvalidUpscaleDesiredClientCount);
                }
                let missing_keys_count = provided_count - keys.len() as u16;
                debug!(
                    "Found {} secret keys for {}, missing {missing_keys_count} keys",
                    keys.len(),
                    vm.name
                );
                if missing_keys_count > 0 {
                    debug!(
                        "Generating {missing_keys_count} secret keys for {}",
                        vm.name
                    );
                    for _ in 0..missing_keys_count {
                        let sk = PrivateKeySigner::random();
                        debug!("Generated key with address: {}", sk.address());
                        keys.push(sk);
                    }
                }
            }
        }

        self.deposit_funds(&ant_secret_keys, options).await?;

        Ok(ant_secret_keys)
    }

    pub async fn prepare_pre_funded_wallets(
        &self,
        wallet_keys: &[String],
    ) -> Result<HashMap<VirtualMachine, Vec<PrivateKeySigner>>> {
        debug!("Using pre-funded wallets");

        let client_vms = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Clients, true)?;
        if client_vms.is_empty() {
            return Err(Error::EmptyInventory(AnsibleInventoryType::Clients));
        }

        let total_keys = wallet_keys.len();
        let vm_count = client_vms.len();
        if !total_keys.is_multiple_of(vm_count) {
            return Err(Error::InvalidWalletCount(total_keys, vm_count));
        }

        let uploaders_per_vm = total_keys / vm_count;
        let mut vm_to_keys = HashMap::new();
        let mut key_index = 0;

        for vm in client_vms {
            let mut keys = Vec::new();
            for _ in 0..uploaders_per_vm {
                let sk_str = &wallet_keys[key_index];
                let sk = sk_str.parse().map_err(|_| Error::FailedToParseKey)?;
                keys.push(sk);
                key_index += 1;
            }
            vm_to_keys.insert(vm, keys);
        }

        Ok(vm_to_keys)
    }

    /// Drain all the funds from the local ANT wallets to the provided wallet
    pub async fn drain_funds_from_ant_instances(
        &self,
        to_address: Address,
        evm_network: Network,
    ) -> Result<()> {
        debug!("Draining all the local ANT wallets to {to_address:?}");
        println!("Draining all the local ANT wallets to {to_address:?}");
        let ant_secret_keys = self.get_client_secret_keys()?;

        for (vm, keys) in ant_secret_keys.iter() {
            debug!(
                "Draining funds for Client vm: {} to {to_address:?}",
                vm.name
            );
            for ant_sk in keys.iter() {
                debug!(
                    "Draining funds for Client vm: {} with key: {ant_sk:?}",
                    vm.name,
                );

                let from_wallet =
                    Wallet::new(evm_network.clone(), EthereumWallet::new(ant_sk.clone()));

                let token_balance = from_wallet.balance_of_tokens().await.inspect_err(|err| {
                    debug!(
                        "Failed to get token balance for {} with err: {err:?}",
                        from_wallet.address()
                    )
                })?;

                println!(
                    "Draining {token_balance} tokens from {} to {to_address:?}",
                    from_wallet.address()
                );
                debug!(
                    "Draining {token_balance} tokens from {} to {to_address:?}",
                    from_wallet.address()
                );

                if token_balance.is_zero() {
                    debug!(
                        "No tokens to drain from wallet: {} with token balance",
                        from_wallet.address()
                    );
                } else {
                    from_wallet
                        .transfer_tokens(to_address, token_balance)
                        .await
                        .inspect_err(|err| {
                            debug!(
                                "Failed to transfer {token_balance} tokens from {to_address} with err: {err:?}",
                            )
                        })?;
                    println!(
                        "Drained {token_balance} tokens from {} to {to_address:?}",
                        from_wallet.address()
                    );
                    debug!(
                        "Drained {token_balance} tokens from {} to {to_address:?}",
                        from_wallet.address()
                    );
                }

                let gas_balance = from_wallet
                    .balance_of_gas_tokens()
                    .await
                    .inspect_err(|err| {
                        debug!(
                            "Failed to get gas token balance for {} with err: {err:?}",
                            from_wallet.address()
                        )
                    })?;

                println!(
                    "Draining {gas_balance} gas from {} to {to_address:?}",
                    from_wallet.address()
                );
                debug!(
                    "Draining {gas_balance} gas from {} to {to_address:?}",
                    from_wallet.address()
                );

                if gas_balance.is_zero() {
                    debug!("No gas tokens to drain from wallet: {to_address}");
                } else {
                    from_wallet
                    // 0.001 gas
                        .transfer_gas_tokens(to_address, gas_balance - U256::from_str("10_000_000_000_000").unwrap()).await
                        .inspect_err(|err| {
                            debug!(
                                "Failed to transfer {gas_balance} gas from {to_address} with err: {err:?}",
                            )
                        })?;
                    println!(
                        "Drained {gas_balance} gas from {} to {to_address:?}",
                        from_wallet.address()
                    );
                    debug!(
                        "Drained {gas_balance} gas from {} to {to_address:?}",
                        from_wallet.address()
                    );
                }
            }
        }
        println!("All funds drained to {to_address:?} successfully");
        debug!("All funds drained to {to_address:?} successfully");

        Ok(())
    }

    /// Return the (vm name, ant_instance count) for all Client VMs
    pub fn get_current_ant_instance_count(&self) -> Result<HashMap<VirtualMachine, usize>> {
        let client_inventories = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Clients, true)?;
        if client_inventories.is_empty() {
            debug!("No Client VMs found");
            return Err(Error::EmptyInventory(AnsibleInventoryType::Clients));
        }

        let mut ant_instnace_count = HashMap::new();

        for vm in client_inventories {
            debug!(
                "Fetching ant instance count for {} @ {}",
                vm.name, vm.public_ip_addr
            );
            let cmd =
                "systemctl list-units --type=service --all | grep ant_random_uploader_ | wc -l";
            let result = self
                .ssh_client
                .run_command(&vm.public_ip_addr, "root", cmd, true);
            match result {
                Ok(count) => {
                    debug!("Count found to be {count:?}, parsing");
                    let count = count
                        .first()
                        .ok_or_else(|| {
                            error!("No count found for {}", vm.name);
                            Error::SecretKeyNotFound
                        })?
                        .trim()
                        .parse()
                        .map_err(|_| Error::FailedToParseKey)?;
                    ant_instnace_count.insert(vm.clone(), count);
                }
                Err(Error::ExternalCommandRunFailed {
                    binary,
                    exit_status,
                }) => {
                    if let Some(1) = exit_status.code() {
                        debug!("No ant instance found for {:?}", vm.public_ip_addr);
                        ant_instnace_count.insert(vm.clone(), 0);
                    } else {
                        debug!("Error while fetching ant instance count with different exit code {exit_status:?}",);
                        return Err(Error::ExternalCommandRunFailed {
                            binary,
                            exit_status,
                        });
                    }
                }
                Err(err) => {
                    debug!("Error while fetching ant instance count: {err:?}",);
                    return Err(err);
                }
            }
        }

        Ok(ant_instnace_count)
    }

    fn get_ant_secret_key_per_vm(
        &self,
        vm: &VirtualMachine,
        instance_count: usize,
    ) -> Result<Vec<PrivateKeySigner>> {
        let mut sks_per_vm = Vec::new();

        debug!(
            "Fetching ANT secret key for {} @ {}",
            vm.name, vm.public_ip_addr
        );
        // Note: if this is gonna be parallelized, we need to make sure the secret keys are in order.
        // the playbook expects them in order
        for count in 1..=instance_count {
            let cmd = format!(
                "systemctl show ant_random_uploader_{count}.service --property=Environment | grep SECRET_KEY | cut -d= -f3 | awk '{{print $1}}'"
            );
            debug!("Fetching secret key for {} instance {count}", vm.name);
            let result = self
                .ssh_client
                .run_command(&vm.public_ip_addr, "root", &cmd, true);
            match result {
                Ok(secret_keys) => {
                    let sk_str = secret_keys
                        .iter()
                        .map(|sk| sk.trim().to_string())
                        .collect::<Vec<String>>();
                    let sk_str = sk_str.first().ok_or({
                        debug!("No secret key found for {}", vm.name);
                        Error::SecretKeyNotFound
                    })?;
                    let sk = sk_str.parse().map_err(|_| Error::FailedToParseKey)?;

                    debug!("Secret keys found for {} instance {count}: {sk:?}", vm.name,);

                    sks_per_vm.push(sk);
                }
                Err(err) => {
                    debug!("Error while fetching secret key: {err}");
                    return Err(err);
                }
            }
        }

        Ok(sks_per_vm)
    }

    async fn deposit_funds(
        &self,
        all_secret_keys: &HashMap<VirtualMachine, Vec<PrivateKeySigner>>,
        options: &FundingOptions,
    ) -> Result<()> {
        if all_secret_keys.is_empty() {
            error!("No ANT secret keys found");
            return Err(Error::SecretKeyNotFound);
        }

        let funding_wallet_sk: PrivateKeySigner =
            if let Some(sk) = &options.funding_wallet_secret_key {
                sk.parse().map_err(|_| Error::FailedToParseKey)?
            } else {
                warn!("Funding wallet secret key not provided. Skipping funding.");
                return Ok(());
            };

        let _sk_count = all_secret_keys.values().map(|v| v.len()).sum::<usize>();

        let from_wallet = match &options.evm_network {
            EvmNetwork::Anvil | EvmNetwork::Custom => {
                let network = if let (
                    Some(evm_data_payments_address),
                    Some(evm_payment_token_address),
                    Some(evm_rpc_url),
                ) = (
                    options.evm_data_payments_address.as_ref(),
                    options.evm_payment_token_address.as_ref(),
                    options.evm_rpc_url.as_ref(),
                ) {
                    Network::new_custom(
                        evm_rpc_url,
                        evm_payment_token_address,
                        evm_data_payments_address,
                        options.evm_merkle_payments_address.as_deref(),
                    )
                } else {
                    error!("Custom evm network data not provided");
                    return Err(Error::EvmTestnetDataNotFound);
                };

                Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk))
            }
            EvmNetwork::ArbitrumOne => {
                let network = Network::ArbitrumOne;
                Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk))
            }
            EvmNetwork::ArbitrumSepoliaTest => {
                let network = Network::ArbitrumSepoliaTest;
                Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk))
            }
        };
        debug!("Using EVM network: {:?}", options.evm_network);

        let token_balance = from_wallet.balance_of_tokens().await?;
        let gas_balance = from_wallet.balance_of_gas_tokens().await?;
        println!("Funding wallet token balance: {token_balance}");
        println!("Funding wallet gas balance: {gas_balance}");
        debug!("Funding wallet token balance: {token_balance:?} and gas balance {gas_balance}");

        let default_token_amount = U256::from_str(DEFAULT_TOKEN_AMOUNT).unwrap();
        let default_gas_amount = U256::from_str(DEFAULT_GAS_AMOUNT).unwrap();

        let token_amount = options.token_amount.unwrap_or(default_token_amount);
        let gas_amount = options.gas_amount.unwrap_or(default_gas_amount);

        println!(
            "Transferring {token_amount} tokens and {gas_amount} gas tokens to each ANT instance"
        );
        debug!(
            "Transferring {token_amount} tokens and {gas_amount} gas tokens to each ANT instance"
        );

        for (vm, vm_secret_keys) in all_secret_keys.iter() {
            println!("Transferring funds for Client vm: {}", vm.name);
            for sk in vm_secret_keys.iter() {
                sk.address();

                if !token_amount.is_zero() {
                    print!("Transferring {token_amount} tokens to {}...", sk.address());
                    from_wallet
                        .transfer_tokens(sk.address(), token_amount)
                        .await
                        .inspect_err(|err| {
                            debug!(
                                "Failed to transfer {token_amount} tokens to {}: {err:?}",
                                sk.address()
                            )
                        })?;
                    println!("Transfer complete");
                }
                if !gas_amount.is_zero() {
                    print!("Transferring {gas_amount} gas to {}...", sk.address());
                    from_wallet
                        .transfer_gas_tokens(sk.address(), gas_amount)
                        .await
                        .inspect_err(|err| {
                            debug!(
                                "Failed to transfer {gas_amount} gas to {}: {err:?}",
                                sk.address()
                            )
                        })?;
                    println!("Transfer complete");
                }
            }
        }
        println!("All funds transferred successfully");
        debug!("All funds transferred successfully");

        Ok(())
    }
}

/// Get the Address of the funding wallet from the secret key string
pub fn get_address_from_sk(secret_key: &str) -> Result<Address> {
    let sk: PrivateKeySigner = secret_key.parse().map_err(|_| Error::FailedToParseKey)?;
    Ok(sk.address())
}

pub async fn drain_funds(
    ansible_provisioner: &AnsibleProvisioner,
    environment_details: &EnvironmentDetails,
) -> Result<()> {
    let evm_network = match environment_details.evm_details.network {
        EvmNetwork::Anvil => None,
        EvmNetwork::Custom => Some(Network::new_custom(
            environment_details.evm_details.rpc_url.as_ref().unwrap(),
            environment_details
                .evm_details
                .payment_token_address
                .as_ref()
                .unwrap(),
            environment_details
                .evm_details
                .data_payments_address
                .as_ref()
                .unwrap(),
            environment_details
                .evm_details
                .merkle_payments_address
                .as_deref(),
        )),
        EvmNetwork::ArbitrumOne => Some(Network::ArbitrumOne),
        EvmNetwork::ArbitrumSepoliaTest => Some(Network::ArbitrumSepoliaTest),
    };

    if let (Some(network), Some(address)) =
        (evm_network, &environment_details.funding_wallet_address)
    {
        // Check if wallets exist before attempting to drain funds
        match ansible_provisioner.get_current_ant_instance_count() {
            Ok(ant_instances) if !ant_instances.is_empty() => {
                let has_wallets = ant_instances.values().any(|&count| count > 0);
                if has_wallets {
                    ansible_provisioner
                        .drain_funds_from_ant_instances(
                            Address::from_str(address).map_err(|err| {
                                log::error!("Invalid funding wallet public key: {err:?}");
                                Error::FailedToParseKey
                            })?,
                            network,
                        )
                        .await?;
                } else {
                    println!("No wallets found to drain funds from. Skipping wallet removal.");
                    log::info!("No wallets found to drain funds from. Skipping wallet removal.");
                }
            }
            Ok(_) | Err(_) => {
                println!("No client VMs or wallets found. Skipping wallet removal.");
                log::info!("No client VMs or wallets found. Skipping wallet removal.");
            }
        }
        Ok(())
    } else {
        println!("Custom network provided. Not draining funds.");
        log::info!("Custom network provided. Not draining funds.");
        Ok(())
    }
}
