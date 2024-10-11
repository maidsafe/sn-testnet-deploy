terraform {
  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.0"
    }
  }
  backend "s3" {
    key = "sn-testnet-tool-digital-ocean.tfstate"
  }
}

resource "digitalocean_droplet" "auditor" {
  count    = var.auditor_vm_count
  image    = var.auditor_droplet_image_id
  name     = "${terraform.workspace}-auditor-${count.index + 1}"
  region   = var.region
  size     = var.node_droplet_size
  backups  = true
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:auditor"]
}

resource "digitalocean_droplet" "bootstrap_node" {
  count    = var.bootstrap_node_vm_count
  image    = var.bootstrap_droplet_image_id
  name     = "${terraform.workspace}-bootstrap-node-${count.index + 1}"
  region   = var.region
  size     = var.bootstrap_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:bootstrap_node"]
}

resource "digitalocean_droplet" "build" {
  count    = var.use_custom_bin ? 1 : 0
  image    = var.build_droplet_image_id
  name     = "${terraform.workspace}-build"
  region   = var.region
  size     = var.build_machine_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:build"]
}

resource "digitalocean_droplet" "genesis_bootstrap" {
  count    = var.genesis_vm_count
  image    = var.bootstrap_droplet_image_id
  name     = "${terraform.workspace}-genesis-bootstrap"
  region   = var.region
  size     = var.bootstrap_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:genesis"]
}

resource "digitalocean_droplet" "nat_gateway" {
  count    = var.setup_nat_gateway ? 1 : 0
  image    = var.nat_gateway_droplet_image_id
  name     = "${terraform.workspace}-nat-gateway"
  region   = var.region
  size     = var.nat_gateway_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:nat_gateway"]
}

resource "digitalocean_droplet" "node" {
  count    = var.node_vm_count 
  image    = var.node_droplet_image_id
  name     = "${terraform.workspace}-node-${count.index + 1}"
  region   = var.region
  size     = var.node_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:node"]
}

resource "digitalocean_droplet" "private_node" {
  count   = var.private_node_vm_count
  image    = var.node_droplet_image_id
  name     = "${terraform.workspace}-private-node-${count.index + 1}"
  region   = var.region
  size     = var.node_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:private_node"]
}

resource "digitalocean_droplet" "uploader" {
  count    = var.uploader_vm_count
  image    = var.uploader_droplet_image_id
  name     = "${terraform.workspace}-uploader-${count.index + 1}"
  region   = var.region
  size     = var.uploader_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:uploader"]
}

resource "digitalocean_droplet" "evm_node" {
  count    = var.evm_node_vm_count
  image    = var.evm_node_droplet_image_id
  name     = "${terraform.workspace}-evm-node-${count.index + 1}"
  region   = var.region
  size     = var.evm_node_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:evm_node"]
}
