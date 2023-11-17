// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use crate::s3::{S3Repository, S3RepositoryInterface};
use crate::{run_external_command, TestnetDeploy};
use fs_extra::dir::{copy, remove, CopyOptions};
use futures::future::join_all;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

impl TestnetDeploy {
    /// Run an Ansible playbook to copy the logs from all the machines in the inventory.
    ///
    /// It needs to be part of `TestnetDeploy` because the Ansible runner is already setup in that
    /// context.
    pub async fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        let log_dest = PathBuf::from(".").join("logs").join(name);
        // if log_dest.exists() {
        //     println!("Removing existing {} directory", log_dest.to_string_lossy());
        //     remove(log_dest.clone())?;
        // }
        std::fs::create_dir_all(&log_dest)?;
        // get the absolute path
        let log_abs_dest = std::fs::canonicalize(log_dest)?;

        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        // The ansible runner will have its working directory set to this location. We need the
        // same here to test the inventory paths, which are relative to the `ansible` directory.
        let ansible_dir_path = self.working_directory_path.join("ansible");
        std::env::set_current_dir(ansible_dir_path.clone())?;

        // Somehow it might be possible that the workspace wasn't cleared out, but the environment
        // was actually torn down and the generated inventory files were deleted. If the files
        // don't exist, we can reasonably consider the environment non-existent.
        let genesis_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_genesis_inventory_digital_ocean.yml"));
        let build_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_build_inventory_digital_ocean.yml"));
        let remaining_nodes_inventory_path =
            PathBuf::from("inventory").join(format!(".{name}_node_inventory_digital_ocean.yml"));
        if !genesis_inventory_path.exists()
            || !build_inventory_path.exists()
            || !remaining_nodes_inventory_path.exists()
        {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }

        let mut all_node_inventory = self.ansible_runner.inventory_list(genesis_inventory_path)?;
        all_node_inventory.extend(
            self.ansible_runner
                .inventory_list(remaining_nodes_inventory_path)?,
        );

        // goto the resource dir
        std::env::set_current_dir(self.working_directory_path.clone())?;

        all_node_inventory.par_iter().for_each(|(vm_name, _)| {
            let vm_path = log_abs_dest.join(vm_name);
            let _ = std::fs::create_dir_all(vm_path);
        });

        // rysnc
        let mut rsync_working_dir = log_abs_dest.clone();
        rsync_working_dir.pop();

        let mut rsync_args = vec![
            "--compress".to_string(),
            "--archive".to_string(),
            "--prune-empty-dirs".to_string(),
            "--verbose".to_string(),
            "--verbose".to_string(),
        ];
        if !resources_only {
            // to filter the log files
            rsync_args.extend(vec![
                "--filter=+ */".to_string(),     // Include all directories for traversal
                "--filter=+ *.log*".to_string(), // Include all *.log* files
                "--filter=- *".to_string(),      // Exclude all other files
            ])
        } else {
            // to filter the log files
            rsync_args.extend(vec![
                "--filter=+ */".to_string(), // Include all directories for traversal
                "--filter=+ resource-usage.log".to_string(), // Include all *.log* files
                "--filter=- *".to_string(),  // Exclude all other files
            ])
        }
        // add the ssh details
        rsync_args.extend(vec![
            "-e".to_string(),
            format!(
                "ssh -i {}",
                self.ssh_client
                    .get_private_key_path()
                    .to_string_lossy()
                    .as_ref()
            ),
        ]);

        let mut handles = Vec::new();
        for (vm_name, ip_address) in all_node_inventory {
            let vm_path = log_abs_dest.join(&vm_name);

            let ssh_client_clone = self.ssh_client.clone_box();
            let rsync_working_dir_clone = rsync_working_dir.clone();
            let mut rsync_args_clone = rsync_args.clone();
            let handle = tokio::spawn(async move {
                if !resources_only {
                    rsync_args_clone.push(format!("safe@{ip_address}:.local/share/safe/node/"));
                } else {
                    rsync_args_clone.push(format!(
                        "safe@{ip_address}:.local/share/safe/node/gggggggggggg"
                    ));
                }
                rsync_args_clone.push(vm_path.to_string_lossy().to_string());

                // if !resources_only {
                //     println!("Copying logs to tmpdir for {vm_name:?}");
                //     let _op = ssh_client_clone.run_script(
                //         ip_address,
                //         "safe",
                //         PathBuf::from("scripts").join("copy_node_logs.sh"),
                //         true,
                //     )?;
                // }

                println!("Copying logs to our machine {vm_name:?}");
                run_external_command(
                    PathBuf::from("rsync"),
                    rsync_working_dir_clone,
                    rsync_args_clone,
                    true,
                )?;

                println!("Copied logs for {vm_name:?}");

                Ok::<(), Error>(())
            });
            handles.push(handle);
        }
        for result in join_all(handles).await {
            match result? {
                Ok(_) => {}
                Err(err) => println!("Failed to SSH with err: {err:?}"),
            }
        }

        Ok(())
    }
}

pub async fn get_logs(name: &str) -> Result<()> {
    let dest_path = std::env::current_dir()?.join("logs").join(name);
    tokio::fs::create_dir_all(dest_path.clone()).await?;
    let s3_repository = S3Repository {};
    s3_repository
        .download_folder("sn-testnet", &format!("testnet-logs/{name}"), &dest_path)
        .await?;
    Ok(())
}

pub async fn reassemble_logs(name: &str) -> Result<()> {
    let src = PathBuf::from(".").join("logs").join(name);
    if !src.exists() {
        return Err(Error::LogsNotRetrievedError(name.to_string()));
    }
    let dest = PathBuf::from(".")
        .join("logs")
        .join(format!("{name}-reassembled"));
    if dest.exists() {
        println!("Removing previous {name}-reassembled directory");
        remove(dest.clone())?;
    }

    std::fs::create_dir_all(&dest)?;
    let mut options = CopyOptions::new();
    options.overwrite = true;
    copy(src.clone(), dest.clone(), &options)?;

    visit_dirs(&dest, &process_part_files, &src, &dest)?;
    Ok(())
}

pub async fn rm_logs(name: &str) -> Result<()> {
    let s3_repository = S3Repository {};
    s3_repository
        .delete_folder("sn-testnet", &format!("testnet-logs/{name}"))
        .await?;
    Ok(())
}

fn process_part_files(dir_path: &Path, source_root: &PathBuf, dest_root: &PathBuf) -> Result<()> {
    let reassembled_dir_path = if dir_path == dest_root {
        dest_root.clone()
    } else {
        dest_root.join(dir_path.strip_prefix(source_root)?)
    };
    std::fs::create_dir_all(&reassembled_dir_path)?;

    let entries: Vec<_> = std::fs::read_dir(dir_path)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;

    let mut part_files: Vec<_> = entries
        .iter()
        .filter(|path| path.is_file() && path.to_string_lossy().contains("part"))
        .collect();

    part_files.sort_by_key(|a| {
        a.file_stem()
            .unwrap()
            .to_string_lossy()
            .split(".part")
            .nth(1)
            .unwrap()
            .parse::<u32>()
            .unwrap()
    });

    if part_files.is_empty() {
        return Ok(());
    }

    let output_file_path = reassembled_dir_path.join("reassembled.log");
    println!("Creating reassembled file at {output_file_path:#?}");
    let mut output_file = File::create(&output_file_path)?;
    for part_file in part_files.iter() {
        let mut part_content = String::new();
        File::open(part_file)?.read_to_string(&mut part_content)?;

        // For some reason logstash writes "\n" as a literal string rather than a newline
        // character.
        part_content = part_content.replace("\\n", "\n");

        let mut cursor = Cursor::new(part_content);
        std::io::copy(&mut cursor, &mut output_file)?;
        std::fs::remove_file(part_file)?;
    }

    Ok(())
}

fn visit_dirs(
    dir: &Path,
    cb: &dyn Fn(&Path, &PathBuf, &PathBuf) -> Result<()>,
    source_root: &PathBuf,
    dest_root: &PathBuf,
) -> Result<()> {
    if dir.is_dir() {
        cb(dir, source_root, dest_root)?;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb, dest_root, dest_root)?;
            }
        }
    }
    Ok(())
}
