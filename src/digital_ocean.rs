// Copyright (c) 2023, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the BSD-3-Clause license.
// Please see the LICENSE file for more details.

use crate::error::{Error, Result};
use log::debug;
use reqwest::Client;
use std::{net::Ipv4Addr, str::FromStr};

pub const DIGITAL_OCEAN_API_BASE_URL: &str = "https://api.digitalocean.com";
pub const DIGITAL_OCEAN_API_PAGE_SIZE: usize = 200;

pub struct Droplet {
    pub id: usize,
    pub name: String,
    pub ip_address: Ipv4Addr,
}

pub struct DigitalOceanClient {
    pub base_url: String,
    pub access_token: String,
    pub page_size: usize,
}

impl DigitalOceanClient {
    pub async fn list_droplets(&self) -> Result<Vec<Droplet>> {
        let client = Client::new();
        let mut has_next_page = true;
        let mut page = 1;
        let mut droplets = Vec::new();
        while has_next_page {
            let url = format!(
                "{}/v2/droplets?page={}&per_page={}",
                self.base_url, page, self.page_size
            );
            debug!("Executing droplet list request with {url}");
            let response = client
                .get(url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .send()
                .await?;
            if response.status().as_u16() == 401 {
                debug!("Error response body: {}", response.text().await?);
                return Err(Error::DigitalOceanUnauthorized);
            } else if !response.status().is_success() {
                let status_code = response.status().as_u16();
                let response_body = response.text().await?;
                debug!("Response status code: {}", status_code);
                debug!("Error response body: {}", response_body);
                return Err(Error::DigitalOceanUnexpectedResponse(
                    status_code,
                    response_body,
                ));
            }

            let json: serde_json::Value = serde_json::from_str(&response.text().await?)?;
            let droplet_array =
                json["droplets"]
                    .as_array()
                    .ok_or(Error::MalformedDigitalOceanApiRespose(
                        "droplets".to_string(),
                    ))?;

            for droplet_json in droplet_array {
                debug!("Droplet json {droplet_json:?}");
                let id = droplet_json["id"]
                    .as_u64()
                    .ok_or(Error::MalformedDigitalOceanApiRespose("id".to_string()))?;
                let name = droplet_json["name"]
                    .as_str()
                    .ok_or(Error::MalformedDigitalOceanApiRespose("name".to_string()))?
                    .to_string();
                let ip_address_array = droplet_json["networks"]["v4"].as_array().ok_or(
                    Error::MalformedDigitalOceanApiRespose("droplets".to_string()),
                )?;
                let public_ip = ip_address_array
                    .iter()
                    .find(|x| x["type"].as_str().unwrap() == "public")
                    .ok_or(Error::DigitalOceanPublicIpAddressNotFound)?;
                debug!("Got public ip {public_ip:?}");
                let ip_address = Ipv4Addr::from_str(
                    public_ip["ip_address"]
                        .as_str()
                        .ok_or(Error::DigitalOceanPublicIpAddressNotFound)?,
                )?;
                debug!("got ip address {ip_address:?}");

                droplets.push(Droplet {
                    id: id as usize,
                    name,
                    ip_address,
                });
            }

            let links_object = json["links"]
                .as_object()
                .ok_or(Error::MalformedDigitalOceanApiRespose("links".to_string()))?;
            if links_object.is_empty() {
                // All the data was returned on a single page.
                has_next_page = false;
            } else {
                let pages_object = links_object["pages"]
                    .as_object()
                    .ok_or(Error::MalformedDigitalOceanApiRespose("pages".to_string()))?;
                if pages_object.contains_key("next") {
                    page += 1;
                } else {
                    has_next_page = false;
                }
            }
        }

        Ok(droplets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::{eyre::eyre, Result};
    use httpmock::prelude::*;

    #[tokio::test]
    async fn test_list_droplets_with_single_page() -> Result<()> {
        // This respresents a real response from the Digital Ocean API (with names and IP addresses
        // changed), as of September 2023.
        const MOCK_API_RESPONSE: &str = r#"
        {
          "droplets": [
            {
              "id": 118019015,
              "name": "testnet-node-01",
              "memory": 2048,
              "vcpus": 1,
              "disk": 50,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2018-11-05T13:57:25Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060266
              ],
              "image": {
                "id": 33346667,
                "name": "Droplet Deployer OLD",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [
                  "lon1",
                  "nyc1",
                  "sfo1",
                  "ams2",
                  "sgp1",
                  "fra1",
                  "tor1",
                  "blr1"
                ],
                "created_at": "2018-04-10T10:40:49Z",
                "min_disk_size": 50,
                "type": "snapshot",
                "size_gigabytes": 7.11,
                "tags": [],
                "status": "available"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-1vcpu-2gb",
                "memory": 2048,
                "vcpus": 1,
                "disk": 50,
                "transfer": 2,
                "price_monthly": 12,
                "price_hourly": 0.01786,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-1vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.2",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "private"
                  },
                  {
                    "ip_address": "104.248.0.110",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "London 1",
                "slug": "lon1",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "m3-4vcpu-32gb",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "m3-8vcpu-64gb",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            },
            {
              "id": 177884621,
              "name": "testnet-node-02",
              "memory": 2048,
              "vcpus": 2,
              "disk": 60,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2020-01-30T03:39:42Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060570
              ],
              "image": {
                "id": 53893572,
                "name": "18.04.3 (LTS) x64",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [],
                "created_at": "2019-10-22T01:38:19Z",
                "min_disk_size": 20,
                "type": "base",
                "size_gigabytes": 2.36,
                "description": "Ubuntu 18.04 x64 20191022",
                "tags": [],
                "status": "deleted"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-2vcpu-2gb",
                "memory": 2048,
                "vcpus": 2,
                "disk": 60,
                "transfer": 3,
                "price_monthly": 18,
                "price_hourly": 0.02679,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-2vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.3",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "private"
                  },
                  {
                    "ip_address": "104.248.0.111",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "New York 3",
                "slug": "nyc3",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "s-2vcpu-8gb-amd",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-2vcpu-8gb-160gb-intel",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "s-4vcpu-16gb-amd",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-4vcpu-16gb-320gb-intel",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "c-4-intel",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "c2-4vcpu-8gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "s-8vcpu-32gb-amd",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "s-8vcpu-32gb-640gb-intel",
                  "m3-4vcpu-32gb",
                  "c-8-intel",
                  "c2-8vcpu-16gb-intel",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "s-16vcpu-64gb-amd",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "s-16vcpu-64gb-intel",
                  "m3-8vcpu-64gb",
                  "c-16-intel",
                  "c2-16vcpu-32gb-intel",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-32-intel",
                  "c2-32vcpu-64gb-intel",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            }
          ],
          "links": {},
          "meta": {
            "total": 104
          }
        }
        "#;

        let server = MockServer::start();
        let list_droplets_mock = server.mock(|when, then| {
            when.method(GET).path("/v2/droplets");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(MOCK_API_RESPONSE);
        });

        let client = DigitalOceanClient {
            base_url: server.base_url(),
            access_token: String::from("fake_token"),
            page_size: DIGITAL_OCEAN_API_PAGE_SIZE,
        };

        let droplets = client.list_droplets().await?;

        assert_eq!(2, droplets.len());
        assert_eq!(118019015, droplets[0].id);
        assert_eq!("testnet-node-01", droplets[0].name);
        assert_eq!(
            Ipv4Addr::from_str("104.248.0.110").unwrap(),
            droplets[0].ip_address
        );
        assert_eq!(177884621, droplets[1].id);
        assert_eq!("testnet-node-02", droplets[1].name);
        assert_eq!(
            Ipv4Addr::from_str("104.248.0.111").unwrap(),
            droplets[1].ip_address
        );

        list_droplets_mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn test_list_droplets_with_paged_response() -> Result<()> {
        const MOCK_API_PAGE_1_RESPONSE: &str = r#"
        {
          "droplets": [
            {
              "id": 118019015,
              "name": "testnet-node-01",
              "memory": 2048,
              "vcpus": 1,
              "disk": 50,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2018-11-05T13:57:25Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060266
              ],
              "image": {
                "id": 33346667,
                "name": "Droplet Deployer OLD",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [
                  "lon1",
                  "nyc1",
                  "sfo1",
                  "ams2",
                  "sgp1",
                  "fra1",
                  "tor1",
                  "blr1"
                ],
                "created_at": "2018-04-10T10:40:49Z",
                "min_disk_size": 50,
                "type": "snapshot",
                "size_gigabytes": 7.11,
                "tags": [],
                "status": "available"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-1vcpu-2gb",
                "memory": 2048,
                "vcpus": 1,
                "disk": 50,
                "transfer": 2,
                "price_monthly": 12,
                "price_hourly": 0.01786,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-1vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.2",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "private"
                  },
                  {
                    "ip_address": "104.248.0.110",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "London 1",
                "slug": "lon1",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "m3-4vcpu-32gb",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "m3-8vcpu-64gb",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            }
          ],
          "links": {
            "pages": {
              "next": "https://api.digitalocean.com/v2/droplets?page=1&per_page=1",
              "last": "https://api.digitalocean.com/v2/droplets?page=2&per_page=1"
            }
          },
          "meta": {
            "total": 104
          }
        }
        "#;
        const MOCK_API_PAGE_2_RESPONSE: &str = r#"
        {
          "droplets": [
            {
              "id": 177884621,
              "name": "testnet-node-02",
              "memory": 2048,
              "vcpus": 2,
              "disk": 60,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2020-01-30T03:39:42Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060570
              ],
              "image": {
                "id": 53893572,
                "name": "18.04.3 (LTS) x64",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [],
                "created_at": "2019-10-22T01:38:19Z",
                "min_disk_size": 20,
                "type": "base",
                "size_gigabytes": 2.36,
                "description": "Ubuntu 18.04 x64 20191022",
                "tags": [],
                "status": "deleted"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-2vcpu-2gb",
                "memory": 2048,
                "vcpus": 2,
                "disk": 60,
                "transfer": 3,
                "price_monthly": 18,
                "price_hourly": 0.02679,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-2vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.3",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "private"
                  },
                  {
                    "ip_address": "104.248.0.111",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "New York 3",
                "slug": "nyc3",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "s-2vcpu-8gb-amd",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-2vcpu-8gb-160gb-intel",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "s-4vcpu-16gb-amd",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-4vcpu-16gb-320gb-intel",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "c-4-intel",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "c2-4vcpu-8gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "s-8vcpu-32gb-amd",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "s-8vcpu-32gb-640gb-intel",
                  "m3-4vcpu-32gb",
                  "c-8-intel",
                  "c2-8vcpu-16gb-intel",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "s-16vcpu-64gb-amd",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "s-16vcpu-64gb-intel",
                  "m3-8vcpu-64gb",
                  "c-16-intel",
                  "c2-16vcpu-32gb-intel",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-32-intel",
                  "c2-32vcpu-64gb-intel",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            }
          ],
          "links": {
            "pages": {
              "first": "https://api.digitalocean.com/v2/droplets?page=1&per_page=1",
              "prev": "https://api.digitalocean.com/v2/droplets?page=1&per_page=1"
            }
          },
          "meta": {
            "total": 104
          }
        }
        "#;

        let server = MockServer::start();
        let list_droplets_page_one_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/droplets")
                .query_param("page", "1")
                .query_param("per_page", "1");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(MOCK_API_PAGE_1_RESPONSE);
        });
        let list_droplets_page_two_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/droplets")
                .query_param("page", "2")
                .query_param("per_page", "1");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(MOCK_API_PAGE_2_RESPONSE);
        });

        let client = DigitalOceanClient {
            base_url: server.base_url(),
            access_token: String::from("fake_token"),
            page_size: 1,
        };

        let droplets = client.list_droplets().await?;
        assert_eq!(2, droplets.len());
        assert_eq!(118019015, droplets[0].id);
        assert_eq!("testnet-node-01", droplets[0].name);
        assert_eq!(
            Ipv4Addr::from_str("104.248.0.110").unwrap(),
            droplets[0].ip_address
        );
        assert_eq!(177884621, droplets[1].id);
        assert_eq!("testnet-node-02", droplets[1].name);
        assert_eq!(
            Ipv4Addr::from_str("104.248.0.111").unwrap(),
            droplets[1].ip_address
        );

        list_droplets_page_one_mock.assert();
        list_droplets_page_two_mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn test_list_droplets_with_unauthorized_response() -> Result<()> {
        const MOCK_API_RESPONSE: &str =
            r#"{ "id": "Unauthorized", "message": "Unable to authenticate you" }"#;
        let server = MockServer::start();
        let list_droplets_page_one_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/droplets")
                .query_param("page", "1")
                .query_param("per_page", "1");
            then.status(401)
                .header("Content-Type", "application/json")
                .body(MOCK_API_RESPONSE);
        });

        let client = DigitalOceanClient {
            base_url: server.base_url(),
            access_token: String::from("fake_token"),
            page_size: 1,
        };

        let result = client.list_droplets().await;
        match result {
            Ok(_) => return Err(eyre!("This test should return an error")),
            Err(e) => {
                assert_eq!(
                    e.to_string(),
                    "Authorization failed for the Digital Ocean API"
                );
            }
        }

        list_droplets_page_one_mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn test_list_droplets_with_unexpected_response() -> Result<()> {
        const MOCK_API_RESPONSE: &str =
            r#"{ "id": "unexpected", "message": "Something unexpected happened" }"#;
        let server = MockServer::start();
        let list_droplets_page_one_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/droplets")
                .query_param("page", "1")
                .query_param("per_page", "1");
            then.status(500)
                .header("Content-Type", "application/json")
                .body(MOCK_API_RESPONSE);
        });

        let client = DigitalOceanClient {
            base_url: server.base_url(),
            access_token: String::from("fake_token"),
            page_size: 1,
        };

        let result = client.list_droplets().await;
        match result {
            Ok(_) => return Err(eyre!("This test should return an error")),
            Err(e) => {
                assert_eq!(
                    e.to_string(),
                    "Unexpected response: 500 -- { \"id\": \"unexpected\", \"message\": \"Something unexpected happened\" }"
                );
            }
        }

        list_droplets_page_one_mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn test_list_droplets_when_response_has_varying_ip_addresses() -> Result<()> {
        // This respresents a real response from the Digital Ocean API (with names and IP addresses
        // changed), as of September 2023.
        const MOCK_API_RESPONSE: &str = r#"
        {
          "droplets": [
            {
              "id": 118019015,
              "name": "testnet-node-01",
              "memory": 2048,
              "vcpus": 1,
              "disk": 50,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2018-11-05T13:57:25Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060266
              ],
              "image": {
                "id": 33346667,
                "name": "Droplet Deployer OLD",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [
                  "lon1",
                  "nyc1",
                  "sfo1",
                  "ams2",
                  "sgp1",
                  "fra1",
                  "tor1",
                  "blr1"
                ],
                "created_at": "2018-04-10T10:40:49Z",
                "min_disk_size": 50,
                "type": "snapshot",
                "size_gigabytes": 7.11,
                "tags": [],
                "status": "available"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-1vcpu-2gb",
                "memory": 2048,
                "vcpus": 1,
                "disk": 50,
                "transfer": 2,
                "price_monthly": 12,
                "price_hourly": 0.01786,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-1vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.2",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "London 1",
                "slug": "lon1",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "m3-4vcpu-32gb",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "m3-8vcpu-64gb",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            },
            {
              "id": 177884621,
              "name": "testnet-node-02",
              "memory": 2048,
              "vcpus": 2,
              "disk": 60,
              "locked": false,
              "status": "active",
              "kernel": null,
              "created_at": "2020-01-30T03:39:42Z",
              "features": [],
              "backup_ids": [],
              "next_backup_window": null,
              "snapshot_ids": [
                136060570
              ],
              "image": {
                "id": 53893572,
                "name": "18.04.3 (LTS) x64",
                "distribution": "Ubuntu",
                "slug": null,
                "public": false,
                "regions": [],
                "created_at": "2019-10-22T01:38:19Z",
                "min_disk_size": 20,
                "type": "base",
                "size_gigabytes": 2.36,
                "description": "Ubuntu 18.04 x64 20191022",
                "tags": [],
                "status": "deleted"
              },
              "volume_ids": [],
              "size": {
                "slug": "s-2vcpu-2gb",
                "memory": 2048,
                "vcpus": 2,
                "disk": 60,
                "transfer": 3,
                "price_monthly": 18,
                "price_hourly": 0.02679,
                "regions": [
                  "ams3",
                  "blr1",
                  "fra1",
                  "lon1",
                  "nyc1",
                  "nyc3",
                  "sfo2",
                  "sfo3",
                  "sgp1",
                  "syd1",
                  "tor1"
                ],
                "available": true,
                "description": "Basic"
              },
              "size_slug": "s-2vcpu-2gb",
              "networks": {
                "v4": [
                  {
                    "ip_address": "192.168.0.3",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "private"
                  },
                  {
                    "ip_address": "104.248.0.111",
                    "netmask": "255.255.240.0",
                    "gateway": "192.168.0.1",
                    "type": "public"
                  }
                ],
                "v6": []
              },
              "region": {
                "name": "New York 3",
                "slug": "nyc3",
                "features": [
                  "backups",
                  "ipv6",
                  "metadata",
                  "install_agent",
                  "storage",
                  "image_transfer"
                ],
                "available": true,
                "sizes": [
                  "s-1vcpu-1gb",
                  "s-1vcpu-1gb-amd",
                  "s-1vcpu-1gb-intel",
                  "s-1vcpu-1gb-35gb-intel",
                  "s-1vcpu-2gb",
                  "s-1vcpu-2gb-amd",
                  "s-1vcpu-2gb-intel",
                  "s-1vcpu-2gb-70gb-intel",
                  "s-2vcpu-2gb",
                  "s-2vcpu-2gb-amd",
                  "s-2vcpu-2gb-intel",
                  "s-2vcpu-2gb-90gb-intel",
                  "s-2vcpu-4gb",
                  "s-2vcpu-4gb-amd",
                  "s-2vcpu-4gb-intel",
                  "s-2vcpu-4gb-120gb-intel",
                  "s-2vcpu-8gb-amd",
                  "c-2",
                  "c2-2vcpu-4gb",
                  "s-2vcpu-8gb-160gb-intel",
                  "s-4vcpu-8gb",
                  "s-4vcpu-8gb-amd",
                  "s-4vcpu-8gb-intel",
                  "g-2vcpu-8gb",
                  "s-4vcpu-8gb-240gb-intel",
                  "gd-2vcpu-8gb",
                  "s-4vcpu-16gb-amd",
                  "m-2vcpu-16gb",
                  "c-4",
                  "c2-4vcpu-8gb",
                  "s-4vcpu-16gb-320gb-intel",
                  "s-8vcpu-16gb",
                  "m3-2vcpu-16gb",
                  "c-4-intel",
                  "s-8vcpu-16gb-amd",
                  "s-8vcpu-16gb-intel",
                  "c2-4vcpu-8gb-intel",
                  "g-4vcpu-16gb",
                  "s-8vcpu-16gb-480gb-intel",
                  "so-2vcpu-16gb",
                  "m6-2vcpu-16gb",
                  "gd-4vcpu-16gb",
                  "so1_5-2vcpu-16gb",
                  "s-8vcpu-32gb-amd",
                  "m-4vcpu-32gb",
                  "c-8",
                  "c2-8vcpu-16gb",
                  "s-8vcpu-32gb-640gb-intel",
                  "m3-4vcpu-32gb",
                  "c-8-intel",
                  "c2-8vcpu-16gb-intel",
                  "g-8vcpu-32gb",
                  "so-4vcpu-32gb",
                  "m6-4vcpu-32gb",
                  "gd-8vcpu-32gb",
                  "so1_5-4vcpu-32gb",
                  "s-16vcpu-64gb-amd",
                  "m-8vcpu-64gb",
                  "c-16",
                  "c2-16vcpu-32gb",
                  "s-16vcpu-64gb-intel",
                  "m3-8vcpu-64gb",
                  "c-16-intel",
                  "c2-16vcpu-32gb-intel",
                  "g-16vcpu-64gb",
                  "so-8vcpu-64gb",
                  "m6-8vcpu-64gb",
                  "gd-16vcpu-64gb",
                  "so1_5-8vcpu-64gb",
                  "m-16vcpu-128gb",
                  "c-32",
                  "c2-32vcpu-64gb",
                  "m3-16vcpu-128gb",
                  "c-32-intel",
                  "c2-32vcpu-64gb-intel",
                  "c-48",
                  "m-24vcpu-192gb",
                  "g-32vcpu-128gb",
                  "so-16vcpu-128gb",
                  "m6-16vcpu-128gb",
                  "gd-32vcpu-128gb",
                  "c2-48vcpu-96gb",
                  "m3-24vcpu-192gb",
                  "g-40vcpu-160gb",
                  "so1_5-16vcpu-128gb",
                  "m-32vcpu-256gb",
                  "gd-40vcpu-160gb",
                  "so-24vcpu-192gb",
                  "m6-24vcpu-192gb",
                  "m3-32vcpu-256gb",
                  "so1_5-24vcpu-192gb",
                  "so-32vcpu-256gb",
                  "m6-32vcpu-256gb",
                  "so1_5-32vcpu-256gb"
                ]
              },
              "tags": []
            }
          ],
          "links": {},
          "meta": {
            "total": 104
          }
        }
        "#;

        let server = MockServer::start();
        let list_droplets_mock = server.mock(|when, then| {
            when.method(GET).path("/v2/droplets");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(MOCK_API_RESPONSE);
        });

        let client = DigitalOceanClient {
            base_url: server.base_url(),
            access_token: String::from("fake_token"),
            page_size: DIGITAL_OCEAN_API_PAGE_SIZE,
        };

        let droplets = client.list_droplets().await?;

        assert_eq!(2, droplets.len());
        assert_eq!(118019015, droplets[0].id);
        assert_eq!("testnet-node-01", droplets[0].name);
        assert_eq!(
            Ipv4Addr::from_str("192.168.0.2").unwrap(),
            droplets[0].ip_address
        );
        assert_eq!(177884621, droplets[1].id);
        assert_eq!("testnet-node-02", droplets[1].name);
        assert_eq!(
            Ipv4Addr::from_str("104.248.0.111").unwrap(),
            droplets[1].ip_address
        );

        list_droplets_mock.assert();

        Ok(())
    }
}
