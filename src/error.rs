// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use std::net::IpAddr;

use crate::{ansible::inventory::AnsibleInventoryType, NodeType};
use evmlib::contract::network_token;
use thiserror::Error;
use tokio::task::JoinError;

pub type Result<T, E = Error> = std::result::Result<T, E>;
/// Internal error.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error(transparent)]
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("Could not determine content length for asset")]
    AssetContentLengthUndetermined,
    #[error(transparent)]
    AwsS3Error(#[from] Box<aws_sdk_s3::Error>),
    #[error("The {0} environment variable must be set to use your cloud provider")]
    CloudProviderCredentialsNotSupplied(String),
    #[error("The {0} cloud provider is not supported yet")]
    CloudProviderNotSupported(String),
    #[error("The home data directory could not be retrieved")]
    CouldNotRetrieveDataDirectory,
    #[error("Failed to delete '{0}' from '{1}")]
    DeleteS3ObjectError(String, String),
    #[error("Authorization failed for the Digital Ocean API")]
    DigitalOceanUnauthorized,
    #[error("Unexpected response: {0} -- {1}")]
    DigitalOceanUnexpectedResponse(u16, String),
    #[error("The public IP address was not obtainable from the API response")]
    DigitalOceanPublicIpAddressNotFound,
    #[error("Digital Ocean API rate limit exceeded after {0} retry attempts")]
    DigitalOceanRateLimitExhausted(u32),
    #[error("The provided ansible inventory is empty or does not exists {0}")]
    EmptyInventory(AnsibleInventoryType),
    #[error("Could not retrieve environment details for '{0}'")]
    EnvironmentDetailsNotFound(String),
    #[error("The '{0}' environment does not exist")]
    EnvironmentDoesNotExist(String),
    #[error("The environment name is required")]
    EnvironmentNameRequired,
    #[error("Could not convert '{0}' to an EnvironmentType variant")]
    EnvironmentNameFromStringError(String),
    #[error("No EVM node found in the inventory")]
    EvmNodeNotFound,
    #[error("EVM testnet data not found or could not be read")]
    EvmTestnetDataNotFound,
    #[error("Error parsing EVM testnet data: {0}")]
    EvmTestnetDataParsingError(String),
    #[error("Command that executed with {binary} failed. See output for details.")]
    ExternalCommandRunFailed {
        binary: String,
        exit_status: std::process::ExitStatus,
    },
    #[error("Failed to parse key")]
    FailedToParseKey,
    #[error("Failed to retrieve filename")]
    FilenameNotRetrieved,
    #[error(transparent)]
    FsExtraError(#[from] fs_extra::error::Error),
    #[error("Could not obtain Genesis multiaddr")]
    GenesisListenAddress,
    #[error("To provision the remaining nodes the multiaddr of the genesis node must be supplied")]
    GenesisMultiAddrNotSupplied,
    #[error("Failed to retrieve '{0}' from '{1}")]
    GetS3ObjectError(String, String),
    #[error(transparent)]
    InquireError(#[from] inquire::InquireError),
    #[error("'{0}' is not a valid binary to build")]
    InvalidBinaryName(String),
    #[error("The node type '{0:?}' is not supported")]
    InvalidNodeType(NodeType),
    #[error("The number of wallet secret keys ({0}) does not match the number of uploaders ({1})")]
    InvalidWalletCount(usize, usize),
    #[error(
        "The '{0}' deployment type for the environment is not supported for upscaling Clients"
    )]
    InvalidClientUpscaleDeploymentType(String),
    #[error("The desired auditor VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredAuditorVmCount,
    #[error("The desired Client count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredClientCount,
    #[error("The desired Client VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredClientVmCount,
    #[error("The desired Peer Cache VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredPeerCacheVmCount,
    #[error("The desired Peer Cache node count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredPeerCacheNodeCount,
    #[error("The desired node VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredNodeVmCount,
    #[error("The desired node count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredNodeCount,
    #[error("The desired full cone private node VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredFullConePrivateNodeVmCount,
    #[error("The desired symmetric private node VM count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredSymmetricPrivateNodeVmCount,
    #[error("The desired full cone private node count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredFullConePrivateNodeCount,
    #[error("The desired symmetric private node count is smaller than the current count. This is invalid for an upscale operation.")]
    InvalidUpscaleDesiredSymmetricPrivateNodeCount,
    #[error("Options were used that are not applicable to a bootstrap deployment")]
    InvalidUpscaleOptionsForBootstrapDeployment,
    #[error("No {0} inventory found for {1} at {2}")]
    InventoryNotFound(String, String, String),
    #[error("The vm count for the provided custom vms are not equal: {0:?} != {1:?}")]
    VmCountMismatch(Option<AnsibleInventoryType>, Option<AnsibleInventoryType>),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Could not obtain IpDetails")]
    IpDetailsNotObtained,
    #[error(transparent)]
    JoinError(#[from] JoinError),
    #[error("Failed to list objects in S3 bucket with prefix '{prefix}': {error}")]
    ListS3ObjectsError { prefix: String, error: String },
    #[error("Could not configure logging: {0}")]
    LoggingConfiguration(String),
    #[error("Logs for a '{0}' testnet already exist")]
    LogsForPreviousTestnetExist(String),
    #[error("Logs have not been retrieved for the '{0}' environment.")]
    LogsNotRetrievedError(String),
    #[error("The API response did not contain the expected '{0}' value")]
    MalformedDigitalOceanApiRespose(String),
    #[error("Could not convert from DeployOptions to ProvisionOptions: peer cache node count must have a value")]
    MissingPeerCacheNodeCount,
    #[error(
        "Could not convert from DeployOptions to ProvisionOptions: node count must have a value"
    )]
    MissingNodeCount,
    #[error("The NAT gateway VM was not supplied")]
    NatGatewayNotSupplied,
    #[error(transparent)]
    NetworkTokenError(#[from] network_token::Error),
    #[error("This deployment does not have an auditor. It may be a bootstrap deployment.")]
    NoAuditorError,
    #[error("This deployment does not have any Client. It may be a bootstrap deployment.")]
    NoClientError,
    #[error("This deployment does not have a faucet. It may be a bootstrap deployment.")]
    NoFaucetError,
    #[error("The node count for the provided custom vms are not equal")]
    NodeCountMismatch,
    #[error("Could not obtain a multiaddr from the node inventory")]
    NodeAddressNotFound,
    #[error("Failed to upload {0} to S3 bucket {1}")]
    PutS3ObjectError(String, String),
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error("The rewards address must be supplied")]
    RewardsAddressNotSet,
    #[error("A wallet secret key must be provided for the repair run")]
    RepairWalletAddressNotProvided,
    #[error("Routed VM for IP {0} not found")]
    RoutedVmNotFound(IpAddr),
    #[error("Safe client command failed: {0}")]
    SafeCmdError(String),
    #[error("Failed to download the safe or safenode binary")]
    SafeBinaryDownloadError,
    #[error("Error in byte stream when attempting to retrieve S3 object")]
    S3ByteStreamError,
    #[error("The secret key was not found in the environment")]
    SecretKeyNotFound,
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error("An unexpected error occurred during the setup process")]
    SetupError,
    #[error("The SLACK_WEBHOOK_URL variable was not set")]
    SlackWebhookUrlNotSupplied,
    #[error("SSH command failed: {0}")]
    SshCommandFailed(String),
    #[error("Failed to obtain lock to update SSH settings")]
    SshSettingsRwLockError,
    #[error("After several retry attempts an SSH connection could not be established")]
    SshUnavailable,
    #[error(transparent)]
    StripPrefixError(#[from] std::path::StripPrefixError),
    #[error(transparent)]
    TemplateError(#[from] indicatif::style::TemplateError),
    #[error("Terraform show failed")]
    TerraformShowFailed,
    #[error("Terraform resource not found {0}")]
    TerraformResourceNotFound(String),
    #[error("Mismatch of a terraform resource value {expected} != {actual}")]
    TerraformResourceValueMismatch { expected: String, actual: String },
    #[error("The '{0}' binary was not found. It is required for the deploy process. Make sure it is installed.")]
    ToolBinaryNotFound(String),
    #[error("The {0} type is not yet supported for an upscaling provision")]
    UpscaleInventoryTypeNotSupported(String),
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
