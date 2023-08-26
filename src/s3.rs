// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use async_recursion::async_recursion;
use async_trait::async_trait;
use aws_sdk_s3::Client;
#[cfg(test)]
use mockall::automock;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

/// Provides an interface for using S3.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// ssh process.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait S3RepositoryInterface {
    async fn download_object(&self, object_key: &str, dest_path: &Path) -> Result<()>;
    async fn download_folder(&self, folder_path: &str, dest_path: &Path) -> Result<()>;
    async fn delete_folder(&self, folder_path: &str) -> Result<()>;
    async fn folder_exists(&self, folder_path: &str) -> Result<bool>;
}

pub struct S3Repository {
    pub bucket_name: String,
}

#[async_trait]
impl S3RepositoryInterface for S3Repository {
    async fn download_object(&self, object_key: &str, dest_path: &Path) -> Result<()> {
        let conf = aws_config::from_env().region("eu-west-2").load().await;
        let client = Client::new(&conf);
        self.retrieve_object(&client, object_key, &dest_path.to_path_buf())
            .await?;
        Ok(())
    }

    async fn download_folder(&self, folder_path: &str, dest_path: &Path) -> Result<()> {
        let conf = aws_config::from_env().region("eu-west-2").load().await;
        let client = Client::new(&conf);
        tokio::fs::create_dir_all(dest_path).await?;
        self.list_and_retrieve(&client, folder_path, &dest_path.to_path_buf())
            .await?;
        Ok(())
    }

    async fn delete_folder(&self, folder_path: &str) -> Result<()> {
        let conf = aws_config::from_env().region("eu-west-2").load().await;
        let client = Client::new(&conf);
        self.list_and_delete(&client, folder_path).await?;
        Ok(())
    }

    async fn folder_exists(&self, folder_path: &str) -> Result<bool> {
        let conf = aws_config::from_env().region("eu-west-2").load().await;
        let client = Client::new(&conf);
        let folder = if folder_path.ends_with('/') {
            folder_path.to_string()
        } else {
            format!("{}/", folder_path)
        };
        let output = client
            .list_objects_v2()
            .bucket(self.bucket_name.clone())
            .prefix(folder)
            .delimiter("/".to_string())
            .send()
            .await
            .map_err(|_| Error::ListS3ObjectsError(folder_path.to_string()))?;
        Ok(!output.contents().unwrap_or_default().is_empty())
    }
}

impl S3Repository {
    pub fn new(bucket_name: &str) -> Self {
        Self {
            bucket_name: bucket_name.to_string(),
        }
    }

    #[async_recursion]
    async fn list_and_retrieve(
        &self,
        client: &Client,
        prefix: &str,
        root_path: &PathBuf,
    ) -> Result<(), Error> {
        let output = client
            .list_objects_v2()
            .bucket(self.bucket_name.clone())
            .prefix(prefix)
            .delimiter("/".to_string())
            .send()
            .await
            .map_err(|_| Error::ListS3ObjectsError(prefix.to_string()))?;

        // So-called 'common prefixes' are subdirectories.
        if let Some(common_prefixes) = output.common_prefixes {
            for cp in common_prefixes {
                let next_prefix = cp.prefix.unwrap();
                self.list_and_retrieve(client, &next_prefix, root_path)
                    .await?;
            }
        }

        if let Some(objects) = output.contents {
            for object in objects {
                let object_key = object.key.unwrap();
                let mut dest_file_path = root_path.clone();
                dest_file_path.push(&object_key);
                if dest_file_path.exists() {
                    println!("Has already been retrieved in a previous sync.");
                    continue;
                }

                self.retrieve_object(client, &object_key, &dest_file_path)
                    .await?;
            }
        }

        Ok(())
    }

    #[async_recursion]
    async fn list_and_delete(&self, client: &Client, prefix: &str) -> Result<(), Error> {
        let output = client
            .list_objects_v2()
            .bucket(self.bucket_name.clone())
            .prefix(prefix)
            .delimiter("/".to_string())
            .send()
            .await
            .map_err(|_| Error::ListS3ObjectsError(prefix.to_string()))?;

        // So-called 'common prefixes' are subdirectories.
        if let Some(common_prefixes) = output.common_prefixes {
            for cp in common_prefixes {
                let next_prefix = cp.prefix.unwrap();
                self.list_and_delete(client, &next_prefix).await?;
            }
        }

        if let Some(objects) = output.contents {
            for object in objects {
                let object_key = object.key.unwrap();
                self.delete_object(client, &object_key).await?;
            }
        }

        Ok(())
    }

    async fn retrieve_object(
        &self,
        client: &Client,
        object_key: &str,
        dest_path: &PathBuf,
    ) -> Result<()> {
        println!("Retrieving {object_key} from S3...");
        let mut resp = client
            .get_object()
            .bucket(self.bucket_name.clone())
            .key(object_key)
            .send()
            .await
            .map_err(|_| {
                Error::GetS3ObjectError(object_key.to_string(), self.bucket_name.clone())
            })?;

        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        let mut file = tokio::fs::File::create(&dest_path).await?;
        while let Some(bytes) = resp
            .body
            .try_next()
            .await
            .map_err(|_| Error::S3ByteStreamError)?
        {
            file.write_all(&bytes).await?;
        }

        println!("Saved at {}", dest_path.to_string_lossy());
        Ok(())
    }

    async fn delete_object(&self, client: &Client, object_key: &str) -> Result<()> {
        println!("Deleting {object_key} from S3...");
        client
            .delete_object()
            .bucket(self.bucket_name.clone())
            .key(object_key)
            .send()
            .await
            .map_err(|_| {
                Error::DeleteS3ObjectError(object_key.to_string(), self.bucket_name.clone())
            })?;
        Ok(())
    }
}
