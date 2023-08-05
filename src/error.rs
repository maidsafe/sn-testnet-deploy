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
    #[error("Both the repository owner and branch name must be supplied if either are used")]
    CustomBinConfigError,
    #[error("The '{0}' environment does not exist")]
    EnvironmentDoesNotExist(String),
    #[error("Command executed with {0} failed. See output for details.")]
    ExternalCommandRunFailed(String),
    #[error("To provision the remaining nodes the multiaddr of the genesis node must be supplied")]
    GenesisMultiAddrNotSupplied,
    #[error(transparent)]
    InquireError(#[from] inquire::InquireError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error("An unexpected error occurred during the setup process")]
    SetupError,
    #[error("After several retry attempts an SSH connection could not be established")]
    SshUnavailable,
    #[error(transparent)]
    TemplateError(#[from] indicatif::style::TemplateError),
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
