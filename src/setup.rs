// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use inquire::{Select, Text};

pub fn setup_dotenv_file() -> Result<()> {
    let default_ansible_vault_password_path = dirs_next::home_dir()
        .ok_or_else(|| Error::SetupError)?
        .join(".ansible")
        .join("vault-password")
        .to_string_lossy()
        .to_string();
    let ansible_vault_password_path =
        Text::new("Please supply the path of the vault password file for Ansible:")
            .with_help_message(
                "If you do not have the vault password, contact a team member who can supply a \
                copy of it. Then write it out to this file, or another one of your choosing.",
            )
            .with_initial_value(&default_ansible_vault_password_path)
            .with_validator(inquire::required!())
            .prompt()?;

    let aws_access_key_id = Text::new("Please supply your AWS access key ID:")
        .with_help_message(
            "Even if you do not deploy to AWS, this is required for Terraform state storage.",
        )
        .with_validator(inquire::required!())
        .prompt()?;
    let aws_access_secret_access_key =
        Text::new("Please supply the corresponding AWS secret access key:")
            .with_help_message(
                "Even if you do not deploy to AWS, this is required for Terraform state storage.",
            )
            .with_validator(inquire::required!())
            .prompt()?;
    let aws_region = Text::new("Please supply the AWS region:")
        .with_help_message("This is not for the state bucket, but for the region to deploy to.")
        .with_initial_value("eu-west-2")
        .with_validator(inquire::required!())
        .prompt()?;
    let digital_ocean_pat = Text::new("Please supply your PAT for Digital Ocean:")
        .with_help_message(
            "This is required for creating droplets that will host the nodes. \
            If you do not have a PAT, contact someone in your team who can get you setup with one.",
        )
        .with_validator(inquire::required!())
        .prompt()?;
    let ssh_key_files = get_ssh_key_file_candidates()?;
    let ssh_key_path = Select::new(
        "Please select an SSH key from your ~/.ssh directory",
        ssh_key_files,
    )
    .with_help_message("This key will be used for SSH access to droplets or EC2 instances.")
    .prompt()?;
    let slack_webhook_url =
        Text::new("Please supply the Slack webhook URL for sending notifications:")
            .with_help_message(
                "If you do not have this, contact a team member who can supply it. \
                This is an optional value.",
            )
            .with_initial_value("")
            .with_validator(inquire::required!())
            .prompt()?;
    let sn_testnet_dev_subnet_id =
        Text::new("Please supply the ID of the VPC subnet for launching EC2 instances:")
            .with_help_message("If you are unsure of this value, just accept the default.")
            .with_initial_value("subnet-018f2ab26755df7f9")
            .with_validator(inquire::required!())
            .prompt()?;
    let sn_testnet_dev_security_group_id =
        Text::new("Please supply the ID of the VPC security group for launching EC2 instances:")
            .with_help_message("If you are unsure of this value, just accept the default.")
            .with_initial_value("sg-0d47df5b3f0d01e2a")
            .with_validator(inquire::required!())
            .prompt()?;
    let terraform_state_bucket_name =
        Text::new("Please supply the name of the S3 bucket for Terraform state:")
            .with_help_message("If you are unsure of this value, just accept the default.")
            .with_initial_value("maidsafe-org-infra-tfstate")
            .with_validator(inquire::required!())
            .prompt()?;

    let contents = format!(
        r#"
ANSIBLE_VAULT_PASSWORD_PATH={}
AWS_ACCESS_KEY_ID={}
AWS_SECRET_ACCESS_KEY={}
AWS_DEFAULT_REGION={}
DO_PAT={}
SSH_KEY_PATH={}
SLACK_WEBHOOK_URL={}
SN_TESTNET_DEV_SUBNET_ID={}
SN_TESTNET_DEV_SECURITY_GROUP_ID={}
TERRAFORM_STATE_BUCKET_NAME={}
"#,
        ansible_vault_password_path,
        aws_access_key_id,
        aws_access_secret_access_key,
        aws_region,
        digital_ocean_pat,
        ssh_key_path,
        slack_webhook_url,
        sn_testnet_dev_subnet_id,
        sn_testnet_dev_security_group_id,
        terraform_state_bucket_name
    );

    std::fs::write(".env", contents.trim())?;
    Ok(())
}

fn get_ssh_key_file_candidates() -> Result<Vec<String>> {
    let ssh_dir_path = dirs_next::home_dir()
        .ok_or_else(|| Error::SetupError)?
        .join(".ssh");
    let entries = std::fs::read_dir(ssh_dir_path)?;
    let ssh_files: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|res| res.path())
        .filter(|path| path.is_file() && path.extension().unwrap_or_default() != "pub")
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    Ok(ssh_files)
}
