use super::super::setup::*;
use crate::ansible::MockAnsibleRunnerInterface;
use crate::digital_ocean::{Droplet, MockDigitalOceanClientInterface};
use crate::logstash::LogstashDeploy;
use crate::ssh::MockSshClientInterface;
use crate::terraform::MockTerraformRunnerInterface;
use crate::CloudProvider;
use color_eyre::Result;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

#[tokio::test]
async fn should_return_the_correct_hosts_for_the_stack() -> Result<()> {
    let (tmp_dir, working_dir) = setup_working_directory()?;
    let mut digital_ocean_client_mock = MockDigitalOceanClientInterface::new();
    digital_ocean_client_mock
        .expect_list_droplets()
        .times(1)
        .returning(|| {
            Ok(vec![
                Droplet {
                    id: 2000,
                    name: "logstash-main-1".to_string(),
                    ip_address: Ipv4Addr::from_str("10.0.0.1")?,
                },
                Droplet {
                    id: 2001,
                    name: "logstash-main-2".to_string(),
                    ip_address: Ipv4Addr::from_str("10.0.0.2")?,
                },
                Droplet {
                    id: 2002,
                    name: "logstash-test-1".to_string(),
                    ip_address: Ipv4Addr::from_str("10.0.0.3")?,
                },
                Droplet {
                    id: 2003,
                    name: "logstash-test-2".to_string(),
                    ip_address: Ipv4Addr::from_str("10.0.0.4")?,
                },
            ])
        });

    let logstash = LogstashDeploy::new(
        Box::new(MockTerraformRunnerInterface::new()),
        Box::new(MockAnsibleRunnerInterface::new()),
        Box::new(MockSshClientInterface::new()),
        Box::new(digital_ocean_client_mock),
        working_dir.to_path_buf(),
        CloudProvider::DigitalOcean,
    );

    let stack_hosts = logstash.get_stack_hosts("main").await?;

    assert_eq!(2, stack_hosts.len());
    assert_eq!(stack_hosts[0].ip(), IpAddr::from_str("10.0.0.1")?);
    assert_eq!(stack_hosts[0].port(), 5044);
    assert_eq!(stack_hosts[1].ip(), IpAddr::from_str("10.0.0.2")?);
    assert_eq!(stack_hosts[1].port(), 5044);

    drop(tmp_dir);
    Ok(())
}
