use clap::{Parser, Subcommand};
use color_eyre::{eyre::eyre, Result};
use dotenv::dotenv;
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
    /// Initialise a new testnet environment
    Init {
        /// The name of the environment
        #[arg(short = 'n', long)]
        name: String,
        /// The cloud provider to deploy to.
        ///
        /// Valid values are "aws" or "digital-ocean".
        #[clap(long, default_value_t = CloudProvider::DigitalOcean, value_parser = parse_provider, verbatim_doc_comment)]
        provider: CloudProvider,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let opt = Opt::parse();
    match opt.command {
        Some(Commands::Init { name, provider }) => {
            let testnet_deploy = TestnetDeployBuilder::default().provider(provider).build()?;
            testnet_deploy.init(&name).await?;
            Ok(())
        }
        None => Ok(()),
    }
}
