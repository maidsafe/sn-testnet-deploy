// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use super::*;

use clap::Subcommand;
use color_eyre::Result;
use sn_testnet_deploy::{
    inventory::DeploymentInventoryService, CloudProvider, TestnetDeployBuilder,
};

#[derive(Subcommand, Debug)]
pub enum LogCommands {
    /// Removes all the rotated log files from the the node VMs.
    Cleanup {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Setup a cron job to perform the cleanup periodically.
        #[clap(long)]
        setup_cron: bool,
    },
    /// Retrieve the logs for a given environment by copying them from all the VMs.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Copy {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Should we copy the resource-usage.logs only
        #[arg(short = 'r', long)]
        resources_only: bool,
    },
    /// Retrieve the logs for a given environment from S3.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Get {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Reassemble retrieved logs from their parts.
    ///
    /// The logs must have already been retrieved using the 'get' command and be present at
    /// 'logs/<name>'.
    ///
    /// This will write the logs to 'logs/<name>-reassembled', relative to the current directory.
    ///
    /// The original logs are left intact so you can sync again if need be.
    Reassemble {
        /// The name of the environment for which logs have already been retrieved
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Run a ripgrep query through all the logs from all the VMs and copy the results.
    ///
    /// The results will be written to `logs/<name>/<vm>/rg-timestamp.log`
    Rg {
        /// The ripgrep arguments that are directly passed to ripgrep. The text to search for should be put inside
        /// single quotes. The dir to search for is set automatically, so do not provide one.
        ///
        /// Example command: `cargo run --release -- logs rg --name <name> --args "'ValidSpendRecordPutFromNetwork' -z -a"`
        #[arg(short = 'a', long, allow_hyphen_values(true))]
        args: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Remove the logs from a given environment from the bucket on S3.
    Rm {
        /// The name of the environment for which logs have already been retrieved
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Rsync the logs from all the VMs for a given environment.
    /// Rerunning the same command will sync only the changed log files without copying everything from the beginning.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Rsync {
        /// Do not sync the client logs.
        #[arg(long, default_value = "false")]
        disable_client_logs: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Optionally only sync the logs for the VMs that contain the following string.
        #[arg(long)]
        vm_filter: Option<String>,
    },
}

pub async fn handle_logs_command(log_cmd: LogCommands) -> Result<()> {
    match log_cmd {
        LogCommands::Cleanup {
            name,
            provider,
            setup_cron,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service.setup_environment_inventory(&name)?;

            testnet_deployer.cleanup_node_logs(setup_cron)?;
            Ok(())
        }
        LogCommands::Copy {
            name,
            provider,
            resources_only,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service.setup_environment_inventory(&name)?;

            testnet_deployer.copy_logs(&name, resources_only)?;
            Ok(())
        }
        LogCommands::Get { name } => {
            sn_testnet_deploy::logs::get_logs(&name).await?;
            Ok(())
        }
        LogCommands::Reassemble { name } => {
            sn_testnet_deploy::logs::reassemble_logs(&name)?;
            Ok(())
        }
        LogCommands::Rg {
            args,
            name,
            provider,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service.setup_environment_inventory(&name)?;

            testnet_deployer.ripgrep_logs(&name, &args)?;
            Ok(())
        }
        LogCommands::Rm { name } => {
            sn_testnet_deploy::logs::rm_logs(&name).await?;
            Ok(())
        }
        LogCommands::Rsync {
            disable_client_logs,
            name,
            provider,
            vm_filter,
        } => {
            let testnet_deployer = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deployer.init().await?;
            let inventory_service = DeploymentInventoryService::from(&testnet_deployer);
            inventory_service.setup_environment_inventory(&name)?;

            testnet_deployer.rsync_logs(&name, vm_filter, disable_client_logs)?;
            Ok(())
        }
    }
}
