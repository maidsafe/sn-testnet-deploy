use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;
/// Internal error.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Could not determine content length for asset")]
    AssetContentLengthUndetermined,
    #[error("Both the repository owner and branch name must be supplied if either are used")]
    CustomBinConfigError,
    #[error("The '{0}' environment does not exist")]
    EnvironmentDoesNotExist(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    TemplateError(#[from] indicatif::style::TemplateError),
    #[error("Terraform run failed")]
    TerraformRunFailed,
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
