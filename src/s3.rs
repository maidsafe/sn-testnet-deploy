// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use async_trait::async_trait;
use aws_sdk_s3::Client;
#[cfg(test)]
use mockall::automock;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

/// Provides an interface for using the SSH client.
///
/// This trait exists for unit testing: it enables testing behaviour without actually calling the
/// ssh process.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait S3RepositoryInterface {
    async fn download_object(&self, object_key: &str, dest_path: &Path) -> Result<()>;
}

pub struct S3AssetRepository {
    pub bucket_name: String,
}

#[async_trait]
impl S3RepositoryInterface for S3AssetRepository {
    async fn download_object(&self, object_key: &str, dest_path: &Path) -> Result<()> {
        let conf = aws_config::from_env().region("eu-west-2").load().await;
        let client = Client::new(&conf);

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
}

impl S3AssetRepository {
    pub fn new(bucket_name: &str) -> Self {
        Self {
            bucket_name: bucket_name.to_string(),
        }
    }
}
