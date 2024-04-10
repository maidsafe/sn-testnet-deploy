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
use semver::Version;
use sn_releases::{ReleaseType, SafeReleaseRepoActions};
use sn_testnet_deploy::{
    deploy::DeployCmd, error::Error, get_data_directory, get_wallet_directory,
    logstash::LogstashDeployBuilder, manage_test_data::TestDataClientBuilder, network_commands,
    notify_slack, setup::setup_dotenv_file, CloudProvider, DeploymentInventory, SnCodebaseType,
    TestnetDeployBuilder, UpgradeOptions,
};
use std::time::Duration;

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
    command: Commands,
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
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The branch of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this branch. It is typically used for testing
        /// changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        branch: Option<String>,
        /// Provide environment variables for the safenode service.
        ///
        /// This is useful to set the safenode's log levels. Each variable should be comma separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Supply a version number to be used for the faucet binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner arguments.
        /// You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        faucet_version: Option<String>,
        /// The name of the Logstash stack to forward logs to.
        #[clap(long, default_value = "main")]
        logstash_stack_name: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The number of safenode processes to run on each VM.
        #[clap(long, default_value = "40")]
        node_count: u16,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// If set to true, the RPC of the node will be accessible remotely.
        ///
        /// By default, the safenode RPC is only accessible via the 'localhost' and is not exposed for security reasons.
        #[clap(long, default_value = "false")]
        public_rpc: bool,
        /// The owner/org of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this repository. It is typically used for
        /// testing changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        repo_owner: Option<String>,
        /// Supply a version number for the safe binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner arguments.
        /// You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        safe_version: Option<String>,
        /// The features to enable on the safenode binary.
        ///
        /// If not provided, the default feature set specified for the safenode binary are used.
        #[clap(long)]
        safenode_features: Option<Vec<String>>,
        /// Supply a version number for the safenode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        safenode_version: Option<String>,
        /// Supply a version number for the safenode-manager binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long)]
        safenode_manager_version: Option<String>,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode processes.
        #[clap(long, default_value = "10")]
        vm_count: u16,
    },
    Inventory {
        /// Supply the name of the branch used when creating the testnet.
        ///
        /// You can supply this if you are running the command on a different machine from where
        /// the testnet was deployed. It will then get written to the cached inventory on the local
        /// machine. This information gets used with the upload-test-data command for retrieving
        /// the safe client that was built when the testnet was deployed.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        #[arg(long)]
        branch: Option<String>,
        /// If set to true, the inventory will be regenerated.
        ///
        /// This is useful if the testnet was created on another machine.
        #[clap(long, default_value = "false")]
        force_regeneration: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Optionally supply the node count.
        ///
        /// You can supply this if you are running the command on a different machine from where
        /// the testnet was deployed. It will then get written to the cached inventory on the local
        /// machine. This information gets used for Slack notifications.
        #[arg(long)]
        node_count: Option<u16>,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
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
    },
    #[clap(name = "logs", subcommand)]
    Logs(LogCommands),
    #[clap(name = "logstash", subcommand)]
    Logstash(LogstashCommands),
    #[clap(name = "network", subcommand)]
    Network(NetworkCommands),
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
    /// Start all nodes in an environment.
    ///
    /// This can be useful if all nodes did not upgrade successfully.
    Start {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider for the environment.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
    },
    /// Upgrade the node binaries of a testnet environment to the latest version.
    Upgrade {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// Provide environment variables for the safenode service.
        ///
        /// These will override the values provided initially.
        ///
        /// This is useful to set safenode's log levels. Each variable should be comma separated
        /// without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Optionally supply a version for the faucet binary to be upgraded to.
        ///
        /// If not provided, the latest version will be used. A lower version number can be
        /// specified to downgrade to a known good version.
        ///
        /// There should be no 'v' prefix.
        #[arg(long)]
        faucet_version: Option<String>,
        /// Set to force the node manager to accept the faucet version provided.
        ///
        /// This can be used to downgrade the faucet to a known good version.
        #[clap(long)]
        force_faucet: bool,
        /// Set to force the node manager to accept the safenode version provided.
        ///
        /// This can be used to downgrade safenode to a known good version.
        #[clap(long)]
        force_safenode: bool,
        /// Maximum number of forks Ansible will use to execute tasks on target hosts.
        #[clap(long, default_value_t = 2)]
        forks: usize,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        #[arg(long)]
        /// Optionally supply a version for the faucet binary to be upgraded to.
        ///
        /// If not provided, the latest version will be used. A lower version number can be
        /// specified to downgrade to a known good version.
        ///
        /// There should be no 'v' prefix.
        safenode_version: Option<String>,
    },
    /// Upgrade the safenode-manager binaries to a particular version.
    ///
    /// Simple mechanism that simply copies over the existing binary.
    #[clap(name = "upgrade-node-manager")]
    UpgradeNodeManager {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        #[arg(long)]
        /// The cloud provider of the environment.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// Supply a version for the binary to be upgraded to.
        ///
        /// There should be no 'v' prefix.
        /// The name of the environment
        #[arg(short = 'v', long)]
        version: String,
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
        /// Example command: `cargo run --release -- logs rg --name <name> --args "'ValidSpendRecordPutFromNetwork' -c"`
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

// Administer or perform activities on a deployed network.
#[derive(Subcommand, Debug)]
enum NetworkCommands {
    /// Restart nodes in the testnet to simulate the churn of nodes.
    #[clap(name = "churn", subcommand)]
    ChurnCommands(ChurnCommands),
    /// Modifies the log levels for all the safenode services through RPC requests.
    UpdateNodeLogLevel {
        /// The number of nodes to update concurrently.
        #[clap(long, short = 'c', default_value_t = 10)]
        concurrent_updates: usize,
        /// The log level to set.
        ///
        /// Example: --log-level SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(long)]
        log_level: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum ChurnCommands {
    /// Churn nodes at fixed intervals.
    FixedInterval {
        /// The number of time each node in the network is restarted.
        #[clap(long, default_value_t = 1)]
        churn_cycles: usize,
        /// The number of nodes to restart concurrently per VM.
        #[clap(long, short = 'c', default_value_t = 2)]
        concurrent_churns: usize,
        /// The interval between each node churn.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?)}, default_value = "60")]
        interval: Duration,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Whether to retain the same PeerId on restart.
        #[clap(long, default_value_t = false)]
        retain_peer_id: bool,
    },
    /// Churn nodes at random intervals.
    RandomInterval {
        /// Number of nodes to restart in the given time frame.
        #[clap(long, default_value_t = 10)]
        churn_count: usize,
        /// The number of time each node in the network is restarted.
        #[clap(long, default_value_t = 1)]
        churn_cycles: usize,
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Whether to retain the same PeerId on restart.
        #[clap(long, default_value_t = false)]
        retain_peer_id: bool,
        /// The time frame in which the churn_count nodes are restarted.
        /// Nodes are restarted at a rate of churn_count/time_frame with random delays between each restart.
        #[clap(long, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?)}, default_value = "600")]
        time_frame: Duration,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    dotenv().ok();
    env_logger::init();

    let opt = Opt::parse();
    match opt.command {
        Commands::Clean { name, provider } => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.clean(&name).await?;
            Ok(())
        }
        Commands::Deploy {
            ansible_verbose,
            branch,
            env_variables,
            faucet_version,
            logstash_stack_name,
            name,
            node_count,
            provider,
            public_rpc,
            repo_owner,
            safe_version,
            safenode_features,
            safenode_version,
            safenode_manager_version,
            vm_count,
        } => {
            let sn_codebase_type = get_sn_codebase_type(
                branch,
                repo_owner,
                safe_version,
                safenode_version,
                safenode_manager_version,
                faucet_version,
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

            let logstash_details = {
                let logstash_deploy = LogstashDeployBuilder::default()
                    .provider(provider)
                    .build()?;
                let stack_hosts = logstash_deploy
                    .get_stack_hosts(&logstash_stack_name)
                    .await?;
                if stack_hosts.is_empty() {
                    None
                } else {
                    Some((logstash_stack_name, stack_hosts))
                }
            };
            let deploy_cmd = DeployCmd::new(
                testnet_deploy,
                name,
                node_count,
                vm_count,
                public_rpc,
                logstash_details,
                sn_codebase_type,
                env_variables,
            );
            deploy_cmd.execute().await?;
            Ok(())
        }
        Commands::Inventory {
            branch,
            force_regeneration,
            name,
            provider,
            node_count,
            repo_owner,
        } => {
            let sn_codebase_type =
                get_sn_codebase_type(branch, repo_owner, None, None, None, None, None).await?;

            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy
                .list_inventory(&name, force_regeneration, sn_codebase_type, node_count)
                .await?;
            Ok(())
        }
        Commands::Logs(log_cmd) => match log_cmd {
            LogCommands::Rsync {
                name,
                provider,
                resources_only,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
                testnet_deploy.init(&name).await?;
                testnet_deploy.rsync_logs(&name, resources_only).await?;
                Ok(())
            }
            LogCommands::Rg {
                args,
                name,
                provider,
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
        Commands::Logstash(logstash_cmd) => match logstash_cmd {
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
        Commands::Network(NetworkCommands::ChurnCommands(churn_cmds)) => {
            let name = match &churn_cmds {
                ChurnCommands::FixedInterval { name, .. } => name,
                ChurnCommands::RandomInterval { name, .. } => name,
            };
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;

            match churn_cmds {
                ChurnCommands::FixedInterval {
                    churn_cycles,
                    concurrent_churns,
                    interval,
                    name: _,
                    retain_peer_id,
                } => {
                    network_commands::perform_fixed_interval_network_churn(
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
                    name: _,
                    retain_peer_id,
                    time_frame,
                } => {
                    network_commands::perform_random_interval_network_churn(
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
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;
            network_commands::update_node_log_levels(inventory, log_level, concurrent_updates)
                .await?;

            Ok(())
        }
        Commands::Notify { name } => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let inventory = DeploymentInventory::read(&inventory_path)?;
            notify_slack(inventory).await?;
            Ok(())
        }
        Commands::Setup {} => {
            setup_dotenv_file()?;
            Ok(())
        }
        Commands::SmokeTest { name } => {
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
        Commands::Start { name, provider } => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.start(&name).await?;
            Ok(())
        }
        Commands::Upgrade {
            ansible_verbose,
            env_variables,
            faucet_version,
            force_faucet,
            force_safenode,
            forks,
            name,
            provider,
            safenode_version,
        } => {
            let testnet_deploy = TestnetDeployBuilder::default()
                .ansible_verbose_mode(ansible_verbose)
                .provider(provider.clone())
                .build()?;
            testnet_deploy
                .upgrade(UpgradeOptions {
                    ansible_verbose,
                    env_variables,
                    faucet_version,
                    force_faucet,
                    force_safenode,
                    forks,
                    name,
                    provider,
                    safenode_version,
                })
                .await?;
            Ok(())
        }
        Commands::UpgradeNodeManager {
            name,
            provider,
            version,
        } => {
            println!("Upgrading the node manager binaries...");
            let testnet_deploy = TestnetDeployBuilder::default()
                .ansible_verbose_mode(false)
                .provider(provider.clone())
                .build()?;
            testnet_deploy
                .upgrade_node_manager(&name, version.parse()?)
                .await?;
            Ok(())
        }
        Commands::UploadTestData { name } => {
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
    }
}

// Validate the branch and version args along with the feature list
#[allow(clippy::type_complexity)]
async fn get_sn_codebase_type(
    branch: Option<String>,
    repo_owner: Option<String>,
    safe_version: Option<String>,
    safenode_version: Option<String>,
    safenode_manager_version: Option<String>,
    faucet_version: Option<String>,
    safenode_features: Option<Vec<String>>,
) -> Result<SnCodebaseType> {
    let use_versioning = faucet_version.is_some()
        || safe_version.is_some()
        || safenode_version.is_some()
        || safenode_manager_version.is_some();
    let build_binaries = branch.is_some() || repo_owner.is_some();
    if build_binaries && use_versioning {
        return Err(
            eyre!("Version numbers and branches cannot be supplied at the same time").suggestion(
                "Please choose whether you want to use version numbers or build the binaries",
            ),
        );
    }
    if use_versioning && safenode_features.is_some() {
        return Err(eyre!(
            "The --safenode-features argument only applies if we are building binaries"
        ));
    }
    if let (Some(_), None) | (None, Some(_)) = (&repo_owner, &branch) {
        return Err(eyre!(
            "The --branch and --repo-owner arguments must be supplied together"
        ));
    }

    let safenode_features = safenode_features.map(|list| list.join(","));
    let codebase_type = if use_versioning {
        let faucet_version = get_version_from_option(faucet_version, &ReleaseType::Faucet).await?;
        let safe_version = get_version_from_option(safe_version, &ReleaseType::Safe).await?;
        let safenode_version =
            get_version_from_option(safenode_version, &ReleaseType::Safenode).await?;
        let safenode_manager_version =
            get_version_from_option(safenode_manager_version, &ReleaseType::SafenodeManager)
                .await?;
        SnCodebaseType::Versioned {
            faucet_version,
            safe_version,
            safenode_version,
            safenode_manager_version,
        }
    } else if build_binaries {
        // Unwraps are justified here because it's already been asserted that both must have
        // values.
        let repo_owner = repo_owner.unwrap();
        let branch = branch.unwrap();

        let url = format!("https://github.com/{repo_owner}/safe_network/tree/{branch}",);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            bail!("The provided branch or owner does not exist: {url:?}");
        }
        SnCodebaseType::Branch {
            repo_owner,
            branch,
            safenode_features,
        }
    } else {
        SnCodebaseType::Main { safenode_features }
    };

    Ok(codebase_type)
}

// Since delimiter is on, we get element of the csv and not the entire csv.
fn parse_environment_variables(env_var: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = env_var.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(eyre!(
            "Environment variable must be in the format KEY=VALUE or KEY=INNER_KEY=VALUE.\nMultiple key-value pairs can be given with a comma between them."
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

async fn get_version_from_option(
    version: Option<String>,
    release_type: &ReleaseType,
) -> Result<Version> {
    let release_repo = <dyn SafeReleaseRepoActions>::default_config();
    let version = if let Some(version) = version {
        println!("Using {version} for {release_type}");
        version
    } else {
        println!("Getting latest version for {release_type}...");
        let version = release_repo
            .get_latest_version(release_type)
            .await?
            .to_string();
        println!("Using {version} for {release_type}");
        version
    };
    Ok(version.parse()?)
}
