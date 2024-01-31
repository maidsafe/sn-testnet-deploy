// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{bail, eyre},
    Help, Result,
};
use dotenv::dotenv;
use rand::Rng;
use sn_testnet_deploy::{
    deploy::DeployCmd, error::Error, get_data_directory, get_wallet_directory,
    logstash::LogstashDeployBuilder, manage_test_data::TestDataClientBuilder, notify_slack,
    setup::setup_dotenv_file, CloudProvider, DeploymentInventory, SnCodebaseType,
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
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Deploy a new testnet environment using the latest version of the safenode binary.
    Deploy {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The number of safenode processes to run on each VM.
        #[clap(long, default_value = "40")]
        node_count: u16,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode processes.
        #[clap(long, default_value = "10")]
        vm_count: u16,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// The name of the Logstash stack to forward logs to.
        #[clap(long, default_value = "main")]
        logstash_stack_name: String,
        /// The features to enable on the safenode binary.
        ///
        /// If not provided, the default feature set specified for the safenode binary are used.
        #[clap(long)]
        safenode_features: Option<Vec<String>>,
        /// Optionally supply the name of a branch on the Github repository to be used for the
        /// safenode binary. A safenode binary will be built from this repository.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the --safe-version
        /// and --safenode-version arguments. You can only supply version numbers or a custom
        /// branch, not both.
        #[arg(long)]
        branch: Option<String>,
        /// Optionally supply the owner or organisation of the Github repository to be used for the
        /// safenode binary. A safenode binary will be built from this repository.
        ///
        /// This argument must be used in conjunction with the --branch argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the --safe-version
        /// and --safenode-version arguments. You can only supply version numbers or a custom
        /// branch, not both.
        #[arg(long)]
        repo_owner: Option<String>,
        /// Optionally supply a version number to be used for the safe binary. There should be no
        /// 'v' prefix.
        ///
        /// This argument must be used in conjunction with the --safenode-version argument.
        ///
        /// The --safe-version and --safenode-version arguments are mutually exclusive with the
        /// --branch and --repo-owner arguments. You can only supply version numbers or a custom
        /// branch, not both.
        #[arg(long)]
        safe_version: Option<String>,
        #[arg(long)]
        /// Optionally supply a version number to be used for the safenode binary. There should be
        /// no 'v' prefix.
        ///
        /// This argument must be used in conjunction with the --safe-version argument.
        ///
        /// The --safe-version and --safenode-version arguments are mutually exclusive with the
        /// --branch and --repo-owner arguments. You can only supply version numbers or a custom
        /// branch, not both.
        safenode_version: Option<String>,
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
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
    /// Rsync the logs from all the VMs for a given environment.
    /// Rerunning the same command will sync only the changed log files without copying everything from the beginning.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Rsync {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Should we copy the resource-usage.logs only
        #[arg(short = 'r', long)]
        resources_only: bool,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
    /// Run a ripgrep query through all the logs from all the VMs and copy the results.
    ///
    /// The results will be written to `logs/<name>/<vm>/rg-timestamp.log`
    Rg {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The ripgrep arguments that are directly passed to ripgrep. The text to search for should be put inside
        /// single quotes. The dir to search for is set automatically, so do not provide one.
        ///
        /// Example command: `cargo run --release -- logs rg --name <name> --args "'ValidSpendRecordPutFromNetwork' -c"`
        #[arg(short = 'a', long, allow_hyphen_values(true))]
        args: String,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },

    /// Retrieve the logs for a given environment by copying them from all the VMs.
    ///
    /// This will write the logs to 'logs/<name>', relative to the current directory.
    Copy {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Should we copy the resource-usage.logs only
        #[arg(short = 'r', long)]
        resources_only: bool,
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
            name,
            node_count,
            vm_count,
            provider,
            safenode_features,
            branch,
            repo_owner,
            logstash_stack_name,
            safe_version,
            safenode_version,
            ansible_verbose,
        }) => {
            let sn_codebase_type = get_sn_codebase_type(
                branch,
                repo_owner,
                safe_version,
                safenode_version,
                safenode_features,
            )
            .await?;

            let testnet_deploy = TestnetDeployBuilder::default()
                .ansible_verbose_mode(ansible_verbose)
                .provider(provider.clone())
                .build()?;

            match testnet_deploy.init(&name).await {
                Ok(_) => {}
                Err(e @ Error::LogsForPreviousTestnetExist(_)) => {
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
                Err(e) => {
                    return Err(eyre!(e));
                }
            }

            let logstash_deploy = LogstashDeployBuilder::default()
                .provider(provider)
                .build()?;
            let stack_hosts = logstash_deploy
                .get_stack_hosts(&logstash_stack_name)
                .await?;
            let deploy_cmd = DeployCmd::new(
                testnet_deploy,
                name,
                node_count,
                vm_count,
                (logstash_stack_name, stack_hosts),
                sn_codebase_type,
            );
            deploy_cmd.execute().await?;
            Ok(())
        }
        Some(Commands::Inventory {
            name,
            provider,
            branch,
            repo_owner,
            node_count,
        }) => {
            let sn_codebase_type =
                get_sn_codebase_type(branch, repo_owner, None, None, None).await?;

            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy
                .list_inventory(&name, false, sn_codebase_type, node_count)
                .await?;
            Ok(())
        }
        Some(Commands::Logs(log_cmd)) => match log_cmd {
            LogCommands::Rsync {
                name,
                resources_only,
                provider,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
                testnet_deploy.init(&name).await?;
                testnet_deploy.rsync_logs(&name, resources_only).await?;
                Ok(())
            }
            LogCommands::Rg {
                name,
                provider,
                args,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
                testnet_deploy.init(&name).await?;
                testnet_deploy.ripgrep_logs(&name, &args).await?;
                Ok(())
            }
            LogCommands::Copy {
                name,
                provider,
                resources_only,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
                testnet_deploy.init(&name).await?;
                testnet_deploy.copy_logs(&name, resources_only).await?;
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
            let mut rng = rand::thread_rng();
            let i = rng.gen_range(0..inventory.peers.len());
            let random_peer = &inventory.peers[i];

            let test_data_client = TestDataClientBuilder::default().build()?;
            let uploaded_files = test_data_client
                .upload_test_data(&name, random_peer, &inventory.sn_codebase_type)
                .await?;

            println!("Uploaded files:");
            for (path, address) in uploaded_files.iter() {
                println!("{path}: {address}");
            }
            inventory.add_uploaded_files(uploaded_files.clone());
            inventory.save(&inventory_path)?;

            Ok(())
        }
        None => Ok(()),
    }
}

// Validate the branch and version args along with the feature list
#[allow(clippy::type_complexity)]
async fn get_sn_codebase_type(
    branch: Option<String>,
    repo_owner: Option<String>,
    safe_version: Option<String>,
    safenode_version: Option<String>,
    safenode_features: Option<Vec<String>>,
) -> Result<SnCodebaseType> {
    if let (Some(_), None) | (None, Some(_)) = (&repo_owner, &branch) {
        return Err(eyre!(
            "Both 'repository owner' and 'branch name' must be supplied together."
        ));
    }
    if let (Some(_), None) | (None, Some(_)) = (&safe_version, &safenode_version) {
        return Err(eyre!(
            "Both 'safe' and 'safenode' versions must be supplied together."
        ));
    }
    if safe_version.is_some() && safenode_features.is_some() {
        return Err(eyre!(
            "Cannot enable custom safenode features if the 'safe, safenode' versions are provided."
        ));
    }

    if branch.is_some()
        && repo_owner.is_some()
        && safe_version.is_some()
        && safenode_version.is_some()
    {
        return Err(eyre!(
            "Custom version numbers and custom branches cannot be supplied at the same time"
        )
        .suggestion(
            "Please choose whether you want to use specific versions or a custom \
                    branch, then run again.",
        ));
    }

    // get the CSV features list
    let safenode_features = safenode_features.map(|list| list.join(","));

    let codebase_type = if let (Some(repo_owner), Some(branch)) = (repo_owner, branch) {
        // check if the custom branch exists.

        let url = format!("https://github.com/{repo_owner}/safe_network/tree/{branch}",);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            bail!("The provided branch or owner does not exists: {url:?}");
        }
        SnCodebaseType::Branch {
            repo_owner,
            branch,
            safenode_features,
        }
    } else if let (Some(safe_version), Some(safenode_version)) = (safe_version, safenode_version) {
        SnCodebaseType::Versioned {
            safe_version,
            safenode_version,
        }
    } else {
        SnCodebaseType::Main { safenode_features }
    };

    Ok(codebase_type)
}
