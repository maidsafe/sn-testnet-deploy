# Safe Network Testnet Deploy

This tool can be used to automate the deployment of testnets on AWS or Digital Ocean.

It uses Terraform to create the infrastructure and Ansible to provision it, and a Rust binary to coordinate the two.

If you are interested in deploying your own testnets, or using the tool to deploy your own nodes for participating in one of Maidsafe's testnets, you can fork the repository and change a few details.

Since we use Ansible, if you want to use the tool on Windows, you'll need to use WSL.

## License

This repository is licensed under the BSD-3-Clause license.

See the [LICENSE](LICENSE) file for more details.
