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

variable "ant_client_droplet_size" {
  description = "The size of the droplet for ANT Client VMs"
}

variable "build_machine_size" {
  default = "s-8vcpu-16gb"
}

variable "build_droplet_image_id" {
  description = "The ID of the image for the build machine. Varies per region."
}

variable "ant_client_droplet_image_id" {
  description = "The ID of the ANT Client droplet image. Varies per environment type."
}

variable "region" {
  default = "lon1"
}

variable "ant_client_vm_count" {
  default     = 2
  description = "The number of droplets to launch for ANT Clients"
}

variable "use_custom_bin" {
  type        = bool
  default     = false
  description = "A boolean to enable use of a custom bin"
}

variable "volumes_per_node" {
  description = "Number of volumes to attach to each node VM. This is set to the maximum number of volumes that can be attached to a droplet."
  type        = number
  default     = 7
}

variable "ant_client_volume_size" {
  description = "Size of each volume in GB for ANT Client VMs"
  type        = number
  default     = 70
}
