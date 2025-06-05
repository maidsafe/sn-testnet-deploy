// Copyright (c) 2023, MaidSafe.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

mod cmd;

use crate::cmd::{
    network::{ChurnCommands, NetworkCommands},
    nodes,
    provision::ProvisionCommands,
    telegraf::TelegrafCommands,
    Commands,
};
use clap::Parser;
use color_eyre::Result;
use dotenv::dotenv;
use sn_testnet_deploy::{
    inventory::DeploymentInventoryService, setup::setup_dotenv_file, CloudProvider,
    EnvironmentType, TestnetDeployBuilder,
};
use std::env;

#[derive(Parser, Debug)]
#[clap(name = "sn-testnet-deploy", version = env!("CARGO_PKG_VERSION"))]
struct Opt {
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    dotenv().ok();
    env_logger::init();

    let opt = Opt::parse();
    match opt.command {
        Commands::Bootstrap {
            ansible_verbose,
            antctl_version,
            antnode_features,
            antnode_version,
            branch,
            chunk_size,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_payment_token_address,
            evm_rpc_url,
            full_cone_private_node_count,
            full_cone_private_node_vm_count,
            full_cone_private_node_volume_size,
            forks,
            interval,
            log_format,
            max_archived_log_files,
            max_log_files,
            name,
            network_id,
            network_contacts_url,
            node_count,
            node_env_variables,
            node_vm_count,
            node_volume_size,
            node_vm_size,
            peer,
            provider,
            region,
            repo_owner,
            rewards_address,
            skip_binary_build,
            symmetric_private_node_count,
            symmetric_private_node_vm_count,
            symmetric_private_node_volume_size,
        } => {
            cmd::deployments::handle_bootstrap(
                ansible_verbose,
                antctl_version,
                antnode_features,
                antnode_version,
                branch,
                chunk_size,
                node_env_variables,
                environment_type,
                evm_data_payments_address,
                evm_network_type,
                evm_payment_token_address,
                evm_rpc_url,
                forks,
                full_cone_private_node_count,
                full_cone_private_node_vm_count,
                full_cone_private_node_volume_size,
                interval,
                log_format,
                max_archived_log_files,
                max_log_files,
                name,
                network_contacts_url,
                network_id,
                node_count,
                node_vm_count,
                node_vm_size,
                node_volume_size,
                peer,
                provider,
                region,
                repo_owner,
                rewards_address,
                skip_binary_build,
                symmetric_private_node_count,
                symmetric_private_node_vm_count,
                symmetric_private_node_volume_size,
            )
            .await?;
            Ok(())
        }
        Commands::Clean { name, provider } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;

            testnet_deployer.clean().await?;
            Ok(())
        }
        Commands::Clients(clients_cmd) => {
            cmd::clients::handle_clients_command(clients_cmd).await?;
            Ok(())
        }
        Commands::Deploy {
            ansible_verbose,
            ant_version,
            antctl_version,
            antnode_features,
            antnode_version,
            branch,
            chunk_size,
            client_env_variables,
            client_vm_count,
            client_vm_size,
            disable_download_verifier,
            disable_performance_verifier,
            disable_random_verifier,
            disable_telegraf,
            environment_type,
            evm_data_payments_address,
            evm_network_type,
            evm_node_vm_size,
            evm_payment_token_address,
            evm_rpc_url,
            full_cone_nat_gateway_vm_size,
            full_cone_private_node_count,
            full_cone_private_node_vm_count,
            full_cone_private_node_volume_size,
            forks,
            funding_wallet_secret_key,
            genesis_node_volume_size,
            initial_gas,
            initial_tokens,
            interval,
            log_format,
            max_archived_log_files,
            max_log_files,
            name,
            network_id,
            network_contacts_file_name,
            network_dashboard_branch,
            node_count,
            node_env_variables,
            node_vm_count,
            node_vm_size,
            node_volume_size,
            peer_cache_node_count,
            peer_cache_node_vm_count,
            peer_cache_node_vm_size,
            peer_cache_node_volume_size,
            skip_binary_build,
            symmetric_nat_gateway_vm_size,
            symmetric_private_node_count,
            symmetric_private_node_vm_count,
            symmetric_private_node_volume_size,
            provider,
            public_rpc,
            region,
            repo_owner,
            rewards_address,
            to_genesis,
            uploaders_count,
            upload_interval,
        } => {
            cmd::deployments::handle_deploy(
                ansible_verbose,
                ant_version,
                antctl_version,
                antnode_features,
                antnode_version,
                branch,
                chunk_size,
                client_env_variables,
                client_vm_count,
                client_vm_size,
                disable_download_verifier,
                disable_performance_verifier,
                disable_random_verifier,
                disable_telegraf,
                environment_type,
                evm_data_payments_address,
                evm_network_type,
                evm_node_vm_size,
                evm_payment_token_address,
                evm_rpc_url,
                full_cone_nat_gateway_vm_size,
                full_cone_private_node_count,
                full_cone_private_node_vm_count,
                full_cone_private_node_volume_size,
                forks,
                funding_wallet_secret_key,
                genesis_node_volume_size,
                initial_gas,
                initial_tokens,
                interval,
                log_format,
                max_archived_log_files,
                max_log_files,
                name,
                network_id,
                network_contacts_file_name,
                network_dashboard_branch,
                node_count,
                node_env_variables,
                node_vm_count,
                node_vm_size,
                node_volume_size,
                peer_cache_node_count,
                peer_cache_node_vm_count,
                peer_cache_node_vm_size,
                peer_cache_node_volume_size,
                skip_binary_build,
                symmetric_nat_gateway_vm_size,
                symmetric_private_node_count,
                symmetric_private_node_vm_count,
                symmetric_private_node_volume_size,
                provider,
                public_rpc,
                region,
                repo_owner,
                rewards_address,
                to_genesis,
                uploaders_count,
                upload_interval,
            )
            .await?;
            Ok(())
        }
        Commands::ExtendVolumeSize {
            ansible_verbose,
            genesis_node_volume_size,
            full_cone_private_node_volume_size,
            node_volume_size,
            name,
            peer_cache_node_volume_size,
            provider,
            symmetric_private_node_volume_size,
        } => {
            cmd::misc::handle_extend_volume_size(
                ansible_verbose,
                genesis_node_volume_size,
                full_cone_private_node_volume_size,
                node_volume_size,
                name,
                peer_cache_node_volume_size,
                provider,
                symmetric_private_node_volume_size,
            )
            .await?;
            Ok(())
        }
        Commands::Funds(funds_cmd) => {
            cmd::funds::handle_funds_command(funds_cmd).await?;
            Ok(())
        }
        Commands::Inventory {
            force_regeneration,
            full,
            name,
            network_contacts_file_name,
            peer_cache,
            provider,
        } => {
            cmd::misc::handle_inventory(
                force_regeneration,
                full,
                name,
                network_contacts_file_name,
                peer_cache,
                provider,
            )
            .await?;
            Ok(())
        }
        Commands::Logs(log_cmd) => {
            cmd::logs::handle_logs_command(log_cmd).await?;
            Ok(())
        }
        Commands::Network(NetworkCommands::ChurnCommands(churn_cmds)) => {
            let (name, provider) = match &churn_cmds {
                ChurnCommands::FixedInterval { name, provider, .. } => (name, provider),
                ChurnCommands::RandomInterval { name, provider, .. } => (name, provider),
            };
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_forks(1)
                .environment_name(name)
                .provider(*provider)
                .build()?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(name, true, None)
                .await?;

            match churn_cmds {
                ChurnCommands::FixedInterval {
                    churn_cycles,
                    concurrent_churns,
                    interval,
                    retain_peer_id,
                    ..
                } => {
                    cmd::network::handle_fixed_interval_network_churn(
                        inventory,
                        interval,
                        concurrent_churns,
                        retain_peer_id,
                        churn_cycles,
                    )
                    .await?;
                }
                ChurnCommands::RandomInterval {
                    churn_count,
                    churn_cycles,
                    retain_peer_id,
                    time_frame,
                    ..
                } => {
                    cmd::network::handle_random_interval_network_churn(
                        inventory,
                        time_frame,
                        churn_count,
                        retain_peer_id,
                        churn_cycles,
                    )
                    .await?;
                }
            }
            Ok(())
        }
        Commands::Network(NetworkCommands::UpdateNodeLogLevel {
            concurrent_updates,
            log_level,
            name,
        }) => {
            cmd::network::handle_update_node_log_level(concurrent_updates, log_level, name).await?;
            Ok(())
        }
        Commands::Notify { name } => {
            cmd::misc::handle_notify(name).await?;
            Ok(())
        }
        Commands::Setup {} => {
            setup_dotenv_file()?;
            Ok(())
        }
        Commands::Start {
            custom_inventory,
            forks,
            interval,
            name,
            node_type,
            provider,
        } => {
            nodes::handle_start_command(
                custom_inventory,
                forks,
                interval,
                name,
                node_type,
                provider,
            )
            .await?;
            Ok(())
        }
        Commands::Status {
            forks,
            name,
            provider,
        } => {
            nodes::handle_status_command(forks, name, provider).await?;
            Ok(())
        }
        Commands::Stop {
            custom_inventory,
            delay,
            forks,
            interval,
            name,
            node_type,
            provider,
            service_name,
        } => {
            nodes::handle_stop_command(
                custom_inventory,
                delay,
                forks,
                interval,
                name,
                node_type,
                provider,
                service_name,
            )
            .await?;
            Ok(())
        }
        Commands::ConfigureSwapfile {
            name,
            provider,
            peer_cache,
            size,
        } => {
            cmd::misc::handle_configure_swapfile(name, provider, peer_cache, size).await?;
            Ok(())
        }
        Commands::Upgrade {
            ansible_verbose,
            custom_inventory,
            force,
            forks,
            interval,
            name,
            node_env_variables,
            node_type,
            provider,
            pre_upgrade_delay,
            version,
        } => {
            cmd::upgrade::handle_upgrade_command(
                ansible_verbose,
                custom_inventory,
                node_env_variables,
                force,
                forks,
                interval,
                name,
                node_type,
                provider,
                pre_upgrade_delay,
                version,
            )
            .await?;
            Ok(())
        }
        Commands::UpgradeAntctl {
            custom_inventory,
            name,
            node_type,
            provider,
            version,
        } => {
            cmd::upgrade::handle_upgrade_antctl_command(
                custom_inventory,
                name,
                node_type,
                provider,
                version,
            )
            .await?;
            Ok(())
        }
        Commands::Upscale {
            ansible_verbose,
            ant_version,
            antctl_version,
            antnode_version,
            branch,
            desired_client_vm_count,
            desired_node_count,
            desired_full_cone_private_node_count,
            desired_full_cone_private_node_vm_count,
            desired_node_vm_count,
            desired_peer_cache_node_count,
            desired_peer_cache_node_vm_count,
            desired_symmetric_private_node_count,
            desired_symmetric_private_node_vm_count,
            desired_uploaders_count,
            disable_download_verifier,
            disable_performance_verifier,
            disable_random_verifier,
            funding_wallet_secret_key,
            infra_only,
            interval,
            max_archived_log_files,
            max_log_files,
            name,
            network_dashboard_branch,
            node_env_variables,
            plan,
            provider,
            public_rpc,
            repo_owner,
            skip_binary_build,
        } => {
            cmd::deployments::handle_upscale(
                ansible_verbose,
                ant_version,
                antctl_version,
                antnode_version,
                branch,
                desired_client_vm_count,
                desired_node_count,
                desired_full_cone_private_node_count,
                desired_full_cone_private_node_vm_count,
                desired_node_vm_count,
                desired_peer_cache_node_count,
                desired_peer_cache_node_vm_count,
                desired_symmetric_private_node_count,
                desired_symmetric_private_node_vm_count,
                desired_uploaders_count,
                disable_download_verifier,
                disable_performance_verifier,
                disable_random_verifier,
                funding_wallet_secret_key,
                infra_only,
                interval,
                max_archived_log_files,
                max_log_files,
                name,
                network_dashboard_branch,
                node_env_variables,
                plan,
                provider,
                public_rpc,
                repo_owner,
                skip_binary_build,
            )
            .await?;
            Ok(())
        }
        Commands::UpdatePeer {
            custom_inventory,
            name,
            node_type,
            peer,
            provider,
        } => {
            nodes::handle_update_peer_command(custom_inventory, name, node_type, peer, provider)
                .await?;
            Ok(())
        }
        Commands::ResetToNNodes {
            custom_inventory,
            evm_network_type,
            forks,
            name,
            node_count,
            node_type,
            provider,
            start_interval,
            stop_interval,
            version,
        } => {
            nodes::handle_reset_to_n_nodes_command(
                custom_inventory,
                evm_network_type,
                forks,
                name,
                node_count,
                node_type,
                provider,
                start_interval,
                stop_interval,
                version,
            )
            .await?;
            Ok(())
        }
        Commands::Provision(provision_cmd) => match provision_cmd {
            ProvisionCommands::FullConePrivateNodes { name } => {
                cmd::provision::handle_provision_full_cone_private_nodes(name).await?;
                Ok(())
            }
            ProvisionCommands::PeerCacheNodes { name } => {
                cmd::provision::handle_provision_peer_cache_nodes(name).await?;
                Ok(())
            }
            ProvisionCommands::GenericNodes { name } => {
                cmd::provision::handle_provision_generic_nodes(name).await?;
                Ok(())
            }
            ProvisionCommands::SymmetricPrivateNodes { name } => {
                cmd::provision::handle_provision_symmetric_private_nodes(name).await?;
                Ok(())
            }
            ProvisionCommands::Clients { name } => {
                cmd::provision::handle_provision_clients(name).await?;
                Ok(())
            }
        },
        Commands::Telegraf(telegraf_cmd) => match telegraf_cmd {
            TelegrafCommands::Start {
                custom_inventory,
                forks,
                name,
                node_type,
                provider,
            } => {
                cmd::telegraf::handle_start_telegraf_command(
                    custom_inventory,
                    forks,
                    name,
                    node_type,
                    provider,
                )
                .await
            }
            TelegrafCommands::Stop {
                custom_inventory,
                forks,
                name,
                node_type,
                provider,
            } => {
                cmd::telegraf::handle_stop_telegraf_command(
                    custom_inventory,
                    forks,
                    name,
                    node_type,
                    provider,
                )
                .await
            }
            TelegrafCommands::UpgradeClientConfig {
                forks,
                name,
                provider,
            } => cmd::telegraf::handle_upgrade_client_telegraf_config(forks, name, provider).await,
            TelegrafCommands::UpgradeGeoIpConfig {
                forks,
                name,
                provider,
            } => cmd::telegraf::handle_upgrade_geoip_telegraf_config(forks, name, provider).await,
            TelegrafCommands::UpgradeNodeConfig {
                forks,
                name,
                provider,
            } => cmd::telegraf::handle_upgrade_node_telegraf_config(forks, name, provider).await,
        },
    }
}
