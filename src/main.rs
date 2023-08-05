// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use clap::{Parser, Subcommand};
use color_eyre::{eyre::eyre, Result};
use dotenv::dotenv;
use sn_testnet_deploy::setup::setup_dotenv_file;
use sn_testnet_deploy::CloudProvider;
use sn_testnet_deploy::TestnetDeployBuilder;

pub fn parse_provider(val: &str) -> Result<CloudProvider> {
    match val {
        "aws" => Ok(CloudProvider::Aws),
        "digital-ocean" => Ok(CloudProvider::DigitalOcean),
        _ => Err(eyre!(
            "The only supported providers are 'aws' or 'digital-ocean'"
        )),
    }
}

#[derive(Parser, Debug)]
#[clap(name = "sn-testnet-deploy", version = env!("CARGO_PKG_VERSION"))]
struct Opt {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Clean a deployed testnet environment.
    Clean {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Deploy a new testnet environment using the latest version of the safenode binary.
    Deploy {
        /// Optionally supply the name of a branch on the Github repository to be used for the
        /// safenode binary. A safenode binary will be built from this repository.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        #[arg(long)]
        branch: Option<String>,
        /// Optionally supply the owner or organisation of the Github repository to be used for the
        /// safenode binary. A safenode binary will be built from this repository.
        ///
        /// This argument must be used in conjunction with the --branch argument.
        #[arg(long)]
        repo_owner: Option<String>,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The number of safenode processes to run on each VM.
        #[clap(long, default_value = "20")]
        node_count: u16,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode processes.
        #[clap(long, default_value = "10")]
        vm_count: u16,
    },
    Setup {},
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let opt = Opt::parse();
    match opt.command {
        Some(Commands::Clean { name, provider }) => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.clean(&name).await?;
            Ok(())
        }
        Some(Commands::Deploy {
            branch,
            name,
            node_count,
            provider,
            repo_owner,
            vm_count,
        }) => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.init(&name).await?;
            testnet_deploy
                .deploy(&name, vm_count, node_count, repo_owner, branch)
                .await?;
            Ok(())
        }
        Some(Commands::Setup {}) => {
            setup_dotenv_file()?;
            Ok(())
        }
        None => Ok(()),
    }
}
