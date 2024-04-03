variable "droplet_ssh_keys" {
  type    = list(number)
  default = [
    37243057, # Benno Zeeman
    38313409, # Roland Sherwin
    36971688, # David Irvine
    19315097, # Stephen Coyle
    29201567, # Josh Wilson
    29586082, # Gabriel Viganotti
    30643816, # Anselme Grumbach
    30113222, # Qi Ma
    30878672, # Chris O'Neil
    41483452, # Jason Paul
    31216015, # QA
    34183228, # GH Actions Automation
    38596814  # sn-testnet-workflows automation
  ]
}

variable "droplet_size" {
  default = "s-2vcpu-4gb"
}

variable "build_machine_size" {
  default = "s-8vcpu-16gb"
}

# This corresponds to the 'safe_network-build-1708589497' image/snapshot.
variable "build_droplet_image_id" {
  default = "150625831"
}

# This corresponds to the 'safe_network-node-1708591050' image/snapshot.2
variable "node_droplet_image_id" {
  default = "150627826"
}

variable "region" {
  default = "lon1"
}

variable "node_count" {
  default = 30
  description = "The number of droplets to launch for the nodes"
}

variable "use_custom_bin" {
  type = bool
  default = false
  description = "A boolean to enable use of a custom bin"
}
