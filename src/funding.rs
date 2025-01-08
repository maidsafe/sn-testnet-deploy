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
    EvmNetwork,
};
use alloy::primitives::Address;
use alloy::{network::EthereumWallet, signers::local::PrivateKeySigner};
use evmlib::{common::U256, wallet::Wallet, Network};
use log::{debug, error, warn};
use std::collections::HashMap;
use std::str::FromStr;

/// 1 token (1e18)
const DEFAULT_TOKEN_AMOUNT: &str = "1_000_000_000_000_000_000";
/// 0.1 ETH (1e17)
const DEFAULT_GAS_AMOUNT: &str = "100_000_000_000_000_000";

pub struct FundingOptions {
    pub evm_network: EvmNetwork,
    /// For custom network
    pub evm_data_payments_address: Option<String>,
    /// For custom network
    pub evm_payment_token_address: Option<String>,
    /// For custom network
    pub evm_rpc_url: Option<String>,
    pub funding_wallet_secret_key: Option<String>,
    /// Have to specify during upscale and deploy
    pub uploaders_count: Option<u16>,
    /// The amount of tokens to transfer to each uploader.
    /// Defaults to 1 token, i.e., 1_000_000_000_000_000_000
    pub token_amount: Option<U256>,
    /// The amount of gas tokens to transfer to each uploader
    /// Defaults to 0.1 ETH i.e, 100_000_000_000_000_000
    pub gas_amount: Option<U256>,
}

impl AnsibleProvisioner {
    /// Retrieve the uploader secret keys for all uploader VMs
    pub fn get_uploader_secret_keys(
        &self,
    ) -> Result<HashMap<VirtualMachine, Vec<PrivateKeySigner>>> {
        let uploaders_count = self.get_current_uploader_count()?;

        debug!("Fetching uploader secret keys");
        let mut uploader_secret_keys = HashMap::new();

        if uploaders_count.is_empty() {
            debug!("No uploaders VMs found");
            return Err(Error::EmptyInventory(AnsibleInventoryType::Uploaders));
        }

        for (vm, count) in uploaders_count {
            if count == 0 {
                warn!("No uploader instances found for {:?}, ", vm.name);
                uploader_secret_keys.insert(vm.clone(), Vec::new());
            } else {
                let sks = self.get_uploader_secret_key_per_vm(&vm, count)?;
                uploader_secret_keys.insert(vm.clone(), sks);
            }
        }

        Ok(uploader_secret_keys)
    }

    /// Deposit funds from the funding_wallet_secret_key to the uploader wallets
    /// If FundingOptions::uploaders_count is provided, it will generate the missing secret keys.
    /// If not provided, we'll just fund the existing uploader wallets
    pub async fn deposit_funds_to_uploaders(
        &self,
        options: &FundingOptions,
    ) -> Result<HashMap<VirtualMachine, Vec<PrivateKeySigner>>> {
        debug!("Funding all the uploader wallets");
        let mut uploader_secret_keys = self.get_uploader_secret_keys()?;

        for (vm, keys) in uploader_secret_keys.iter_mut() {
            if let Some(provided_count) = options.uploaders_count {
                if provided_count < keys.len() as u16 {
                    error!("Provided {provided_count} is less than the existing {} uploaders count for {}", keys.len(), vm.name);
                    return Err(Error::InvalidUpscaleDesiredUploaderCount);
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

        let funding_wallet_sk = if let Some(sk) = &options.funding_wallet_secret_key {
            Some(sk.parse().map_err(|_| Error::FailedToParseKey)?)
        } else {
            None
        };

        self.deposit_funds(funding_wallet_sk, &uploader_secret_keys, options)
            .await?;

        Ok(uploader_secret_keys)
    }

    /// Drain all the funds from the uploader wallets to the provided wallet
    pub async fn drain_funds_from_uploaders(
        &self,
        to_address: Address,
        evm_network: Network,
    ) -> Result<()> {
        debug!("Draining all the uploader wallets to {to_address:?}");
        println!("Draining all the uploader wallets to {to_address:?}");
        let uploader_secret_keys = self.get_uploader_secret_keys()?;

        for (vm, keys) in uploader_secret_keys.iter() {
            debug!(
                "Draining funds for uploader vm: {} to {to_address:?}",
                vm.name
            );
            for uploader_sk in keys.iter() {
                debug!(
                    "Draining funds for uploader vm: {} with key: {uploader_sk:?}",
                    vm.name,
                );

                let from_wallet = Wallet::new(
                    evm_network.clone(),
                    EthereumWallet::new(uploader_sk.clone()),
                );

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

    /// Return the (vm name, uploader count) for all uploader VMs
    fn get_current_uploader_count(&self) -> Result<HashMap<VirtualMachine, usize>> {
        let uploader_inventories = self
            .ansible_runner
            .get_inventory(AnsibleInventoryType::Uploaders, true)?;
        if uploader_inventories.is_empty() {
            debug!("No uploaders VMs found");
            return Err(Error::EmptyInventory(AnsibleInventoryType::Uploaders));
        }

        let mut uploader_count = HashMap::new();

        for vm in uploader_inventories {
            debug!(
                "Fetching uploader count for {} @ {}",
                vm.name, vm.public_ip_addr
            );
            let cmd = "systemctl list-units --type=service --all | grep ant_uploader_ | wc -l";
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
                    uploader_count.insert(vm.clone(), count);
                }
                Err(Error::ExternalCommandRunFailed {
                    binary,
                    exit_status,
                }) => {
                    if let Some(1) = exit_status.code() {
                        debug!("No uploaders found for {:?}", vm.public_ip_addr);
                        uploader_count.insert(vm.clone(), 0);
                    } else {
                        debug!("Error while fetching uploader count with different exit code {exit_status:?}",);
                        return Err(Error::ExternalCommandRunFailed {
                            binary,
                            exit_status,
                        });
                    }
                }
                Err(err) => {
                    debug!("Error while fetching uploader count: {err:?}",);
                    return Err(err);
                }
            }
        }

        Ok(uploader_count)
    }

    fn get_uploader_secret_key_per_vm(
        &self,
        vm: &VirtualMachine,
        instance_count: usize,
    ) -> Result<Vec<PrivateKeySigner>> {
        let mut sks_per_vm = Vec::new();

        debug!(
            "Fetching uploader secret key for {} @ {}",
            vm.name, vm.public_ip_addr
        );
        // Note: if this is gonna be parallelized, we need to make sure the secret keys are in order.
        // the playbook expects them in order
        for count in 1..=instance_count {
            let cmd = format!(
                "systemctl show ant_uploader_{count}.service --property=Environment | grep SECRET_KEY | cut -d= -f3 | awk '{{print $1}}'"
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
        funding_wallet_sk: Option<PrivateKeySigner>,
        all_secret_keys: &HashMap<VirtualMachine, Vec<PrivateKeySigner>>,
        options: &FundingOptions,
    ) -> Result<()> {
        if all_secret_keys.is_empty() {
            error!("No uploader secret keys found");
            return Err(Error::SecretKeyNotFound);
        }

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
                    )
                } else {
                    error!("Custom evm network data not provided");
                    return Err(Error::EvmTestnetDataNotFound);
                };

                let Some(deployer_wallet_sk) = &options.funding_wallet_secret_key else {
                    error!("Deployer wallet secret key not provided");
                    return Err(Error::SecretKeyNotFound);
                };
                let deployer_wallet_sk: PrivateKeySigner = deployer_wallet_sk
                    .parse()
                    .map_err(|_| Error::FailedToParseKey)?;

                Wallet::new(network.clone(), EthereumWallet::new(deployer_wallet_sk))
            }
            EvmNetwork::ArbitrumOne => {
                let funding_wallet_sk = funding_wallet_sk.ok_or_else(|| {
                    error!("Funding wallet secret key not provided");
                    Error::SecretKeyNotFound
                })?;
                let network = Network::ArbitrumOne;
                Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk))
            }
            EvmNetwork::ArbitrumSepolia => {
                let funding_wallet_sk = funding_wallet_sk.ok_or_else(|| {
                    error!("Funding wallet secret key not provided");
                    Error::SecretKeyNotFound
                })?;
                let network = Network::ArbitrumSepolia;
                Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk))
            }
        };
        debug!("Using emv network: {:?}", options.evm_network);

        let token_balance = from_wallet.balance_of_tokens().await?;
        let gas_balance = from_wallet.balance_of_gas_tokens().await?;
        println!("Funding wallet token balance: {token_balance}");
        println!("Funding wallet gas balance: {gas_balance}");
        debug!("Funding wallet token balance: {token_balance:?} and gas balance {gas_balance}");

        let default_token_amount = U256::from_str(DEFAULT_TOKEN_AMOUNT).unwrap();
        let default_gas_amount = U256::from_str(DEFAULT_GAS_AMOUNT).unwrap();

        let tokens_for_each_uploader = options.token_amount.unwrap_or(default_token_amount);
        let gas_for_each_uploader = options.gas_amount.unwrap_or(default_gas_amount);

        println!("Transferring {tokens_for_each_uploader} tokens and {gas_for_each_uploader} gas tokens to each uploader");
        debug!("Transferring {tokens_for_each_uploader} tokens and {gas_for_each_uploader} gas tokens to each uploader");

        for (vm, sks_per_machine) in all_secret_keys.iter() {
            debug!("Transferring funds for uploader vm: {}", vm.name);
            for sk in sks_per_machine.iter() {
                sk.address();

                if !tokens_for_each_uploader.is_zero() {
                    debug!(
                        "Transferring {tokens_for_each_uploader} tokens for uploader vm: {} with public key: {}",
                        vm.name,
                        sk.address()
                    );
                    from_wallet
                    .transfer_tokens(sk.address(), tokens_for_each_uploader)
                    .await.inspect_err(|err| {
                        debug!(
                            "Failed to transfer {tokens_for_each_uploader} tokens to {} with err: {err:?}", sk.address()
                        )
                    })?;
                }
                if !gas_for_each_uploader.is_zero() {
                    debug!(
                        "Transferring {gas_for_each_uploader} gas for uploader vm: {} with public key: {}",
                        vm.name,
                        sk.address()
                    );
                    from_wallet
                    .transfer_gas_tokens(sk.address(), gas_for_each_uploader)
                    .await
                    .inspect_err(|err| {
                        debug!(
                            "Failed to transfer {gas_for_each_uploader} gas tokens to {} with err: {err:?}", sk.address()
                        )
                    })
                    ?;
                }
            }
        }
        println!("All Funds transferred successfully");
        debug!("All Funds transferred successfully");

        Ok(())
    }
}

/// Get the Address of the funding wallet from the secret key string
pub fn get_address_from_sk(secret_key: &str) -> Result<Address> {
    let sk: PrivateKeySigner = secret_key.parse().map_err(|_| Error::FailedToParseKey)?;
    Ok(sk.address())
}
