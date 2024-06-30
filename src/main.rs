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
    deploy::DeployOptions,
    error::Error,
    get_wallet_directory,
    inventory::{get_data_directory, DeploymentInventory, DeploymentInventoryService},
    logstash::LogstashDeployBuilder,
    manage_test_data::TestDataClientBuilder,
    network_commands, notify_slack,
    setup::setup_dotenv_file,
    upscale::UpscaleOptions,
    BinaryOption, CloudProvider, LogFormat, TestnetDeployBuilder, UpgradeOptions,
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

#[allow(clippy::large_enum_variant)]
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
        /// Supply the beta encryption key for the auditor.
        ///
        /// If not used a default key will be supplied.
        #[arg(long)]
        beta_encryption_key: Option<String>,
        /// The number of safenode services to run on each bootstrap VM.
        #[clap(long, default_value_t = 1)]
        bootstrap_node_count: u16,
        /// The number of bootstrap node VMs to create.
        ///
        /// Each VM will run many safenode services.
        #[clap(long, default_value_t = 10)]
        bootstrap_node_vm_count: u16,
        /// The branch of the Github repository to build from.
        ///
        /// If used, all binaries will be built from this branch. It is typically used for testing
        /// changes on a fork.
        ///
        /// This argument must be used in conjunction with the --repo-owner argument.
        ///
        /// The --branch and --repo-owner arguments are mutually exclusive with the binary version
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        branch: Option<String>,
        /// Provide environment variables for the safenode service.
        ///
        /// This is useful to set the safenode's log levels. Each variable should be comma
        /// separated without any space.
        ///
        /// Example: --env SN_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = true, value_parser = parse_environment_variables, verbatim_doc_comment)]
        env_variables: Option<Vec<(String, String)>>,
        /// Supply a version number to be used for the faucet binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner arguments.
        /// You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        faucet_version: Option<String>,
        /// Override the maximum number of forks Ansible will use to execute tasks on target hosts.
        ///
        /// The default value from ansible.cfg is 50.
        #[clap(long)]
        forks: Option<usize>,
        /// Specify the logging format for the nodes.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// The name of the Logstash stack to forward logs to.
        #[clap(long, default_value = "main")]
        logstash_stack_name: String,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// The number of safenode services to run on each VM.
        #[clap(long, default_value_t = 25)]
        node_count: u16,
        /// The number of node VMs to create.
        ///
        /// Each VM will run many safenode services.
        #[clap(long, default_value_t = 10)]
        node_vm_count: u16,
        /// Protocol version is used to partition the network and will not allow nodes with
        /// different protocol versions to join.
        ///
        /// If set to 'restricted', the branch name is used as the protocol version; otherwise the
        /// version is set to the value supplied.
        ///
        /// This argument is mutually exclusive with the --safenode-version argument.
        #[arg(long, verbatim_doc_comment)]
        protocol_version: Option<String>,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
        /// If set to true, the RPC of the node will be accessible remotely.
        ///
        /// By default, the safenode RPC is only accessible via the 'localhost' and is not exposed for
        /// security reasons.
        #[clap(long, default_value_t = false, verbatim_doc_comment)]
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
        #[arg(long, verbatim_doc_comment)]
        repo_owner: Option<String>,
        /// The features to enable on the safenode binary.
        ///
        /// If not provided, the default feature set specified for the safenode binary are used.
        ///
        /// The features argument is mutually exclusive with the --safenode-version argument.
        #[clap(long, verbatim_doc_comment)]
        safenode_features: Option<Vec<String>>,
        /// Supply a version number for the safenode binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_version: Option<String>,
        /// Supply a version number for the safenode-manager binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        safenode_manager_version: Option<String>,
        /// Supply a version number for the sn_auditor binary.
        ///
        /// There should be no 'v' prefix.
        ///
        /// The version arguments are mutually exclusive with the --branch and --repo-owner
        /// arguments. You can only supply version numbers or a custom branch, not both.
        #[arg(long, verbatim_doc_comment)]
        sn_auditor_version: Option<String>,
        /// The number of uploader VMs to create.
        #[clap(long, default_value_t = 5)]
        uploader_vm_count: u16,
    },
    Inventory {
        /// If set to true, the inventory will be regenerated.
        ///
        /// This is useful if the testnet was created on another machine.
        #[clap(long, default_value_t = false)]
        force_regeneration: bool,
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// Provide a name for the network contacts file to be uploaded to S3.
        ///
        /// If not used, the contacts file will have the same name as the environment.
        #[arg(long)]
        network_contacts_file_name: Option<String>,
        /// The cloud provider that was used.
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
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
        /// Supply a version number for the safe binary to be used in the smoke test.
        ///
        /// This does not apply to a testnet that was deployed with a custom branch reference, in
        /// which case, the safe binary used for the test will be the one that was built along with
        /// the deployment.
        ///
        /// There should be no 'v' prefix.
        #[arg(long)]
        safe_version: Option<String>,
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
        /// Optionally supply a version number for the safenode binary to upgrade to.
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
        #[arg(short = 'v', long)]
        version: String,
    },
    /// Clean a deployed testnet environment.
    #[clap(name = "upload-test-data")]
    UploadTestData {
        /// The name of the environment.
        #[arg(short = 'n', long)]
        name: String,
        /// Supply a version number for the safe binary to be used in the smoke test.
        ///
        /// This does not apply to a testnet that was deployed with a custom branch reference, in
        /// which case, the safe binary used for the test will be the one that was built along with
        /// the deployment.
        ///
        /// There should be no 'v' prefix.
        #[arg(long)]
        safe_version: Option<String>,
    },
    /// Upscale VMs and node services for an existing network.
    Upscale {
        /// Set to run Ansible with more verbose output.
        #[arg(long)]
        ansible_verbose: bool,
        /// The desired number of safenode services to be running on each bootstrap VM after the
        /// scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        ///
        /// Note: bootstrap VMs normally only use a single node service, so you probably want this
        /// value to be 1.
        #[clap(long, verbatim_doc_comment)]
        desired_bootstrap_node_count: Option<u16>,
        /// The desired number of bootstrap VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 20, use 20 as the value
        /// not 10 as a delta.
        #[clap(long, verbatim_doc_comment)]
        desired_bootstrap_node_vm_count: Option<u16>,
        /// If set to true, for new VMs the RPC of the node will be accessible remotely.
        ///
        /// By default, the safenode RPC is only accessible via the 'localhost' and is not exposed for
        /// security reasons.
        #[clap(long, default_value_t = false, verbatim_doc_comment)]
        public_rpc: bool,
        /// The name of the existing network to upscale.
        #[arg(short = 'n', long, verbatim_doc_comment)]
        name: String,
        /// The desired number of safenode services to be running on each node VM after the scale.
        ///
        /// If there are currently 10 services running on each VM, and you want there to be 25, the
        /// value used should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_node_count: Option<u16>,
        /// The desired number of node VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_node_vm_count: Option<u16>,
        /// The desired number of uploader VMs to be running after the scale.
        ///
        /// If there are currently 10 VMs running, and you want there to be 25, the value used
        /// should be 25, rather than 15 as a delta to reach 25.
        #[clap(long, verbatim_doc_comment)]
        desired_uploader_vm_count: Option<u16>,
        /// The cloud provider for the network.
        #[clap(long, value_parser = parse_provider, verbatim_doc_comment, default_value_t = CloudProvider::DigitalOcean)]
        provider: CloudProvider,
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
            let testnet_deploy = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;
            testnet_deploy.clean().await?;
            Ok(())
        }
        Commands::Deploy {
            ansible_verbose,
            beta_encryption_key,
            branch,
            bootstrap_node_count,
            bootstrap_node_vm_count,
            env_variables,
            faucet_version,
            forks,
            log_format,
            logstash_stack_name,
            name,
            network_contacts_file_name,
            node_count,
            node_vm_count,
            protocol_version,
            provider,
            public_rpc,
            repo_owner,
            safenode_features,
            safenode_version,
            safenode_manager_version,
            sn_auditor_version,
            uploader_vm_count,
        } => {
            let binary_option = get_binary_option(
                branch,
                protocol_version,
                repo_owner,
                faucet_version,
                safenode_version,
                safenode_manager_version,
                sn_auditor_version,
                safenode_features,
            )
            .await?;

            let mut builder = TestnetDeployBuilder::default();
            builder
                .ansible_verbose_mode(ansible_verbose)
                .environment_name(&name)
                .provider(provider.clone());
            if let Some(forks) = forks {
                builder.ansible_forks(forks);
            }
            let testnet_deployer = builder.build()?;

            let inventory_service = DeploymentInventoryService::from(testnet_deployer.clone());
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
                .await?;

            match testnet_deployer.init().await {
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
                    .environment_name(&name)
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

            testnet_deployer
                .deploy(&DeployOptions {
                    beta_encryption_key,
                    binary_option: binary_option.clone(),
                    bootstrap_node_count,
                    bootstrap_node_vm_count,
                    current_inventory: inventory,
                    env_variables,
                    log_format,
                    logstash_details,
                    name: name.clone(),
                    node_count,
                    node_vm_count,
                    public_rpc,
                    uploader_vm_count,
                })
                .await?;

            let max_retries = 3;
            let mut retries = 0;
            let inventory = loop {
                match inventory_service
                    .generate_or_retrieve_inventory(&name, true, Some(binary_option.clone()))
                    .await
                {
                    Ok(inv) => break inv,
                    Err(e) if retries < max_retries => {
                        retries += 1;
                        eprintln!("Failed to generate inventory on attempt {retries}: {:?}", e);
                        eprintln!("Will retry up to {max_retries} times...");
                    }
                    Err(_) => {
                        eprintln!("Failed to generate inventory after {max_retries} attempts");
                        eprintln!(
                            "Please try running the `inventory` command or workflow separately"
                        );
                        return Ok(());
                    }
                }
            };

            inventory.print_report()?;
            inventory.save()?;

            inventory_service
                .upload_network_contacts(&inventory, network_contacts_file_name)
                .await?;

            Ok(())
        }
        Commands::Inventory {
            force_regeneration,
            name,
            network_contacts_file_name,
            provider,
        } => {
            let testnet_deploy = TestnetDeployBuilder::default()
                .environment_name(&name)
                .provider(provider)
                .build()?;

            let inventory_service = DeploymentInventoryService::from(testnet_deploy);
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, force_regeneration, None)
                .await?;
            inventory.print_report()?;
            inventory.save()?;

            inventory_service
                .upload_network_contacts(&inventory, network_contacts_file_name)
                .await?;

            Ok(())
        }
        Commands::Logs(log_cmd) => match log_cmd {
            LogCommands::Rsync {
                name,
                provider,
                resources_only,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deploy.init().await?;
                testnet_deploy.rsync_logs(&name, resources_only).await?;
                Ok(())
            }
            LogCommands::Rg {
                args,
                name,
                provider,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deploy.init().await?;
                testnet_deploy.ripgrep_logs(&name, &args).await?;
                Ok(())
            }
            LogCommands::Copy {
                name,
                provider,
                resources_only,
            } => {
                let testnet_deploy = TestnetDeployBuilder::default()
                    .environment_name(&name)
                    .provider(provider)
                    .build()?;
                testnet_deploy.init().await?;
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
        Commands::SmokeTest { name, safe_version } => {
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
            test_data_client
                .smoke_test(&mut inventory, safe_version)
                .await?;
            inventory.save()?;
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
                .ansible_forks(forks)
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
        Commands::UploadTestData { name, safe_version } => {
            let inventory_path = get_data_directory()?.join(format!("{name}-inventory.json"));
            if !inventory_path.exists() {
                return Err(eyre!("There is no inventory for the {name} testnet")
                    .suggestion("Please run the inventory command to generate it"));
            }

            let mut inventory = DeploymentInventory::read(&inventory_path)?;
            let mut rng = rand::thread_rng();
            let i = rng.gen_range(0..inventory.peers().len());
            let random_peer = &inventory.peers()[i];

            let test_data_client = TestDataClientBuilder::default().build()?;
            let uploaded_files = test_data_client
                .upload_test_data(&name, random_peer, &inventory.binary_option, safe_version)
                .await?;

            println!("Uploaded files:");
            for (path, address) in uploaded_files.iter() {
                println!("{path}: {address}");
            }
            inventory.add_uploaded_files(uploaded_files.clone());
            inventory.save()?;

            Ok(())
        }
        Commands::Upscale {
            ansible_verbose,
            desired_bootstrap_node_count,
            desired_bootstrap_node_vm_count,
            name,
            desired_node_count,
            desired_node_vm_count,
            desired_uploader_vm_count,
            provider,
            public_rpc,
        } => {
            println!("Upscaling deployment...");
            let testnet_deployer = TestnetDeployBuilder::default()
                .ansible_verbose_mode(ansible_verbose)
                .environment_name(&name)
                .provider(provider.clone())
                .build()?;
            testnet_deployer.init().await?;

            let inventory_service = DeploymentInventoryService::from(testnet_deployer.clone());
            let inventory = inventory_service
                .generate_or_retrieve_inventory(&name, true, None)
                .await?;

            testnet_deployer
                .upscale(&UpscaleOptions {
                    ansible_verbose,
                    current_inventory: inventory,
                    desired_bootstrap_node_count,
                    desired_bootstrap_node_vm_count,
                    desired_node_count,
                    desired_node_vm_count,
                    desired_uploader_vm_count,
                    public_rpc,
                })
                .await?;

            println!("Generating new inventory after upscale...");
            let max_retries = 3;
            let mut retries = 0;
            let inventory = loop {
                match inventory_service
                    .generate_or_retrieve_inventory(&name, true, None)
                    .await
                {
                    Ok(inv) => break inv,
                    Err(e) if retries < max_retries => {
                        retries += 1;
                        eprintln!("Failed to generate inventory on attempt {retries}: {:?}", e);
                        eprintln!("Will retry up to {max_retries} times...");
                    }
                    Err(_) => {
                        eprintln!("Failed to generate inventory after {max_retries} attempts");
                        eprintln!(
                            "Please try running the `inventory` command or workflow separately"
                        );
                        return Ok(());
                    }
                }
            };

            inventory.print_report()?;
            inventory.save()?;

            Ok(())
        }
    }
}

/// Get the binary option for the deployment.
///
/// Versioned binaries are preferred first, since building from source adds significant time to the
/// deployment. There are two options here. If no version arguments were supplied, the latest
/// versions will be used. Otherwise, the specified versions will be used, and if any were not
/// specified, the latest version will be used in its place.
///
/// The second option is to build from source, which is useful for testing changes from forks.
///
/// The usage of arguments are also validated here.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
async fn get_binary_option(
    branch: Option<String>,
    protocol_version: Option<String>,
    repo_owner: Option<String>,
    faucet_version: Option<String>,
    safenode_version: Option<String>,
    safenode_manager_version: Option<String>,
    sn_auditor_version: Option<String>,
    safenode_features: Option<Vec<String>>,
) -> Result<BinaryOption> {
    let mut use_versions = true;

    let branch_specified = branch.is_some() || repo_owner.is_some();
    let versions_specified = faucet_version.is_some()
        || safenode_version.is_some()
        || safenode_manager_version.is_some();
    if branch_specified && versions_specified {
        return Err(
            eyre!("Version numbers and branches cannot be supplied at the same time").suggestion(
                "Please choose whether you want to use version numbers or build the binaries",
            ),
        );
    }

    if versions_specified {
        if safenode_features.is_some() {
            return Err(eyre!(
                "The --safenode-features argument only applies if we are building binaries"
            ));
        }
        if protocol_version.is_some() {
            return Err(eyre!(
                "The --protocol-version argument only applies if we are building binaries"
            ));
        }
    }

    if branch_specified {
        if let (Some(_), None) | (None, Some(_)) = (&repo_owner, &branch) {
            return Err(eyre!(
                "The --branch and --repo-owner arguments must be supplied together"
            ));
        }
        use_versions = false;
    }

    let binary_option = if use_versions {
        print_with_banner("Binaries will be supplied from pre-built versions");

        let faucet_version = get_version_from_option(faucet_version, &ReleaseType::Faucet).await?;
        let safenode_version =
            get_version_from_option(safenode_version, &ReleaseType::Safenode).await?;
        let safenode_manager_version =
            get_version_from_option(safenode_manager_version, &ReleaseType::SafenodeManager)
                .await?;
        let sn_auditor_version =
            get_version_from_option(sn_auditor_version, &ReleaseType::SnAuditor).await?;
        BinaryOption::Versioned {
            faucet_version,
            safenode_version,
            safenode_manager_version,
            sn_auditor_version,
        }
    } else {
        // Unwraps are justified here because it's already been asserted that both must have
        // values.
        let repo_owner = repo_owner.unwrap();
        let branch = branch.unwrap();

        print_with_banner(&format!(
            "Binaries will be built from {}/{}",
            repo_owner, branch
        ));

        let url = format!("https://github.com/{repo_owner}/safe_network/tree/{branch}",);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            bail!("The provided branch or owner does not exist: {url:?}");
        }
        BinaryOption::BuildFromSource {
            repo_owner,
            branch,
            safenode_features: safenode_features.map(|list| list.join(",")),
            protocol_version,
        }
    };

    Ok(binary_option)
}

fn print_with_banner(s: &str) {
    let banner = "=".repeat(s.len());
    println!("{}\n{}\n{}", banner, s, banner);
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
