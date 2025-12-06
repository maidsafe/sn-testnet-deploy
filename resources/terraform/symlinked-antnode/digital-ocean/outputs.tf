output "droplet_ips" {
  value       = digitalocean_droplet.symlinked_antnode[*].ipv4_address
  description = "IP addresses of symlinked antnode droplets"
}

output "droplet_ids" {
  value       = digitalocean_droplet.symlinked_antnode[*].id
  description = "IDs of symlinked antnode droplets"
}

output "volume_count" {
  value       = length(digitalocean_volume.attached_volume)
  description = "Total number of volumes attached"
}
