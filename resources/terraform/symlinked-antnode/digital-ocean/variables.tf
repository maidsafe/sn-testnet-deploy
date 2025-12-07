variable "droplet_ssh_keys" {
  type = list(number)
  default = [
    50457610, # Ermine
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

variable "droplet_size" {
  description = "The size of the droplet"
  default     = "s-2vcpu-4gb"
}

variable "droplet_image_id" {
  description = "The ID of the droplet image. Varies per region."
}

variable "region" {
  description = "Digital Ocean region"
  default     = "lon1"
}

variable "vm_count" {
  description = "Number of VMs to create (typically 1)"
  default     = 1
}

variable "volumes_per_node" {
  description = "Number of volumes to attach to each node VM (for striped storage)"
  type        = number
  default     = 7
}

variable "volume_size" {
  description = "Size in GB for each attached volume (set to 0 to disable volumes)"
  type        = number
  default     = 100
}

variable "use_custom_bin" {
  type        = bool
  default     = false
  description = "A boolean to enable use of a custom bin"
}

variable "build_machine_size" {
  default = "s-8vcpu-16gb"
}

variable "build_droplet_image_id" {
  description = "The ID of the image for the build machine. Varies per region."
}
