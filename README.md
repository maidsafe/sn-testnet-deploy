# Safe Network Testnet Deploy

This tool can be used to automate the deployment of testnets on AWS or Digital Ocean.

It uses Terraform to create the infrastructure and Ansible to provision it, and a Rust binary to coordinate the two.

If you are interested in deploying your own testnets, or using the tool to deploy your own nodes for participating in one of Maidsafe's testnets, you can fork the repository and change a few details.

Since we use Ansible, if you want to use the tool on Windows, you'll need to use WSL.

## Setup

The tool makes use of Terraform to create either droplets on Digital Ocean or EC2 instances on AWS, so you need an installation of that on your platform. It is very likely available in your platform's package manager.

We make use of Ansible for provisioning the VMs. Since Ansible is a Python application, it is advisable to install it in a virtualenv. If this sounds unfamiliar, I would recommend asking ChatGPT something along the lines of, "How can I install Ansible in a virtualenv created and managed by virtualenvwrapper?" The virtualenv must be activated any time you use the tool.

August 2023 update: please pin to Ansible version 8.2.0, using `pip install ansible==8.2.0`. There is an issue with the newer version not correctly reading the Digital Ocean environment variables for the dynamic inventory.

After you've installed these tools, run our `setup` command:
```
cargo run -- setup
```

This will gather a bunch of values that are written to a `.env` file, which is read when the tool starts.

You only need to run this command once.

## Deploying a Testnet

After completing the setup, you can deploy a testnet like so:
```
cargo run -- deploy --name beta
```

This will deploy a testnet named 'beta' to Digital Ocean, with 10 VMs and 20 node processes on each VM, and the latest version of the `safenode` binary.

If you want more VMs or nodes, use the `--vm-count` and `--node-count` arguments to vary those values to suit your needs:
```
cargo run -- deploy --name beta --vm-count 100 --node-count 30
```

You may want to deploy a custom version of the `safenode` binary. To do so, use the `--branch` and `--repo-owner` arguments:
```
cargo run -- deploy --name beta --vm-count 100 --node-count 30 --repo-owner jacderida --branch custom_branch 
```

To get a list of the machines and other information about the testnet:
```
cargo run -- inventory --name beta --provider digital-ocean
```

### Test Data

There is a set of standard test data, which is about 100MB of image files. It can be uploaded to a testnet using the following command:
```
cargo run -- upload-test-data --name beta
```

The archive will be downloaded from S3, then each file will be uploaded using the `files upload` command of the `safe` client. The client itself is also fetched from S3. It was built using the same source as the branch used for the node binary.

## Clean Up

To remove the testnet, use the following command:
```
cargo run -- clean --name beta --provider digital-ocean
```

This will use Terraform to tear down all the droplets it created.

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

## Logging Infrastructure

We forward the logs for our testnets to an S3 bucket, which requires some infrastructure.

The VMs hosting our nodes are small machines with about 2GB of RAM. They use Filebeat to forward the logs to a dedicated Logstash stack, which then forwards them on to S3. For Logstash, right now, we only have one stack and a single machine in the stack, but we've designed to accommodate having many.

This Logstash stack will serve all testnets. To bring the stack up, use this command:
```
cargo run -- logstash deploy --name main --provider digital-ocean
```

To tear it down:
```
cargo run -- logstash clean --name main --provider digital-ocean
```

By default, all deployed testnets will point to this `main` stack. However, it's possible to create another one with a different name, and supply the name of that stack on the main `deploy` command, using the `--logstash-stack-name` argument.

## License

This repository is licensed under the BSD-3-Clause license.

See the [LICENSE](LICENSE) file for more details.
