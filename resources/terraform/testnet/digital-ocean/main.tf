terraform {
  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.0"
    }
  }
  backend "s3" {
    key    = "sn-testnet-tool-digital-ocean.tfstate"
  }
}

resource "digitalocean_droplet" "genesis" {
  image    = var.node_droplet_image_id
  name     = "${terraform.workspace}-genesis"
  region   = var.region
  size     = var.droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:genesis"]
}

resource "digitalocean_droplet" "node" {
  count    = var.node_count
  image    = var.node_droplet_image_id
  name     = "${terraform.workspace}-node-${count.index + 1}"
  region   = var.region
  size     = var.droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:node"]
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

# Todo: Have unique name for firewall? As we got this error
# Error creating firewall: POST https://api.digitalocean.com/v2/firewalls: 409 duplicate name
# resource "digitalocean_firewall" "auditor_fw" {
#   name = "auditor-firewall"

#   inbound_rule {
#     protocol         = "tcp"
#     port_range       = "80"
#     source_addresses = ["127.0.0.1"]
#   }
#   # Allow SSH connections
#   inbound_rule {
#     protocol         = "tcp"
#     port_range       = "22"
#     source_addresses = ["0.0.0.0/0"]
#   }
#   outbound_rule {
#     protocol               = "udp"
#     port_range             = "1-65535"
#     destination_addresses  = ["0.0.0.0/0"]
#   }
#     outbound_rule {
#     protocol               = "tcp"
#     port_range             = "1-65535"
#     destination_addresses  = ["0.0.0.0/0"]
#   }

#   droplet_ids = [digitalocean_droplet.auditor.id]
# }

resource "digitalocean_droplet" "auditor" {
  image    = var.auditor_droplet_image_id
  name     = "${terraform.workspace}-auditor"
  region   = var.region
  size     = var.droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:auditor"]
}