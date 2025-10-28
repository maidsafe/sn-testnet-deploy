# Autonomi Testnet Deploy

This tool can be used to automate the deployment of testnets on AWS or Digital Ocean.

It uses Terraform to create the infrastructure and Ansible to provision it, and a Rust binary to coordinate the two.

If you are interested in deploying your own testnets, or using the tool to deploy your own nodes for participating in one of Maidsafe's testnets, you can fork the repository and change a few details.

Since we use Ansible, if you want to use the tool on Windows, you'll need to use WSL.

## Setup

The tool makes use of Terraform to create either droplets on Digital Ocean or EC2 instances on AWS, so you need an installation of that on your platform. It is very likely available in your platform's package manager.

We make use of Ansible for provisioning the VMs. Since Ansible is a Python application, it is advisable to install it in a virtualenv. If this sounds unfamiliar, I would recommend asking ChatGPT something along the lines of, "How can I install Ansible in a virtualenv created and managed by virtualenvwrapper?" The virtualenv must be activated any time you use the tool.

After you've installed these tools, run our `setup` command:
```
cargo run -- setup
```

This will gather a bunch of values that are written to a `.env` file, which is read when the tool starts.

You only need to run this command once.

## Deploying a Testnet

Use this command to deploy a testnet that uses Arbitrum Sepolia as the EVM:
```
cargo run -- deploy --name DEV-16 --network-id 50 --rewards-address 0x03B770D9cD32077cC0bF330c13C114a87643B124 --evm-network-type arbitrum-sepolia-test --funding-wallet-secret-key <value>
```

## Clean Up

To remove the testnet, use the following command:
```
cargo run -- clean --name DEV-16
```

## Building VM Images

This repository also contains [Packer](https://www.packer.io/) templates for building VM images. With the tools preinstalled, the time to deploy the testnet is significantly reduced.

Regenerating the image should be something that's infrequent, so as of yet there's no command in the deploy binary for generating it. However, it's a simple process. First, install Packer on your system; like Terraform, it's widely available in package managers. Then after that:

```
export DIGITALOCEAN_TOKEN=<your PAT>
cd resources/packer
packer init .
packer build build.pkr.hcl
```

This will produce a VM image that can now be launched as a droplet. There is also another template, `node.pkr.hcl`.

## License

This repository is licensed under the BSD-3-Clause license.

See the [LICENSE](LICENSE) file for more details.
