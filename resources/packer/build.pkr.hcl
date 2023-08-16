packer {
  required_plugins {
    digitalocean = {
      version = ">= 1.0.4"
      source  = "github.com/digitalocean/digitalocean"
    }
    ansible = {
      source  = "github.com/hashicorp/ansible"
      version = "~> 1"
    }
  }
}

variable "user_home" {
  default = env("HOME")
}

variable "droplet_image" {
  type = string
  default = "ubuntu-22-10-x64"
  description = ""
}

variable "region" {
  type = string
  default = "lon1"
  description = "This is quite a powerful image but it means the build time will be fast"
}

variable "size" {
  type = string
  default = "s-8vcpu-16gb"
  description = "This is quite a powerful image but it means the build time will be fast"
}

variable "ssh_username" {
  type = string
  default = "root"
  description = "On DO the root user is used"
}

source "digitalocean" "build" {
  image         = var.droplet_image
  region        = var.region
  size          = var.size
  snapshot_name = "safe_network-build-{{timestamp}}"
  ssh_username  = var.ssh_username
}

build {
  name    = "build-testnet-deploy"
  sources = [
    "source.digitalocean.build"
  ]
  provisioner "file" {
    source = "${var.user_home}/.ansible/vault-password"
    destination = "/tmp/ansible-vault-password"
  }
  provisioner "shell" {
    script = "../scripts/install_ansible.sh"
  }
  provisioner "ansible-local" {
    playbook_dir = "../ansible"
    playbook_file = "../ansible/create_build_image.yml"
    extra_arguments = [
      "--vault-password-file=/tmp/ansible-vault-password",
      "--extra-vars",
      "provider=digital-ocean",
    ]
  }
}
