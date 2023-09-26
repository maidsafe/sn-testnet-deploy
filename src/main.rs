// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use clap::{Parser, Subcommand};
use color_eyre::{eyre::eyre, Help, Result};
use dotenv::dotenv;
use rand::Rng;
use sn_testnet_deploy::error::Error;
use sn_testnet_deploy::logstash::LogstashDeployBuilder;
use sn_testnet_deploy::manage_test_data::TestDataClientBuilder;
use sn_testnet_deploy::setup::setup_dotenv_file;
use sn_testnet_deploy::{
    get_data_directory, get_wallet_directory, notify_slack, CloudProvider, DeploymentInventory,
    TestnetDeployBuilder,
};

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
        /// The name of the Logstash stack to forward logs to.
        #[clap(long, default_value = "main")]
        logstash_stack_name: String,
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
    Inventory {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Optionally supply the name of the custom branch that was used when creating the
        /// testnet.
        ///
        /// You can supply this if you are running the command on a different machine from where
        /// the testnet was deployed. It will then get written to the cached inventory on the local
        /// machine. This information gets used with the upload-test-data command for retrieving
        /// the safe client that was built when the testnet was deployed.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        #[arg(long)]
        branch: Option<String>,
        /// Optionally supply the repo owner of the custom branch that was used when creating the
        /// testnet.
        ///
        /// You can supply this if you are running the command on a different machine from where
        /// the testnet was deployed. It will then get written to the cached inventory on the local
        /// machine. This information gets used with the upload-test-data command for retrieving
        /// the safe client that was built when the testnet was deployed.
        ///
        /// This argument must be used in conjunction with the --branch argument.
        #[arg(long)]
        repo_owner: Option<String>,
        /// Optionally supply the node count.
        ///
        /// You can supply this if you are running the command on a different machine from where
        /// the testnet was deployed. It will then get written to the cached inventory on the local
        /// machine. This information gets used for Slack notifications.
        #[arg(long)]
        node_count: Option<u16>,
    },
    #[clap(name = "logs", subcommand)]
    Logs(LogCommands),
    #[clap(name = "logstash", subcommand)]
    Logstash(LogstashCommands),
    /// Send a notification to Slack with testnet inventory details
    Notify {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
    },
    Setup {},
    /// Run a smoke test against a given network.
    #[clap(name = "smoke-test")]
    SmokeTest {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
    /// Clean a deployed testnet environment.
    #[clap(name = "upload-test-data")]
    UploadTestData {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum LogCommands {
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
    /// Remove the logs from a given environment from the bucket on S3.
    Rm {
        /// The name of the environment for which logs have already been retrieved
        #[arg(short = 'n', long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum LogstashCommands {
    /// Clean a deployed Logstash environment.
    Clean {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Deploy the Logstash infrastructure to support log forwarding to S3.
    Deploy {
        /// The name of the Logstash environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider to provision on
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The number of VMs to create.
        ///
        /// Use this to horizontally scale Logstash if need be.
        #[clap(long, default_value = "1")]
        vm_count: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    dotenv().ok();
    env_logger::init();

    let opt = Opt::parse();
    match opt.command {
        Some(Commands::Clean { name, provider }) => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.clean(&name).await?;
            Ok(())
        }
        Some(Commands::Deploy {
            branch,
            logstash_stack_name,
            name,
            node_count,
            provider,
            repo_owner,
            vm_count,
        }) => {
            if (repo_owner.is_some() && branch.is_none())
                || (branch.is_some() && repo_owner.is_none())
            {
                return Err(eyre!(
                    "Both the repository owner and branch name must be supplied if either are used"
                ));
            }
            let custom_branch_details = repo_owner.map(|repo_owner| (repo_owner, branch.unwrap()));

            let testnet_deploy = TestnetDeployBuilder::default()
                .provider(provider.clone())
                .build()?;
            let result = testnet_deploy.init(&name).await;
            match result {
                Ok(_) => {}
                Err(e) => match e {
                    Error::LogsForPreviousTestnetExist(_) => {
                        return Err(eyre!(e)
                            .wrap_err(format!(
                                "Logs already exist for a previous testnet with the \
                                    name '{name}'"
                            ))
                            .suggestion(
                                "If you wish to keep them, retrieve the logs with the 'logs get' \
                                command, then remove them with 'logs rm'. If you don't need them, \
                                simply run 'logs rm'. Then you can proceed with deploying your \
                                new testnet.",
                            ));
                    }
                    _ => {
                        return Err(eyre!(e));
                    }
                },
            }

            let logstash_deploy = LogstashDeployBuilder::default()
                .provider(provider)
                .build()?;
            let stack_hosts = logstash_deploy
                .get_stack_hosts(&logstash_stack_name)
                .await?;
            testnet_deploy
                .deploy(
                    &name,
                    (&logstash_stack_name, &stack_hosts),
                    vm_count,
                    node_count,
                    custom_branch_details,
                )
                .await?;
            Ok(())
        }
        Some(Commands::Inventory {
            name,
            provider,
            branch,
            repo_owner,
            node_count,
        }) => {
            if (repo_owner.is_some() && branch.is_none())
                || (branch.is_some() && repo_owner.is_none())
            {
                return Err(eyre!(
                    "Both the repository owner and branch name must be supplied if either are used"
                ));
            }

            let custom_branch_details = repo_owner.map(|repo_owner| (repo_owner, branch.unwrap()));
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy
                .list_inventory(&name, false, custom_branch_details, node_count)
                .await?;
            Ok(())
        }
        Some(Commands::Logs(log_cmd)) => match log_cmd {
            LogCommands::Copy { name, provider } => {
                let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
                testnet_deploy.init(&name).await?;
                testnet_deploy.copy_logs(&name).await?;
                Ok(())
            }
            LogCommands::Get { name } => {
                sn_testnet_deploy::logs::get_logs(&name).await?;
                Ok(())
            }
            LogCommands::Reassemble { name } => {
                sn_testnet_deploy::logs::reassemble_logs(&name).await?;
                Ok(())
            }
            LogCommands::Rm { name } => {
                sn_testnet_deploy::logs::rm_logs(&name).await?;
                Ok(())
            }
        },
        Some(Commands::Logstash(logstash_cmd)) => match logstash_cmd {
            LogstashCommands::Clean { name, provider } => {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .provider(provider)
                    .build()?;
                logstash_deploy.clean(&name).await?;
                Ok(())
            }
            LogstashCommands::Deploy {
                name,
                provider,
                vm_count,
            } => {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .provider(provider)
                    .build()?;
                logstash_deploy.init(&name).await?;
                logstash_deploy.deploy(&name, vm_count).await?;
                Ok(())
            }
        },
        Some(Commands::Notify { name }) => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;
            notify_slack(inventory).await?;
            Ok(())
        }
        Some(Commands::Setup {}) => {
            setup_dotenv_file()?;
            Ok(())
        }
        Some(Commands::SmokeTest { name }) => {
            let wallet_dir_path = get_wallet_directory()?;
            if wallet_dir_path.exists() {
                return Err(eyre!(
                    "A previous wallet directory exists. The smoke test is intended \
                    to be for a new network."
                )
                .suggestion("Please remove your previous wallet directory and try again"));
            }

            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let mut inventory = DeploymentInventory::read(&inventory_path)?;
            let test_data_client = TestDataClientBuilder::default().build()?;
            test_data_client.smoke_test(&mut inventory).await?;
            inventory.save(&inventory_path)?;
            Ok(())
        }
        Some(Commands::UploadTestData { name }) => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let mut inventory = DeploymentInventory::read(&inventory_path)?;
            let (first_machine_name, ip_address) = inventory.vm_list.first().ok_or_else(|| {
                eyre!("There are no VMs in the inventory. Please deploy a testnet first")
            })?;

            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;

            // TODO:
            // Ensure gen node has `safe` client
            // get the test data from somewhere
            let output = testnet_deploy.ssh_client.run_script(
                &ip_address,
                "safe",
                PathBuf::from("scripts").join("upload_test_data.sh"),
                false,
            )?;

            // Finally, parse the output to get the uploaded files
            
            
            // println!("Uploaded files:");
            // for (path, address) in uploaded_files.iter() {
            //     println!("{path}: {address}");
            // }
            inventory.add_uploaded_files(uploaded_files.clone());
            inventory.save(&inventory_path)?;

            Ok(())
        }
        None => Ok(()),
    }
}
