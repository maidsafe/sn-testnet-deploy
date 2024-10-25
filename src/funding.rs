// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::Result;
use crate::{
    ansible::{inventory::AnsibleInventoryType, provisioning::AnsibleProvisioner},
    error::Error,
    get_evm_testnet_data,
    inventory::VirtualMachine,
    EvmNetwork,
};
use alloy::{network::EthereumWallet, signers::local::PrivateKeySigner};
use evmlib::{common::U256, wallet::Wallet, Network};
use log::{debug, error, warn};
use std::collections::HashMap;
use std::str::FromStr;

pub struct FundingOptions {
    pub evm_network: EvmNetwork,
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

    /// Send funds from the funding_wallet_secret_key to the uploader wallets
    /// If FundingOptions::uploaders_count is provided, it will generate the missing secret keys.
    /// If not provided, we'll just fund the existing uploader wallets
    pub async fn fund_uploader_wallets(
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
            Some(sk.parse().map_err(|_| Error::SecretKeyParseError)?)
        } else {
            None
        };

        self.transfer_funds(funding_wallet_sk, &uploader_secret_keys, options)
            .await?;

        Ok(uploader_secret_keys)
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
            let cmd = "systemctl list-units --type=service | grep autonomi_uploader_ | wc -l";
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
                        .map_err(|_| Error::SecretKeyParseError)?;
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
                "systemctl show autonomi_uploader_{count}.service --property=Environment | grep SECRET_KEY | cut -d= -f3 | awk '{{print $1}}'"
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
                    let sk = sk_str.parse().map_err(|_| Error::SecretKeyParseError)?;

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

    async fn transfer_funds(
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

        let (from_wallet, network) = match &options.evm_network {
            EvmNetwork::Custom => {
                let evm_testnet_data =
                    get_evm_testnet_data(&self.ansible_runner, &self.ssh_client)?;
                let network = Network::new_custom(
                    &evm_testnet_data.rpc_url,
                    &evm_testnet_data.payment_token_address,
                    &evm_testnet_data.data_payments_address,
                );
                let deployer_wallet_sk: PrivateKeySigner = evm_testnet_data
                    .deployer_wallet_private_key
                    .parse()
                    .map_err(|_| Error::SecretKeyParseError)?;

                let wallet = Wallet::new(network.clone(), EthereumWallet::new(deployer_wallet_sk));
                (wallet, network)
            }
            EvmNetwork::ArbitrumOne => {
                let funding_wallet_sk = funding_wallet_sk.ok_or_else(|| {
                    error!("Funding wallet secret key not provided");
                    Error::SecretKeyNotFound
                })?;
                let network = Network::ArbitrumOne;
                let wallet = Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk));
                (wallet, network)
            }
            EvmNetwork::ArbitrumSepolia => {
                let funding_wallet_sk = funding_wallet_sk.ok_or_else(|| {
                    error!("Funding wallet secret key not provided");
                    Error::SecretKeyNotFound
                })?;
                let network = Network::ArbitrumSepolia;
                let wallet = Wallet::new(network.clone(), EthereumWallet::new(funding_wallet_sk));
                (wallet, network)
            }
        };
        debug!("Using emv network: {network:?}",);

        let token_balance = from_wallet.balance_of_tokens().await?;
        let gas_balance = from_wallet.balance_of_gas_tokens().await?;
        println!("Funding wallet token balance: {token_balance}");
        println!("Funding wallet gas balance: {gas_balance}");
        debug!("Funding wallet token balance: {token_balance:?} and gas balance {gas_balance}");

        // let (tokens_for_each_uploader, _quotient) = token_balance.div_rem(U256::from(sk_count));

        // // +1 to make sure we have enough gas
        // let (gas_tokens_for_each_uploader, _quotient) =
        //     gas_balance.div_rem(U256::from(sk_count + 1));

        let default_token_amount = U256::from_str("1_000_000_000_000_000_000").unwrap();
        let default_gas_amount = U256::from_str("100_000_000_000_000_000").unwrap();

        let tokens_for_each_uploader = options.token_amount.unwrap_or(default_token_amount);
        let gas_tokens_for_each_uploader = options.gas_amount.unwrap_or(default_gas_amount);

        println!("Transferring {tokens_for_each_uploader} tokens and {gas_tokens_for_each_uploader} gas tokens to each uploader");
        debug!("Transferring {tokens_for_each_uploader} tokens and {gas_tokens_for_each_uploader} gas tokens to each uploader");

        for (vm, sks_per_machine) in all_secret_keys.iter() {
            debug!("Transferring funds for uploader vm: {}", vm.name);
            for sk in sks_per_machine.iter() {
                let to_wallet = Wallet::new(network.clone(), EthereumWallet::new(sk.clone()));
                debug!(
                    "Transferring funds for uploader vm: {} with public key: {}",
                    vm.name,
                    to_wallet.address()
                );

                from_wallet
                    .transfer_tokens(to_wallet.address(), tokens_for_each_uploader)
                    .await.inspect_err(|err| {
                        debug!(
                            "Failed to transfer {tokens_for_each_uploader} tokens to {} with err: {err:?}", to_wallet.address()
                        )
                    })?;
                from_wallet
                    .transfer_gas_tokens(to_wallet.address(), gas_tokens_for_each_uploader)
                    .await
                    .inspect_err(|err| {
                        debug!(
                            "Failed to transfer {gas_tokens_for_each_uploader} gas tokens to {} with err: {err:?}", to_wallet.address()
                        )
                    })
                    ?;
            }
        }
        println!("All Funds transferred successfully");
        debug!("All Funds transferred successfully");

        Ok(())
    }
}
