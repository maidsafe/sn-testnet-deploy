// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use thiserror::Error;

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
    #[error("Failed to delete '{0}' from '{1}")]
    DeleteS3ObjectError(String, String),
    #[error("Authorization failed for the Digital Ocean API")]
    DigitalOceanUnauthorized,
    #[error("Unexpected response: {0} -- {1}")]
    DigitalOceanUnexpectedResponse(u16, String),
    #[error("The '{0}' environment does not exist")]
    EnvironmentDoesNotExist(String),
    #[error("Command that executed with {0} failed. See output for details.")]
    ExternalCommandRunFailed(String),
    #[error("To provision the remaining nodes the multiaddr of the genesis node must be supplied")]
    GenesisMultiAddrNotSupplied,
    #[error("Failed to retrieve '{0}' from '{1}")]
    GetS3ObjectError(String, String),
    #[error(transparent)]
    FsExtraError(#[from] fs_extra::error::Error),
    #[error(transparent)]
    InquireError(#[from] inquire::InquireError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Failed to list objects in S3 bucket with '{0}' prefix")]
    ListS3ObjectsError(String),
    #[error("Logs for a '{0}' testnet already exist")]
    LogsForPreviousTestnetExist(String),
    #[error("Logs have not been retrieved for the '{0}' environment.")]
    LogsNotRetrievedError(String),
    #[error("The API response did not contain the expected '{0}' value")]
    MalformedDigitalOceanApiRespose(String),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error("Error in byte stream when attempting to retrieve S3 object")]
    S3ByteStreamError,
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error("An unexpected error occurred during the setup process")]
    SetupError,
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
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
