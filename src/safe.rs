// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::run_external_command;
#[cfg(test)]
use mockall::automock;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Provides an interface for using the `safe` client.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// safe process.
#[cfg_attr(test, automock)]
pub trait SafeClientInterface {
    fn upload_file(&self, peer_multiaddr: &str, path: &Path) -> Result<String>;
}

pub struct SafeClient {
    pub binary_path: PathBuf,
    pub working_directory_path: PathBuf,
}
impl SafeClient {
    pub fn new(binary_path: PathBuf, working_directory_path: PathBuf) -> SafeClient {
        SafeClient {
            binary_path,
            working_directory_path,
        }
    }
}

impl SafeClientInterface for SafeClient {
    fn upload_file(&self, peer_multiaddr: &str, path: &Path) -> Result<String> {
        let output = run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "--peer".to_string(),
                peer_multiaddr.to_string(),
                "files".to_string(),
                "upload".to_string(),
                path.to_string_lossy().to_string(),
            ],
            false,
        )?;

        let re = Regex::new(r"Uploaded .+ to ([a-fA-F0-9]+)")?;
        for line in &output {
            if let Some(captures) = re.captures(line) {
                return Ok(captures[1].to_string());
            }
        }

        Err(Error::SafeCmdError(
            "could not obtain hex address of uploaded file".to_string(),
        ))
    }
}
