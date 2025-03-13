// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::*;

use alloy::primitives::{Address, U256};
use clap::Subcommand;
use color_eyre::{eyre::eyre, Result};
use evmlib::Network;
use sn_testnet_deploy::{
    funding::FundingOptions, get_environment_details, inventory::DeploymentInventoryService,
    CloudProvider, EvmNetwork, TestnetDeployBuilder,
};
use std::str::FromStr;

#[derive(Subcommand, Debug)]
pub enum FundsCommand {
    /// Deposit tokens and gas from the provided funding wallet secret key to all the uploaders
    Deposit {
        /// The secret key for the wallet that will fund all the uploaders.
        ///
        /// This argument only applies when Arbitrum or Sepolia networks are used.
        #[clap(long)]
        funding_wallet_secret_key: Option<String>,
        /// The number of gas to transfer, in U256
        ///
        /// 1 ETH = 1_000_000_000_000_000_000. Defaults to 0.1 ETH
        #[arg(long)]
        gas_to_transfer: Option<U256>,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The number of tokens to transfer, in U256
        ///
        /// 1 Token = 1_000_000_000_000_000_000. Defaults to 100 token.
        #[arg(long)]
        tokens_to_transfer: Option<U256>,
    },
    /// Drain all the tokens and gas from the uploaders to the funding wallet.
    Drain {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
        /// The address of the wallet that will receive all the tokens and gas.
        ///
        /// This argument is optional, the funding wallet address from the S3 environment file will be used by default.
        #[clap(long)]
        to_address: Option<String>,
    },
}

pub async fn handle_funds_command(cmd: FundsCommand) -> Result<()> {
    match cmd {
        FundsCommand::Deposit {
            funding_wallet_secret_key,
            gas_to_transfer,
            name,
            provider,
            tokens_to_transfer,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            let inventory_services = DeploymentInventoryService::from(&testnet_deployer);
            inventory_services
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            let environment_details =
                get_environment_details(&name, &inventory_services.s3_repository).await?;

            let options = FundingOptions {
                evm_data_payments_address: environment_details.evm_details.data_payments_address,
                evm_payment_token_address: environment_details.evm_details.payment_token_address,
                evm_rpc_url: environment_details.evm_details.rpc_url,
                evm_network: environment_details.evm_details.network,
                funding_wallet_secret_key,
                uploaders_count: None,
                token_amount: tokens_to_transfer,
                gas_amount: gas_to_transfer,
            };
            testnet_deployer
                .ansible_provisioner
                .deposit_funds_to_uploaders(&options)
                .await?;

            Ok(())
        }
        FundsCommand::Drain {
            name,
            provider,
            to_address,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;

            let inventory_services = DeploymentInventoryService::from(&testnet_deployer);
            inventory_services
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            let environment_details =
                get_environment_details(&name, &inventory_services.s3_repository).await?;

            let to_address = if let Some(to_address) = to_address {
                Address::from_str(&to_address)?
            } else if let Some(to_address) = environment_details.funding_wallet_address {
                Address::from_str(&to_address)?
            } else {
                return Err(eyre!(
                    "No to-address was provided and no funding wallet address was found in the environment details"
                ));
            };

            let network = match environment_details.evm_details.network {
                EvmNetwork::Anvil => {
                    return Err(eyre!(
                        "Draining funds from uploaders is not supported for an Anvil network"
                    ));
                }
                EvmNetwork::ArbitrumOne => Network::ArbitrumOne,
                EvmNetwork::ArbitrumSepolia => Network::ArbitrumSepolia,
                EvmNetwork::Custom => {
                    if let (
                        Some(emv_data_payments_address),
                        Some(evm_payment_token_address),
                        Some(evm_rpc_url),
                    ) = (
                        environment_details.evm_details.data_payments_address,
                        environment_details.evm_details.payment_token_address,
                        environment_details.evm_details.rpc_url,
                    ) {
                        Network::new_custom(
                            &evm_rpc_url,
                            &evm_payment_token_address,
                            &emv_data_payments_address,
                        )
                    } else {
                        return Err(eyre!(
                            "Custom EVM details not found in the environment details"
                        ));
                    }
                }
            };

            testnet_deployer
                .ansible_provisioner
                .drain_funds_from_uploaders(to_address, network)
                .await?;

            Ok(())
        }
    }
}
