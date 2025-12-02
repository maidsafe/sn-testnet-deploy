// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::{
    error::{Error, Result},
    get_progress_bar,
    inventory::VirtualMachine,
    run_external_command,
    s3::S3Repository,
    TestnetDeployer,
};
use fs_extra::dir::{copy, remove, CopyOptions};
use log::debug;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const DEFAULT_RSYNC_ARGS: [&str; 8] = [
    "--compress",
    "--archive",
    "--prune-empty-dirs",
    "--verbose",
    "--verbose",
    "--filter=+ */",     // Include all directories for traversal
    "--filter=+ *.log*", // Include all *.log* files
    "--filter=- *",      // Exclude all other files
];

const NODE_LOG_DIR: &str = "/mnt/antnode-storage/log/";

impl TestnetDeployer {
    pub fn rsync_logs(
        &self,
        name: &str,
        vm_filter: Option<String>,
        disable_client_logs: bool,
    ) -> Result<()> {
        // take root_dir at the top as `get_all_node_inventory` changes the working dir.
        let root_dir = std::env::current_dir()?;

        let mut all_inventory = vec![];
        if !disable_client_logs {
            all_inventory.extend(self.get_client_inventory(name)?);
        }

        all_inventory.extend(self.get_all_node_inventory(name)?);

        let all_inventory = if let Some(filter) = vm_filter {
            all_inventory
                .into_iter()
                .filter(|vm| vm.name.contains(&filter))
                .collect()
        } else {
            all_inventory
        };

        let log_base_dir = create_initial_log_dir_setup(&root_dir, name, &all_inventory)?;

        // We might use the script, so goto the resource dir.
        std::env::set_current_dir(self.working_directory_path.clone())?;
        println!("Starting to rsync the log files");
        let progress_bar = get_progress_bar(all_inventory.len() as u64)?;

        let rsync_args = all_inventory
            .iter()
            .map(|vm| {
                let args = if vm.name.contains("symmetric") {
                    let args = self.construct_symmetric_private_node_args(vm, &log_base_dir)?;
                    debug!("Using symmetric rsync args for {:?}", vm.name);
                    debug!("Args for {}: {:?}", vm.name, args);
                    args
                } else if vm.name.contains("full-cone") {
                    let args = self.construct_full_cone_private_node_args(vm, &log_base_dir)?;
                    debug!("Using symmetric rsync args for {:?}", vm.name);
                    debug!("Args for {}: {:?}", vm.name, args);
                    args
                } else if vm.name.contains("ant-client") {
                    let args = self.construct_client_args(vm, &log_base_dir);
                    debug!("Using Client rsync args for {:?} ", vm.name);
                    debug!("Args for {}: {:?}", vm.name, args);
                    args
                } else {
                    let args = self.construct_public_node_args(vm, &log_base_dir);
                    debug!("Using public rsync args for {:?}", vm.name);
                    debug!("Args for {}: {:?}", vm.name, args);
                    args
                };

                Ok((vm.clone(), args))
            })
            .collect::<Result<Vec<_>>>()?;

        let failed_inventory = rsync_args
            .par_iter()
            .filter_map(|(vm, args)| {
                if let Err(err) = Self::run_rsync(vm, args) {
                    println!(
                        "Failed to rsync. Retrying it after ssh-keygen {:?} : {} with err: {err:?}",
                        vm.name, vm.public_ip_addr
                    );
                    return Some((vm.clone(), args.clone()));
                }
                progress_bar.inc(1);
                None
            })
            .collect::<Vec<_>>();

        // try ssh-keygen for the failed inventory and try to rsync again
        failed_inventory
            .into_par_iter()
            .for_each(|(vm, args)| {
                debug!("Trying to ssh-keygen for {:?} : {}", vm.name, vm.public_ip_addr);
                if let Err(err) = run_external_command(
                    PathBuf::from("ssh-keygen"),
                    PathBuf::from("."),
                    vec!["-R".to_string(), format!("{}", vm.public_ip_addr)],
                    false,
                    false,
                ) {
                    println!("Failed to ssh-keygen {:?} : {} with err: {err:?}", vm.name, vm.public_ip_addr);
                } else if let Err(err) =
                    Self::run_rsync(&vm, &args)
                {
                    println!("Failed to rsync even after ssh-keygen. Could not obtain logs for {:?} : {} with err: {err:?}", vm.name, vm.public_ip_addr);
                }
                progress_bar.inc(1);
            });
        progress_bar.finish_and_clear();
        println!("Rsync completed!");
        Ok(())
    }

    fn construct_client_args(&self, vm: &VirtualMachine, log_base_dir: &Path) -> Vec<String> {
        let vm_path = log_base_dir.join(&vm.name);
        let mut rsync_args = DEFAULT_RSYNC_ARGS
            .iter()
            .map(|str| str.to_string())
            .collect::<Vec<String>>();

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
            format!("root@{}:/mnt/client/log/", vm.public_ip_addr),
            vm_path.to_string_lossy().to_string(),
        ]);

        rsync_args
    }

    fn construct_public_node_args(&self, vm: &VirtualMachine, log_base_dir: &Path) -> Vec<String> {
        let vm_path = log_base_dir.join(&vm.name);
        let mut rsync_args = DEFAULT_RSYNC_ARGS
            .iter()
            .map(|str| str.to_string())
            .collect::<Vec<String>>();

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
            format!("root@{}:{NODE_LOG_DIR}", vm.public_ip_addr),
            vm_path.to_string_lossy().to_string(),
        ]);

        rsync_args
    }

    fn construct_full_cone_private_node_args(
        &self,
        private_vm: &VirtualMachine,
        log_base_dir: &Path,
    ) -> Result<Vec<String>> {
        let vm_path = log_base_dir.join(&private_vm.name);

        let mut rsync_args = DEFAULT_RSYNC_ARGS
            .iter()
            .map(|str| str.to_string())
            .collect::<Vec<String>>();

        let read_lock = self.ssh_client.routed_vms.read().map_err(|err| {
            log::error!("Failed to set routed VMs: {err}");
            Error::SshSettingsRwLockError
        })?;
        let (_, gateway_ip) = read_lock
            .as_ref()
            .and_then(|routed_vms| {
                routed_vms.find_full_cone_nat_routed_node(&private_vm.public_ip_addr)
            })
            .ok_or(Error::RoutedVmNotFound(private_vm.public_ip_addr))?;

        rsync_args.extend(vec![
            "-e".to_string(),
            format!(
                "ssh -i {} -q -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=30",
                self.ssh_client
                    .get_private_key_path()
                    .to_string_lossy()
                    .as_ref()
            ),
            format!("root@{}:{NODE_LOG_DIR}", gateway_ip),
            vm_path.to_string_lossy().to_string(),
        ]);

        Ok(rsync_args)
    }

    fn construct_symmetric_private_node_args(
        &self,
        private_vm: &VirtualMachine,
        log_base_dir: &Path,
    ) -> Result<Vec<String>> {
        let vm_path = log_base_dir.join(&private_vm.name);

        let mut rsync_args = DEFAULT_RSYNC_ARGS
            .iter()
            .map(|str| str.to_string())
            .collect::<Vec<String>>();

        let read_lock = self.ssh_client.routed_vms.read().map_err(|err| {
            log::error!("Failed to set routed VMs: {err}");
            Error::SshSettingsRwLockError
        })?;
        let (_, gateway_ip) = read_lock
            .as_ref()
            .and_then(|routed_vms| {
                routed_vms.find_symmetric_nat_routed_node(&private_vm.public_ip_addr)
            })
            .ok_or(Error::RoutedVmNotFound(private_vm.public_ip_addr))?;

        rsync_args.extend(vec![
            "-e".to_string(),
            format!(
                "ssh -i {} -q -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=30 -o ProxyCommand='ssh -o StrictHostKeyChecking=no -o BatchMode=yes root@{gateway_ip} -W %h:%p -i {}'",
                self.ssh_client
                    .get_private_key_path()
                    .to_string_lossy()
                    .as_ref(),
                self.ssh_client
                    .get_private_key_path()
                    .to_string_lossy()
                    .as_ref(),
            ),
            format!("root@{}:{NODE_LOG_DIR}", private_vm.private_ip_addr),
            vm_path.to_string_lossy().to_string(),
        ]);

        Ok(rsync_args)
    }
    fn run_rsync(vm: &VirtualMachine, rsync_args: &[String]) -> Result<()> {
        debug!(
            "Rsync logs to our machine for {:?} : {}",
            vm.name, vm.public_ip_addr
        );
        run_external_command(
            PathBuf::from("rsync"),
            PathBuf::from("."),
            rsync_args.to_vec(),
            true,
            false,
        )?;

        debug!(
            "Finished rsync for for {:?} : {}",
            vm.name, vm.public_ip_addr
        );
        Ok(())
    }

    pub fn ripgrep_logs(&self, name: &str, rg_args: &str) -> Result<()> {
        // take root_dir at the top as `get_all_node_inventory` changes the working dir.
        let root_dir = std::env::current_dir()?;
        let all_node_inventory = self.get_all_node_inventory(name)?;
        let log_abs_dest = create_initial_log_dir_setup(&root_dir, name, &all_node_inventory)?;

        let rg_cmd = format!("rg {rg_args} /mnt/antnode-storage/log//");
        println!("Running ripgrep with command: {rg_cmd}");

        // Get current date and time
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%S").to_string();
        let progress_bar = get_progress_bar(all_node_inventory.len() as u64)?;
        let _failed_inventory = all_node_inventory
            .par_iter()
            .filter_map(|vm| {
                let op =
                    match self
                        .ssh_client
                        .run_command(&vm.public_ip_addr, "root", &rg_cmd, true)
                    {
                        Ok(output) => {
                            match Self::store_rg_output(
                                &timestamp,
                                &rg_cmd,
                                &output,
                                &log_abs_dest,
                                &vm.name,
                            ) {
                                Ok(_) => None,
                                Err(err) => {
                                    println!(
                                        "Failed store output for {:?} with: {err:?}",
                                        vm.public_ip_addr
                                    );
                                    Some(vm)
                                }
                            }
                        }
                        Err(Error::ExternalCommandRunFailed {
                            binary,
                            exit_status,
                        }) => {
                            if let Some(1) = exit_status.code() {
                                debug!("No matches found for {:?}", vm.public_ip_addr);
                                match Self::store_rg_output(
                                    &timestamp,
                                    &rg_cmd,
                                    &["No matches found".to_string()],
                                    &log_abs_dest,
                                    &vm.name,
                                ) {
                                    Ok(_) => None,
                                    Err(err) => {
                                        println!(
                                            "Failed store output for {:?} with: {err:?}",
                                            vm.public_ip_addr
                                        );
                                        Some(vm)
                                    }
                                }
                            } else {
                                println!(
                                    "Failed to run rg query for {:?} with: {binary}",
                                    vm.public_ip_addr
                                );
                                Some(vm)
                            }
                        }
                        Err(err) => {
                            println!(
                                "Failed to run rg query for {:?} with: {err:?}",
                                vm.public_ip_addr
                            );
                            Some(vm)
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

    fn store_rg_output(
        timestamp: &str,
        cmd: &str,
        output: &[String],
        log_abs_dest: &Path,
        vm_name: &str,
    ) -> Result<()> {
        std::fs::create_dir_all(log_abs_dest.join(vm_name))?;

        let mut file = File::create(
            log_abs_dest
                .join(vm_name)
                .join(format!("rg-{timestamp}.log")),
        )?;

        writeln!(file, "Command: {cmd}")?;

        for line in output {
            writeln!(file, "{line}")?;
        }

        Ok(())
    }

    /// Run an Ansible playbook to copy the logs from all the machines in the inventory.
    ///
    /// It needs to be part of `TestnetDeploy` because the Ansible runner is already setup in that
    /// context.
    pub fn copy_logs(&self, name: &str, resources_only: bool) -> Result<()> {
        let dest = PathBuf::from(".").join("logs").join(name);
        if dest.exists() {
            println!("Removing existing {} directory", dest.to_string_lossy());
            remove(dest.clone())?;
        }
        std::fs::create_dir_all(&dest)?;
        self.ansible_provisioner.copy_logs(name, resources_only)?;
        Ok(())
    }

    // Return the list of all the node machines.
    fn get_all_node_inventory(&self, name: &str) -> Result<Vec<VirtualMachine>> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }
        self.ansible_provisioner.get_all_node_inventory()
    }

    fn get_client_inventory(&self, name: &str) -> Result<Vec<VirtualMachine>> {
        let environments = self.terraform_runner.workspace_list()?;
        if !environments.contains(&name.to_string()) {
            return Err(Error::EnvironmentDoesNotExist(name.to_string()));
        }
        self.ansible_provisioner.get_client_inventory()
    }
}

pub async fn get_logs(name: &str) -> Result<()> {
    let dest_path = std::env::current_dir()?.join("logs").join(name);
    std::fs::create_dir_all(dest_path.clone())?;
    let s3_repository = S3Repository {};
    s3_repository
        .download_folder("sn-testnet", &format!("testnet-logs/{name}"), &dest_path)
        .await?;
    Ok(())
}

pub fn reassemble_logs(name: &str) -> Result<()> {
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
    all_node_inventory: &[VirtualMachine],
) -> Result<PathBuf> {
    let log_dest = root_dir.join("logs").join(name);
    if !log_dest.exists() {
        std::fs::create_dir_all(&log_dest)?;
    }
    // Get the absolute path here. We might be changing the current_dir and we don't want to run into problems.
    let log_abs_dest = std::fs::canonicalize(log_dest)?;
    // Create a log dir per VM
    all_node_inventory.par_iter().for_each(|vm| {
        let vm_path = log_abs_dest.join(&vm.name);
        let _ = std::fs::create_dir_all(vm_path);
    });
    Ok(log_abs_dest)
}
