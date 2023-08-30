terraform {
  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.0"
    }
  }
  backend "s3" {
    key    = "sn-testnet-deploy-logstash-digital-ocean.tfstate"
  }
}

resource "digitalocean_droplet" "node" {
  count    = var.node_count
  image    = var.droplet_image
  name     = "logstash-${terraform.workspace}-${count.index + 1}"
  region   = var.region
  size     = var.droplet_size
  ssh_keys = var.droplet_ssh_keys
  tags     = ["environment:${terraform.workspace}", "type:logstash"]
}
