use crate::error::{Error, Result};
use crate::CloudProvider;
#[cfg(test)]
use mockall::automock;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Provides an interface which corresponds to Terraform commands.
///
/// To keep things simple, each subcommand will be its own function.
///
/// This trait exists for unit testing: it enables the testing of behaviour without actually
/// calling the Terraform process.
#[cfg_attr(test, automock)]
pub trait TerraformRunnerInterface {
    fn apply(&self, vars: Vec<(String, String)>) -> Result<()>;
    fn init(&self) -> Result<()>;
    fn workspace_list(&self) -> Result<Vec<String>>;
    fn workspace_new(&self, name: &str) -> Result<()>;
    fn workspace_select(&self, name: &str) -> Result<()>;
}

pub struct TerraformRunner {
    pub binary_path: PathBuf,
    pub provider: CloudProvider,
    pub working_directory: PathBuf,
    pub state_bucket_name: String,
}

impl TerraformRunnerInterface for TerraformRunner {
    fn apply(&self, vars: Vec<(String, String)>) -> Result<()> {
        let mut args = vec!["apply".to_string(), "-auto-approve".to_string()];
        for var in vars.iter() {
            args.push("-var".to_string());
            args.push(format!("{}={}", var.0, var.1));
        }
        self.run_terraform_command(args, false)?;
        Ok(())
    }

    fn init(&self) -> Result<()> {
        let args = vec![
            "init".to_string(),
            "-backend-config".to_string(),
            format!("bucket={}", self.state_bucket_name),
        ];
        self.run_terraform_command(args, false)?;
        Ok(())
    }

    fn workspace_list(&self) -> Result<Vec<String>> {
        let output =
            self.run_terraform_command(vec!["workspace".to_string(), "list".to_string()], true)?;
        let workspaces: Vec<String> = output
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().trim_start_matches('*').trim().to_string())
            .collect();
        Ok(workspaces)
    }

    fn workspace_new(&self, name: &str) -> Result<()> {
        self.run_terraform_command(
            vec!["workspace".to_string(), "new".to_string(), name.to_string()],
            false,
        )?;
        Ok(())
    }

    fn workspace_select(&self, name: &str) -> Result<()> {
        self.run_terraform_command(
            vec![
                "workspace".to_string(),
                "select".to_string(),
                name.to_string(),
            ],
            false,
        )?;
        Ok(())
    }
}

impl TerraformRunner {
    pub fn new(
        binary_path: PathBuf,
        working_directory: PathBuf,
        provider: CloudProvider,
        state_bucket_name: &str,
    ) -> TerraformRunner {
        TerraformRunner {
            binary_path,
            working_directory,
            provider,
            state_bucket_name: state_bucket_name.to_string(),
        }
    }

    fn run_terraform_command(
        &self,
        args: Vec<String>,
        suppress_output: bool,
    ) -> Result<Vec<String>> {
        let mut command = Command::new(&self.binary_path);
        for arg in args {
            command.arg(arg);
        }
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.current_dir(&self.working_directory);
        let mut child = command.spawn()?;
        let mut output_lines = Vec::new();

        if let Some(ref mut stdout) = child.stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = line?;
                if !suppress_output {
                    println!("{}", &line);
                }
                output_lines.push(line);
            }
        }

        if let Some(ref mut stderr) = child.stderr {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let line = line?;
                if !suppress_output {
                    println!("{}", &line);
                }
                output_lines.push(line);
            }
        }

        let output = child.wait()?;
        if !output.success() {
            return Err(Error::TerraformRunFailed);
        }

        Ok(output_lines)
    }
}
