use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;
/// Internal error.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Terraform run failed")]
    TerraformRunFailed,
}
