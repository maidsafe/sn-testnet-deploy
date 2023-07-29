pub mod error;
pub mod s3;
pub mod terraform;

use crate::s3::S3AssetRepository;
use crate::terraform::{TerraformRunner, TerraformRunnerInterface};
use color_eyre::Result;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::BufWriter;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tar::Archive;

#[derive(Debug, Clone)]
pub enum CloudProvider {
    Aws,
    DigitalOcean,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProvider::Aws => write!(f, "aws"),
            CloudProvider::DigitalOcean => write!(f, "digital-ocean"),
        }
    }
}

#[derive(Default)]
pub struct TestnetDeployBuilder {
    provider: Option<CloudProvider>,
    state_bucket_name: Option<String>,
    terraform_binary_path: Option<PathBuf>,
    working_directory_path: Option<PathBuf>,
}

impl TestnetDeployBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn provider(&mut self, provider: CloudProvider) -> &mut Self {
        self.provider = Some(provider);
        self
    }

    pub fn state_bucket_name(&mut self, state_bucket_name: String) -> &mut Self {
        self.state_bucket_name = Some(state_bucket_name);
        self
    }

    pub fn terraform_binary_path(&mut self, terraform_binary_path: PathBuf) -> &mut Self {
        self.terraform_binary_path = Some(terraform_binary_path);
        self
    }

    pub fn working_directory(&mut self, working_directory_path: PathBuf) -> &mut Self {
        self.working_directory_path = Some(working_directory_path);
        self
    }

    pub fn build(&self) -> Result<TestnetDeploy> {
        let provider = self
            .provider
            .as_ref()
            .unwrap_or(&CloudProvider::DigitalOcean);
        let state_bucket_name = match self.state_bucket_name {
            Some(ref bucket_name) => bucket_name.clone(),
            None => std::env::var("TERRAFORM_STATE_BUCKET_NAME")?,
        };
        let default_terraform_bin_path = PathBuf::from("terraform");
        let terraform_binary_path = self
            .terraform_binary_path
            .as_ref()
            .unwrap_or(&default_terraform_bin_path);
        let working_directory_path = match self.working_directory_path {
            Some(ref work_dir_path) => work_dir_path.clone(),
            None => std::env::current_dir()?.join("resources"),
        };

        let terraform_runner = TerraformRunner::new(
            terraform_binary_path.to_path_buf(),
            working_directory_path
                .join("terraform")
                .join(provider.to_string()),
            provider.clone(),
            &state_bucket_name,
        );

        let s3_repository = S3AssetRepository::new("https://sn-testnet.s3.eu-west-2.amazonaws.com");
        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_directory_path,
            provider.clone(),
            s3_repository,
        );

        Ok(testnet)
    }
}

pub struct TestnetDeploy {
    pub terraform_runner: Box<dyn TerraformRunnerInterface>,
    pub working_directory_path: PathBuf,
    pub cloud_provider: CloudProvider,
    pub s3_repository: S3AssetRepository,
    pub inventory_file_path: PathBuf,
}

impl TestnetDeploy {
    pub fn new(
        terraform_runner: Box<dyn TerraformRunnerInterface>,
        working_directory_path: PathBuf,
        cloud_provider: CloudProvider,
        s3_repository: S3AssetRepository,
    ) -> TestnetDeploy {
        let inventory_file_path = working_directory_path.join(PathBuf::from(
            "ansible/inventory/dev_inventory_digital_ocean.yml",
        ));
        TestnetDeploy {
            terraform_runner,
            working_directory_path,
            cloud_provider,
            s3_repository,
            inventory_file_path,
        }
    }

    pub async fn init(&self, name: &str) -> Result<()> {
        self.terraform_runner.init()?;
        let workspaces = self.terraform_runner.workspace_list()?;
        if !workspaces.contains(&name.to_string()) {
            self.terraform_runner.workspace_new(name)?;
        } else {
            println!("Workspace {name} already exists")
        }

        let rpc_client_path = self.working_directory_path.join("safenode_rpc_client");
        if !rpc_client_path.is_file() {
            println!("Downloading the rpc client for safenode...");
            let asset_name = "rpc_client-latest-x86_64-unknown-linux-musl.tar.gz";
            let archive_path = self.working_directory_path.join(asset_name);
            self.s3_repository
                .download_asset(asset_name, &archive_path)
                .await?;
            let archive_file = File::open(archive_path.clone())?;
            let decoder = GzDecoder::new(archive_file);
            let mut archive = Archive::new(decoder);
            let entries = archive.entries()?;
            for entry_result in entries {
                let mut entry = entry_result?;
                let mut file = BufWriter::new(File::create(
                    self.working_directory_path.join(entry.path()?),
                )?);
                std::io::copy(&mut entry, &mut file)?;
            }

            std::fs::remove_file(archive_path)?;
            let mut permissions = std::fs::metadata(&rpc_client_path)?.permissions();
            permissions.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&rpc_client_path, permissions)?;
        }

        let inventory_files = ["build", "genesis", "node"];
        for inventory_type in inventory_files.iter() {
            let src_path = self.inventory_file_path.clone();
            let dest_path = self
                .working_directory_path
                .join("ansible")
                .join("inventory")
                .join(format!(
                    ".{}_{}_inventory_digital_ocean.yml",
                    name, inventory_type
                ));
            if dest_path.is_file() {
                // In this case 'init' has already been called before and the value has been
                // replaced, so just move on.
                continue;
            }

            let mut contents = std::fs::read_to_string(&src_path)?;
            contents = contents.replace("env_value", name);
            contents = contents.replace("type_value", inventory_type);
            std::fs::write(&dest_path, contents)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::s3::S3AssetRepository;
    use crate::terraform::MockTerraformRunnerInterface;
    use assert_fs::fixture::ChildPath;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;
    use color_eyre::Result;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use httpmock::prelude::*;
    use mockall::predicate::*;
    use mockall::Sequence;
    use std::fs::{File, Metadata};
    use std::os::unix::fs::PermissionsExt;

    const RPC_CLIENT_BIN_NAME: &str = "safenode_rpc_client";

    fn setup_working_directory() -> Result<(TempDir, ChildPath)> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let working_dir = tmp_dir.child("work");
        working_dir.create_dir_all()?;
        working_dir.copy_from("resources/", &["**"])?;
        Ok((tmp_dir, working_dir))
    }

    fn create_fake_rpc_client_archive(working_dir: &ChildPath) -> Result<(ChildPath, Metadata)> {
        let temp_archive_dir = working_dir.child("setup_archive");
        let rpc_client_archive = temp_archive_dir.child("rpc_client.tar.gz");
        let fake_rpc_client_bin = temp_archive_dir.child(RPC_CLIENT_BIN_NAME);
        fake_rpc_client_bin.write_binary(b"fake code")?;

        let mut fake_rpc_client_bin_file = File::open(fake_rpc_client_bin.path())?;
        let gz_encoder = GzEncoder::new(
            File::create(rpc_client_archive.path())?,
            Compression::default(),
        );
        let mut builder = tar::Builder::new(gz_encoder);
        builder.append_file(RPC_CLIENT_BIN_NAME, &mut fake_rpc_client_bin_file)?;
        builder.into_inner()?;
        let rpc_client_archive_metadata = std::fs::metadata(rpc_client_archive.path())?;

        Ok((rpc_client_archive, rpc_client_archive_metadata))
    }

    fn setup_default_terraform_runner() -> MockTerraformRunnerInterface {
        let mut terraform_runner = MockTerraformRunnerInterface::new();
        terraform_runner.expect_init().times(1).returning(|| Ok(()));
        terraform_runner
            .expect_workspace_list()
            .times(1)
            .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
        terraform_runner
            .expect_workspace_new()
            .times(1)
            .with(eq("alpha".to_string()))
            .returning(|_| Ok(()));
        terraform_runner
    }

    fn setup_default_s3_repository(working_dir: &ChildPath) -> Result<S3AssetRepository> {
        let (rpc_client_archive, rpc_client_archive_metadata) =
            create_fake_rpc_client_archive(working_dir)?;
        let asset_server = MockServer::start();
        asset_server.mock(|when, then| {
            when.method(GET)
                .path("/rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
            then.status(200)
                .header(
                    "Content-Length",
                    rpc_client_archive_metadata.len().to_string(),
                )
                .header("Content-Type", "application/gzip")
                .body_from_file(rpc_client_archive.path().to_str().unwrap());
        });

        let s3_repository = S3AssetRepository::new(&asset_server.base_url());
        Ok(s3_repository)
    }

    #[tokio::test]
    async fn init_should_create_a_new_workspace() -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let mut terraform_runner = MockTerraformRunnerInterface::new();
        terraform_runner.expect_init().times(1).returning(|| Ok(()));
        terraform_runner
            .expect_workspace_list()
            .times(1)
            .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
        terraform_runner
            .expect_workspace_new()
            .times(1)
            .with(eq("beta".to_string()))
            .returning(|_| Ok(()));
        let s3_repository = setup_default_s3_repository(&working_dir)?;
        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );
        testnet.init("beta").await?;
        drop(tmp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn init_should_not_create_a_new_workspace_when_one_with_the_same_name_exists(
    ) -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let mut terraform_runner = MockTerraformRunnerInterface::new();
        terraform_runner.expect_init().times(1).returning(|| Ok(()));
        terraform_runner
            .expect_workspace_list()
            .times(1)
            .returning(|| {
                Ok(vec![
                    "alpha".to_string(),
                    "default".to_string(),
                    "dev".to_string(),
                ])
            });
        terraform_runner
            .expect_workspace_new()
            .times(0)
            .with(eq("alpha".to_string()))
            .returning(|_| Ok(()));

        let s3_repository = setup_default_s3_repository(&working_dir)?;
        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );
        testnet.init("alpha").await?;
        drop(tmp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn init_should_download_and_extract_the_rpc_client() -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let temp_archive_dir = working_dir.child("setup_archive");

        // Create an archive containing a fake rpc client exe, to be returned by the mock HTTP
        // server.
        let rpc_client_archive = temp_archive_dir.child("rpc_client.tar.gz");
        let fake_rpc_client_bin = temp_archive_dir.child(RPC_CLIENT_BIN_NAME);
        fake_rpc_client_bin.write_binary(b"fake code")?;
        let mut fake_rpc_client_bin_file = File::open(fake_rpc_client_bin.path())?;
        let gz_encoder = GzEncoder::new(
            File::create(rpc_client_archive.path())?,
            Compression::default(),
        );
        let mut builder = tar::Builder::new(gz_encoder);
        builder.append_file(RPC_CLIENT_BIN_NAME, &mut fake_rpc_client_bin_file)?;
        builder.into_inner()?;
        let rpc_client_archive_metadata = std::fs::metadata(rpc_client_archive.path())?;

        let asset_server = MockServer::start();
        asset_server.mock(|when, then| {
            when.method(GET)
                .path("/rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
            then.status(200)
                .header(
                    "Content-Length",
                    rpc_client_archive_metadata.len().to_string(),
                )
                .header("Content-Type", "application/gzip")
                .body_from_file(rpc_client_archive.path().to_str().unwrap());
        });
        let downloaded_safe_archive =
            working_dir.child("rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");

        let extracted_rpc_client_bin = working_dir.child(RPC_CLIENT_BIN_NAME);
        let s3_repository = S3AssetRepository::new(&asset_server.base_url());
        let terraform_runner = setup_default_terraform_runner();
        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );

        testnet.init("alpha").await?;

        downloaded_safe_archive.assert(predicates::path::missing());
        extracted_rpc_client_bin.assert(predicates::path::is_file());

        let metadata = std::fs::metadata(extracted_rpc_client_bin.path())?;
        let permissions = metadata.permissions();
        assert!(permissions.mode() & 0o100 > 0, "File is not executable");
        drop(tmp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn init_should_not_download_the_rpc_client_if_it_already_exists() -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let fake_rpc_client_bin = working_dir.child(RPC_CLIENT_BIN_NAME);
        fake_rpc_client_bin.write_binary(b"fake code")?;

        let (rpc_client_archive, rpc_client_archive_metadata) =
            create_fake_rpc_client_archive(&working_dir)?;
        let asset_server = MockServer::start();
        let mock = asset_server.mock(|when, then| {
            when.method(GET)
                .path("/rpc_client-latest-x86_64-unknown-linux-musl.tar.gz");
            then.status(200)
                .header(
                    "Content-Length",
                    rpc_client_archive_metadata.len().to_string(),
                )
                .header("Content-Type", "application/gzip")
                .body_from_file(rpc_client_archive.path().to_str().unwrap());
        });
        let s3_repository = S3AssetRepository::new(&asset_server.base_url());

        let terraform_runner = setup_default_terraform_runner();
        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );
        testnet.init("alpha").await?;

        mock.assert_hits(0);
        drop(tmp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn init_should_generate_ansible_inventory_for_digital_ocean_for_the_new_testnet(
    ) -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let s3_repository = setup_default_s3_repository(&working_dir)?;
        let terraform_runner = setup_default_terraform_runner();

        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );

        testnet.init("alpha").await?;

        let inventory_files = ["build", "genesis", "node"];
        for inventory_type in inventory_files.iter() {
            let inventory_file = working_dir.child(format!(
                "ansible/inventory/.{}_{}_inventory_digital_ocean.yml",
                "alpha", inventory_type
            ));
            inventory_file.assert(predicates::path::is_file());

            let contents = std::fs::read_to_string(inventory_file.path())?;
            assert!(contents.contains("alpha"));
            assert!(contents.contains(inventory_type));
        }
        drop(tmp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn init_should_not_overwrite_generated_inventory() -> Result<()> {
        let (tmp_dir, working_dir) = setup_working_directory()?;
        let s3_repository = setup_default_s3_repository(&working_dir)?;
        let mut terraform_runner = MockTerraformRunnerInterface::new();
        let mut seq = Sequence::new();

        terraform_runner.expect_init().times(2).returning(|| Ok(()));
        terraform_runner
            .expect_workspace_list()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(vec!["default".to_string(), "dev".to_string()]));
        terraform_runner
            .expect_workspace_new()
            .times(1)
            .with(eq("alpha".to_string()))
            .returning(|_| Ok(()));
        terraform_runner
            .expect_workspace_list()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| {
                Ok(vec![
                    "alpha".to_string(),
                    "default".to_string(),
                    "dev".to_string(),
                ])
            });

        let testnet = TestnetDeploy::new(
            Box::new(terraform_runner),
            working_dir.to_path_buf(),
            CloudProvider::DigitalOcean,
            s3_repository,
        );

        testnet.init("alpha").await?;
        testnet.init("alpha").await?; // this should be idempotent

        let inventory_files = ["build", "genesis", "node"];
        for inventory_type in inventory_files.iter() {
            let inventory_file = working_dir.child(format!(
                "ansible/inventory/.{}_{}_inventory_digital_ocean.yml",
                "alpha", inventory_type
            ));
            inventory_file.assert(predicates::path::is_file());

            let contents = std::fs::read_to_string(inventory_file.path())?;
            assert!(contents.contains("alpha"));
            assert!(contents.contains(inventory_type));
        }
        drop(tmp_dir);
        Ok(())
    }
}
