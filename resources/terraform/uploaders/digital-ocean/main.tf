terraform {
  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.48.2"
    }
  }
  backend "s3" {
    key = "sn-testnet-tool-uploaders-digital-ocean.tfstate"
  }
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

resource "digitalocean_droplet" "uploader" {
  count    = var.uploader_vm_count
  image    = var.uploader_droplet_image_id
  name     = "${terraform.workspace}-uploader-${count.index + 1}"
  region   = var.region
  size     = var.uploader_droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:uploader"]
}

locals {
  uploader_volume_keys = flatten([
    for node_index in range(var.uploader_vm_count) : [
      for volume_index in range(var.volumes_per_node) : "${node_index+1}-${volume_index+1}"
    ]
  ])
}

resource "digitalocean_volume" "uploader_attached_volume" {
  for_each = var.uploader_volume_size > 0 ? { for key in local.uploader_volume_keys : key => key } : {}
  name     = lower("${terraform.workspace}-uploader-${split("-", each.key)[0]}-volume-${split("-", each.key)[1]}")
  size     = var.uploader_volume_size
  region   = var.region
}

resource "digitalocean_volume_attachment" "uploader_volume_attachment" {
  for_each = var.uploader_volume_size > 0 ? { for key in local.uploader_volume_keys : key => key } : {}
  droplet_id = digitalocean_droplet.uploader[tonumber(split("-", each.key)[0]) - 1].id
  volume_id  = digitalocean_volume.uploader_attached_volume[each.key].id
}
