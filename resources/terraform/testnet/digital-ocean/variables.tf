variable "droplet_ssh_keys" {
  type = list(number)
  default = [
    44147181, # Ermine Jose
    37243057, # Benno Zeeman
    38313409, # Roland Sherwin
    36971688, # David Irvine
    19315097, # Stephen Coyle
    29201567, # Josh Wilson
    30643816, # Anselme Grumbach
    30113222, # Qi Ma
    42022675, # Shu
    42317962, # Mazzi
    30878672, # Chris O'Neil
    31216015, # QA
    34183228, # GH Actions Automation
    38596814, # sn-testnet-workflows automation
    29586082
  ]
}

variable "full_cone_droplet_size" {
  description = "The size of the droplet for Full Cone NAT gateway and private node VMs"
  default = "s-2vcpu-4gb"
}

variable "symmetric_nat_gateway_droplet_size" {
  description = "The size of the droplet for Symmetric NAT gateway VM"
  default = "s-1vcpu-2gb"
}

variable "node_droplet_size" {
  description = "The size of the droplet for generic nodes VMs"
  default = "s-2vcpu-4gb"
}

variable "peer_cache_droplet_size" {
  description = "The size of the droplet for Peer Cache nodes VMs"
  default = "s-2vcpu-4gb"
}

variable "ant_client_droplet_size" {
  description = "The size of the droplet for ANT Client VMs"
  default = "s-2vcpu-4gb"
}

variable "build_machine_size" {
  default = "s-8vcpu-16gb"
}

variable "build_droplet_image_id" {
  description = "The ID of the image for the build machine. Varies per region."
}

variable "peer_cache_droplet_image_id" {
  description = "The ID of the Peer Cache node droplet image. Varies per environment type."
}

variable "nat_gateway_droplet_image_id" {
  description = "The ID of the gateway droplet image. Varies per environment type."
}

variable "node_droplet_image_id" {
  description = "The ID of the node droplet image. Varies per environment type."
}

variable "ant_client_droplet_image_id" {
  description = "The ID of the ANT Client droplet image. Varies per environment type."
}

variable "region" {
  default = "lon1"
}

variable "genesis_vm_count" {
  default     = 1
  description = "Set to 1 or 0 to control whether there is a genesis node"
}

variable "peer_cache_node_vm_count" {
  default     = 2
  description = "The number of droplets to launch for Peer Cache nodes"
}

variable "node_vm_count" {
  default     = 10
  description = "The number of droplets to launch for nodes"
}

variable "full_cone_private_node_vm_count" {
  default     = 1
  description = "The number of droplets to launch for full cone private nodes"
}

variable "symmetric_private_node_vm_count" {
  default     = 1
  description = "The number of droplets to launch for symmetric private nodes"
}

variable "ant_client_vm_count" {
  default     = 2
  description = "The number of droplets to launch for the ANT Clients"
}

variable "use_custom_bin" {
  type        = bool
  default     = false
  description = "A boolean to enable use of a custom bin"
}

variable "evm_node_vm_count" {
  default     = 0
  description = "The number of droplets to launch for EVM nodes"
}

variable "evm_node_droplet_size" {
  description = "The size of the droplet for EVM node VMs"
  default = "s-4vcpu-8gb"
}

variable "evm_node_droplet_image_id" {
  description = "The ID of the EVM node droplet image. Varies per environment type."
}

variable "volumes_per_node" {
  description = "Number of volumes to attach to each node VM. This is set to the maximum number of volumes that can be attached to a droplet."
  type        = number
  default     = 7
}

variable "peer_cache_node_volume_size" {
  description = "Size of each volume in GB for peer cache nodes"
  type        = number
  default = 0
}

variable "genesis_node_volume_size" {
  description = "Size of each volume in GB for the genesis node"
  type        = number
  default = 0
}

variable "node_volume_size" {
  description = "Size of each volume in GB for generic nodes"
  type        = number
  default = 0
}

variable "full_cone_private_node_volume_size" {
  description = "Size of each volume in GB for full cone private nodes"
  type        = number
  default = 0
}

variable "symmetric_private_node_volume_size" {
  description = "Size of each volume in GB for symmetric private nodes"
  type        = number
  default = 0
}

variable "peer_cache_reserved_ips" {
  type = list(string)
  description = "List of reserved IPs for the peer nodes"
  default = []
}

variable "ant_client_volume_size" {
  description = "Size of each volume in GB for the ANT Client VMs"
  type        = number
  default     = 70
}
