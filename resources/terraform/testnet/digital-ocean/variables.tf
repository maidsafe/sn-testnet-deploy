variable "droplet_ssh_keys" {
  type = list(number)
  default = [
    37243057, # Benno Zeeman
    38313409, # Roland Sherwin
    36971688, # David Irvine
    19315097, # Stephen Coyle
    29201567, # Josh Wilson
    29586082, # Gabriel Viganotti
    30643816, # Anselme Grumbach
    30113222, # Qi Ma
    42022675, # Shu
    42317962, # Mazzi
    30878672, # Chris O'Neil
    31216015, # QA
    34183228, # GH Actions Automation
    38596814  # sn-testnet-workflows automation
  ]
}

variable "droplet_size" {
  default = "s-2vcpu-4gb"
}
variable "boostrap_droplet_size" {
  default = "s-8vcpu-16gb-480gb-intel"
}

variable "build_machine_size" {
  default = "s-8vcpu-16gb"
}

variable "fresh_testnet" {
  default = "false"
}

# This corresponds to the 'safe_network-auditor-1715864456' image/snapshot.
variable "auditor_droplet_image_id" {
  default = "156295663"
}

# This corresponds to the 'safe_network-build-1715854128' image/snapshot.
variable "build_droplet_image_id" {
  default = "156286538"
}

# This corresponds to the 'safe_network-node-1717198184' image/snapshot.
variable "node_droplet_image_id" {
  default = "157362431"
}

variable "region" {
  default = "lon1"
}

# 25*80 = 2k. A good node sample size. Should provide enough bandwidth per droplet to survive on defualt limits.
variable "node_count" {
  default     = 25
  description = "The number of droplets to launch for the nodes"
}

variable "use_custom_bin" {
  type        = bool
  default     = false
  description = "A boolean to enable use of a custom bin"
}
