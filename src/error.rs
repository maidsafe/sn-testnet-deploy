// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::ansible::AnsibleInventoryType;
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
    #[error("Could not retrieve environment details for '{0}'")]
    EnvironmentDetailsNotFound(String),
    #[error("The '{0}' environment does not exist")]
    EnvironmentDoesNotExist(String),
    #[error("The environment name is required")]
    EnvironmentNameRequired,
    #[error("Could not convert '{0}' to an EnvironmentType variant")]
    EnvironmentNameFromStringError(String),
    #[error("Command that executed with {binary} failed. See output for details.")]
    ExternalCommandRunFailed {
        binary: String,
        exit_status: std::process::ExitStatus,
    },
    #[error("The provided ansible inventory is empty or does not exists {0}")]
    EmptyInventory(AnsibleInventoryType),
    #[error("To provision the remaining nodes the multiaddr of the genesis node must be supplied")]
    GenesisMultiAddrNotSupplied,
    #[error("Could not obtain Genesis multiaddr")]
    GenesisListenAddress,
    #[error("Failed to retrieve '{0}' from '{1}")]
    GetS3ObjectError(String, String),
    #[error("Failed to retrieve filename")]
    FilenameNotRetrieved,
    #[error(transparent)]
    FsExtraError(#[from] fs_extra::error::Error),
    #[error(transparent)]
    JoinError(#[from] JoinError),
    #[error(transparent)]
    InquireError(#[from] inquire::InquireError),
    #[error(
        "The desired auditor VM count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredAuditorVmCount,
    #[error(
        "The desired bootstrap VM count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredBootstrapVmCount,
    #[error(
        "The desired bootstrap node count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredBootstrapNodeCount,
    #[error(
        "The desired node VM count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredNodeVmCount,
    #[error(
        "The desired node count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredNodeCount,
    #[error(
        "The desired uploader VM count is smaller than the current count. \
         This is invalid for an upscale operation."
    )]
    InvalidUpscaleDesiredUploaderVmCount,
    #[error("Options were used that are not applicable to a bootstrap deployment")]
    InvalidUpscaleOptionsForBootstrapDeployment,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Could not obtain IpDetails")]
    IpDetailsNotObtained,
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
    #[error("Could not convert from DeployOptions to ProvisionOptions: bootstrap node count must have a value")]
    MissingBootstrapNodeCount,
    #[error(
        "Could not convert from DeployOptions to ProvisionOptions: node count must have a value"
    )]
    MissingNodeCount,
    #[error("Could not obtain the private IP address")]
    PrivateIpNotObtained,
    #[error("This deployment does not have an auditor. It may be a bootstrap deployment.")]
    NoAuditorError,
    #[error("Could not obtain a multiaddr from the node inventory")]
    NodeAddressNotFound,
    #[error("This deployment does not have a faucet. It may be a bootstrap deployment.")]
    NoFaucetError,
    #[error("This deployment does not have any uploaders. It may be a bootstrap deployment.")]
    NoUploadersError,
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error("Failed to upload {0} to S3 bucket {1}")]
    PutS3ObjectError(String, String),
    #[error("Safe client command failed: {0}")]
    SafeCmdError(String),
    #[error("Failed to download the safe or safenode binary")]
    SafeBinaryDownloadError,
    #[error("Error in byte stream when attempting to retrieve S3 object")]
    S3ByteStreamError,
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error("An unexpected error occurred during the setup process")]
    SetupError,
    #[error("The SLACK_WEBHOOK_URL variable was not set")]
    SlackWebhookUrlNotSupplied,
    #[error("SSH command failed: {0}")]
    SshCommandFailed(String),
    #[error("After several retry attempts an SSH connection could not be established")]
    SshUnavailable,
    #[error(transparent)]
    StripPrefixError(#[from] std::path::StripPrefixError),
    #[error(transparent)]
    TemplateError(#[from] indicatif::style::TemplateError),
    #[error(
        "The '{0}' binary was not found. It is required for the deploy process. Make sure it is installed."
    )]
    ToolBinaryNotFound(String),
    #[error("The {0} type is not yet supported for an upscaling provision")]
    UpscaleInventoryTypeNotSupported(String),
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
