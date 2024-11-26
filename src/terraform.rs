// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    is_binary_on_path, run_external_command, CloudProvider,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone)]
pub struct TerraformRunner {
    pub binary_path: PathBuf,
    pub provider: CloudProvider,
    pub working_directory_path: PathBuf,
    pub state_bucket_name: String,
}

impl TerraformRunner {
    pub fn new(
        binary_path: PathBuf,
        working_directory: PathBuf,
        provider: CloudProvider,
        state_bucket_name: &str,
    ) -> Result<TerraformRunner> {
        if !binary_path.exists() {
            // Try the path as a single binary name.
            let bin_name = binary_path.to_string_lossy().to_string();
            if !is_binary_on_path(&binary_path.to_string_lossy()) {
                return Err(Error::ToolBinaryNotFound(bin_name));
            }
        }
        let runner = TerraformRunner {
            binary_path,
            working_directory_path: working_directory,
            provider,
            state_bucket_name: state_bucket_name.to_string(),
        };
        Ok(runner)
    }

    pub fn apply(
        &self,
        vars: Vec<(String, String)>,
        tfvars_filename: Option<String>,
    ) -> Result<()> {
        let mut args = vec!["apply".to_string(), "-auto-approve".to_string()];
        if let Some(tfvars_filename) = tfvars_filename {
            args.push(format!("-var-file={}", tfvars_filename));
        }
        for var in vars.iter() {
            args.push("-var".to_string());
            args.push(format!("{}={}", var.0, var.1));
        }
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }

    pub fn plan(
        &self,
        vars: Option<Vec<(String, String)>>,
        tfvars_filename: Option<String>,
    ) -> Result<()> {
        let mut args = vec!["plan".to_string()];
        if let Some(tfvars_filename) = tfvars_filename {
            args.push(format!("-var-file={}", tfvars_filename));
        }
        if let Some(vars) = vars {
            for var in vars.iter() {
                args.push("-var".to_string());
                args.push(format!("{}={}", var.0, var.1));
            }
        }
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }

    pub fn destroy(
        &self,
        vars: Option<Vec<(String, String)>>,
        tfvars_filename: Option<String>,
    ) -> Result<()> {
        let mut args = vec!["destroy".to_string(), "-auto-approve".to_string()];
        if let Some(tfvars_filename) = tfvars_filename {
            args.push(format!("-var-file={}", tfvars_filename));
        }
        if let Some(vars) = vars {
            for var in vars.iter() {
                args.push("-var".to_string());
                args.push(format!("{}={}", var.0, var.1));
            }
        }
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }

    pub fn init(&self) -> Result<()> {
        let args = vec![
            "init".to_string(),
            "-backend-config".to_string(),
            format!("bucket={}", self.state_bucket_name),
        ];
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            args,
            false,
            false,
        )?;
        Ok(())
    }

    pub fn show(&self, name: &str) -> Result<Vec<TerraformResource>> {
        self.workspace_select(name)?;

        let output = run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec!["show".to_string(), "--json".to_string()],
            true,
            false,
        )?;

        let output = output.first().ok_or(Error::TerraformShowFailed)?;
        let show_output: Output = serde_json::from_str(output)?;

        Ok(show_output.values.root_module.resources)
    }

    pub fn workspace_delete(&self, name: &str) -> Result<()> {
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "workspace".to_string(),
                "delete".to_string(),
                name.to_string(),
            ],
            true,
            false,
        )?;
        Ok(())
    }

    pub fn workspace_list(&self) -> Result<Vec<String>> {
        let output = run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec!["workspace".to_string(), "list".to_string()],
            true,
            false,
        )?;
        let workspaces: Vec<String> = output
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().trim_start_matches('*').trim().to_string())
            .collect();
        Ok(workspaces)
    }

    pub fn workspace_new(&self, name: &str) -> Result<()> {
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec!["workspace".to_string(), "new".to_string(), name.to_string()],
            false,
            false,
        )?;
        Ok(())
    }

    pub fn workspace_select(&self, name: &str) -> Result<()> {
        run_external_command(
            self.binary_path.clone(),
            self.working_directory_path.clone(),
            vec![
                "workspace".to_string(),
                "select".to_string(),
                name.to_string(),
            ],
            false,
            false,
        )?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Output {
    values: Values,
}

#[derive(Serialize, Deserialize, Debug)]
struct Values {
    root_module: Module,
}

#[derive(Serialize, Deserialize, Debug)]
struct Module {
    resources: Vec<TerraformResource>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TerraformResource {
    pub address: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    #[serde(rename = "name")]
    pub resource_name: String,
    pub index: Option<serde_json::Value>,
    pub values: HashMap<String, serde_json::Value>,
    pub sensitive_values: HashMap<String, serde_json::Value>,
}
