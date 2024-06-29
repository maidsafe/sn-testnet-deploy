// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    get_progress_bar, run_external_command,
    s3::S3Repository,
    TestnetDeployer,
};
use fs_extra::dir::{copy, remove, CopyOptions};
use log::debug;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

impl TestnetDeployer {
    pub async fn rsync_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        // take root_dir at the top as `get_all_node_inventory` changes the working dir.
        let root_dir = std::env::current_dir()?;
        let all_node_inventory = self.get_all_node_inventory(name).await?;
        let log_abs_dest = create_initial_log_dir_setup(&root_dir, name, &all_node_inventory)?;

        // Rsync args
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
            // to filter the resource usage files
            rsync_args.extend(vec![
                "--filter=+ */".to_string(), // Include all directories for traversal
                "--filter=+ resource-usage.log".to_string(), // Include only the resource-usage logs
                "--filter=- *".to_string(),  // Exclude all other files
            ])
        }
        // Add the ssh details
        // TODO: SSH limits the connections/instances to 10 at a time. Changing /etc/ssh/sshd_config, doesn't work?
        // How to bypass this?
        rsync_args.extend(vec![
            "-e".to_string(),
            format!(
                "ssh -i {} -q -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=30",
                self.ssh_client
                    .get_private_key_path()
                    .to_string_lossy()
                    .as_ref()
            ),
        ]);

        // We might use the script, so goto the resource dir.
        std::env::set_current_dir(self.working_directory_path.clone())?;
        println!("Starting to rsync the log files");
        let progress_bar = get_progress_bar(all_node_inventory.len() as u64)?;

        let failed_inventory = all_node_inventory
            .par_iter()
            .filter_map(|(vm_name, ip_address)| {
                if let Err(err) = Self::run_rsync(vm_name, ip_address, &log_abs_dest, &rsync_args) {
                    println!("Failed to rsync. Retrying it after ssh-keygen {vm_name:?} : {ip_address} with err: {err:?}");
                    return Some((vm_name, ip_address));
                }
                progress_bar.inc(1);
                None
            });

        // try ssh-keygen for the failed inventory and try to rsync again
        failed_inventory
            .into_par_iter()
            .for_each(|(vm_name, ip_address)| {
                debug!("Trying to ssh-keygen for {vm_name:?} : {ip_address}");
                if let Err(err) = run_external_command(
                    PathBuf::from("ssh-keygen"),
                    PathBuf::from("."),
                    vec!["-R".to_string(), format!("{ip_address}")],
                    false,
                    false,
                ) {
                    println!("Failed to ssh-keygen {vm_name:?} : {ip_address} with err: {err:?}");
                } else if let Err(err) =
                    Self::run_rsync(vm_name, ip_address, &log_abs_dest, &rsync_args)
                {
                    println!("Failed to rsync even after ssh-keygen. Could not obtain logs for {vm_name:?} : {ip_address} with err: {err:?}");
                }
                progress_bar.inc(1);
            });
        progress_bar.finish_and_clear();
        println!("Rsync completed!");
        Ok(())
    }

    fn run_rsync(
        vm_name: &String,
        ip_address: &IpAddr,
        log_abs_dest: &Path,
        rsync_args: &[String],
    ) -> Result<()> {
        let vm_path = log_abs_dest.join(vm_name);
        let mut rsync_args_clone = rsync_args.to_vec();

        rsync_args_clone.push(format!("safe@{ip_address}:/var/log/safenode/"));
        rsync_args_clone.push(vm_path.to_string_lossy().to_string());

        debug!("Rsync logs to our machine for {vm_name:?} : {ip_address}");
        run_external_command(
            PathBuf::from("rsync"),
            PathBuf::from("."),
            rsync_args_clone.clone(),
            true,
            false,
        )?;

        debug!("Finished rsync for for {vm_name:?} : {ip_address}");
        Ok(())
    }

    pub async fn ripgrep_logs(&self, name: &str, rg_args: &str) -> Result<()> {
        // take root_dir at the top as `get_all_node_inventory` changes the working dir.
        let root_dir = std::env::current_dir()?;
        let all_node_inventory = self.get_all_node_inventory(name).await?;
        let log_abs_dest = create_initial_log_dir_setup(&root_dir, name, &all_node_inventory)?;

        let rg_cmd = format!("rg {rg_args} /var/log/safenode/");
        println!("Running ripgrep with command: {rg_cmd}");

        let progress_bar = get_progress_bar(all_node_inventory.len() as u64)?;
        let _failed_inventory = all_node_inventory
            .par_iter()
            .filter_map(|(vm_name, ip_address)| {
                let op = match self
                    .ssh_client
                    .run_command(ip_address, "safe", &rg_cmd, true)
                {
                    Ok(output) => match Self::store_rg_output(&output, &log_abs_dest, vm_name) {
                        Ok(_) => None,
                        Err(err) => {
                            println!("Failed store output for {ip_address:?} with: {err:?}");
                            Some((vm_name, ip_address))
                        }
                    },
                    Err(err) => {
                        println!("Failed to run rg query for {ip_address:?} with: {err:?}");
                        Some((vm_name, ip_address))
                    }
                };
                progress_bar.inc(1);
                op
            })
            .collect::<Vec<_>>();

        progress_bar.finish_and_clear();
        println!("Ripgrep completed!");

        Ok(())
    }

    fn store_rg_output(output: &[String], log_abs_dest: &Path, vm_name: &str) -> Result<()> {
        std::fs::create_dir_all(log_abs_dest.join(vm_name))?;

        // Get current date and time
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%S").to_string();

        let mut file = File::create(
            log_abs_dest
                .join(vm_name)
                .join(format!("rg-{timestamp}.log")),
        )?;

        for line in output {
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }

    /// Run an Ansible playbook to copy the logs from all the machines in the inventory.
    ///
    /// It needs to be part of `TestnetDeploy` because the Ansible runner is already setup in that
    /// context.
    pub async fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        let dest = PathBuf::from(".").join("logs").join(name);
        if dest.exists() {
            println!("Removing existing {} directory", dest.to_string_lossy());
            remove(dest.clone())?;
        }
        std::fs::create_dir_all(&dest)?;
        self.ansible_provisioner
            .copy_logs(name, resources_only)
            .await?;
        Ok(())
    }

    // Return the list of all the node machines.
    async fn get_all_node_inventory(&self, name: &str) -> Result<Vec<(String, IpAddr)>> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }
        self.ansible_provisioner.get_all_node_inventory().await
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

// Create the log dirs for all the machines. Returns the absolute path to the `logs/name`
fn create_initial_log_dir_setup(
    root_dir: &Path,
    name: &str,
    all_node_inventory: &[(String, IpAddr)],
) -> Result<PathBuf> {
    let log_dest = root_dir.join("logs").join(name);
    if !log_dest.exists() {
        std::fs::create_dir_all(&log_dest)?;
    }
    // Get the absolute path here. We might be changing the current_dir and we don't want to run into problems.
    let log_abs_dest = std::fs::canonicalize(log_dest)?;
    // Create a log dir per VM
    all_node_inventory.par_iter().for_each(|(vm_name, _)| {
        let vm_path = log_abs_dest.join(vm_name);
        let _ = std::fs::create_dir_all(vm_path);
    });
    Ok(log_abs_dest)
}
