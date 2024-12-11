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

resource "digitalocean_droplet" "peer_cache_node" {
  count    = var.peer_cache_node_vm_count
  image    = var.peer_cache_droplet_image_id
  name     = "${terraform.workspace}-peer-cache-node-${count.index + 1}"
  region   = var.region
  size     = var.peer_cache_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:peer_cache_node"]
}

resource "digitalocean_reserved_ip_assignment" "peer_cache_node_ip" {
  count       = length(var.peer_cache_reserved_ips) > 0 ? var.peer_cache_node_vm_count : 0
  ip_address  = var.peer_cache_reserved_ips[count.index]
  droplet_id  = digitalocean_droplet.peer_cache_node[count.index].id
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
  image    = var.peer_cache_droplet_image_id
  name     = "${terraform.workspace}-genesis-bootstrap"
  region   = var.region
  size     = var.peer_cache_droplet_size
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

locals {
  peer_cache_node_volume_keys = flatten([
    for node_index in range(var.peer_cache_node_vm_count) : [
      for volume_index in range(var.volumes_per_node) : "${node_index+1}-${volume_index+1}"
    ]
  ])

  genesis_node_volume_keys = flatten([
    for node_index in range(var.genesis_vm_count) : [
      for volume_index in range(var.volumes_per_node) : "${node_index+1}-${volume_index+1}"
    ]
  ])

  node_volume_keys = flatten([
    for node_index in range(var.node_vm_count) : [
      for volume_index in range(var.volumes_per_node) : "${node_index+1}-${volume_index+1}"
    ]
  ])

  private_node_volume_keys = flatten([
    for node_index in range(var.private_node_vm_count) : [
      for volume_index in range(var.volumes_per_node) : "${node_index+1}-${volume_index+1}"
    ]
  ])
}

resource "digitalocean_volume" "peer_cache_node_attached_volume" {
  for_each = { for key in local.peer_cache_node_volume_keys : key => key }
  name        = lower("${terraform.workspace}-peer-cache-node-${split("-", each.key)[0]}-volume-${split("-", each.key)[1]}")
  size        = var.peer_cache_node_volume_size
  region      = var.region
}

resource "digitalocean_volume_attachment" "peer_cache_node_volume_attachment" {
  for_each = { for key in local.peer_cache_node_volume_keys : key => key }
  droplet_id = digitalocean_droplet.peer_cache_node[tonumber(split("-", each.key)[0]) -1 ].id
  volume_id  = digitalocean_volume.peer_cache_node_attached_volume[each.key].id
}

resource "digitalocean_volume" "genesis_node_attached_volume" {
  for_each = { for key in local.genesis_node_volume_keys : key => key }
  name        = lower("${terraform.workspace}-genesis-bootstrap-${split("-", each.key)[0]}-volume-${split("-", each.key)[1]}")
  size        = var.genesis_node_volume_size
  region      = var.region
}

resource "digitalocean_volume_attachment" "genesis_node_volume_attachment" {
  for_each = { for key in local.genesis_node_volume_keys : key => key }
  droplet_id = digitalocean_droplet.genesis_bootstrap[tonumber(split("-", each.key)[0]) -1 ].id
  volume_id  = digitalocean_volume.genesis_node_attached_volume[each.key].id
}

resource "digitalocean_volume" "node_attached_volume" {
  for_each = { for key in local.node_volume_keys : key => key }
  name        = lower("${terraform.workspace}-node-${split("-", each.key)[0]}-volume-${split("-", each.key)[1]}")
  size        = var.node_volume_size
  region      = var.region
}

resource "digitalocean_volume_attachment" "node_volume_attachment" {
  for_each = { for key in local.node_volume_keys : key => key }
  droplet_id = digitalocean_droplet.node[tonumber(split("-", each.key)[0]) -1 ].id
  volume_id  = digitalocean_volume.node_attached_volume[each.key].id
}

resource "digitalocean_volume" "private_node_attached_volume" {
  for_each = { for key in local.private_node_volume_keys : key => key }
  name        = lower("${terraform.workspace}-private-node-${split("-", each.key)[0]}-volume-${split("-", each.key)[1]}")
  size        = var.private_node_volume_size
  region      = var.region
}

resource "digitalocean_volume_attachment" "private_node_volume_attachment" {
  for_each = { for key in local.private_node_volume_keys : key => key }
  droplet_id = digitalocean_droplet.private_node[tonumber(split("-", each.key)[0]) -1 ].id
  volume_id  = digitalocean_volume.private_node_attached_volume[each.key].id
}