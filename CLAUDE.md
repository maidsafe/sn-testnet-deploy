# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is `sn-testnet-deploy` - a tool for automating deployment of Safe Network testnets on AWS or Digital Ocean. It orchestrates Terraform for infrastructure creation and Ansible for provisioning, coordinated by a Rust binary.

## Key Commands

### Build and Release
- `cargo build` - Build the testnet-deploy binary
- `cargo run -- <command>` - Run commands via the binary
- `just build-release-artifacts x86_64-unknown-linux-musl` - Build release artifacts for Linux
- `just package-release-assets` - Package binaries for distribution

### Testing and Quality
The project follows standard Rust practices:
- `cargo test` - Run unit tests
- `cargo clippy` - Run linting (recent commit mentions clippy fixes)
- `cargo fmt` - Format code

### Core Deployment Commands
- `cargo run -- setup` - Initial setup (creates .env file with configuration)
- `cargo run -- deploy --name <testnet-name>` - Deploy a new testnet
- `cargo run -- inventory --name <testnet-name> --provider digital-ocean` - List machines and testnet info
- `cargo run -- clean --name <testnet-name> --provider digital-ocean` - Tear down testnet
- `cargo run -- status --name <testnet-name>` - Get status of all nodes
- `cargo run -- upgrade --name <testnet-name>` - Upgrade node binaries

### Image Building (using Just)
- `just build-node-image` - Build VM images for nodes
- `just build-client-image` - Build VM images for clients
- `just build-all-images <region>` - Build all VM images for a region

## Architecture

### High-Level Structure
The codebase follows a modular architecture with these main components:

**Core Infrastructure:**
- `src/terraform.rs` - Terraform orchestration for cloud infrastructure
- `src/ansible/` - Ansible automation for VM provisioning and configuration
- `src/infra.rs` - Infrastructure management logic

**Cloud Provider Integration:**
- `src/digital_ocean.rs` - Digital Ocean specific implementations
- Cloud provider abstraction supports AWS and Digital Ocean

**Node and Network Management:**
- `src/bootstrap.rs` - Network bootstrapping from existing deployments
- `src/nodes.rs` - Node lifecycle management (start/stop/upgrade)
- `src/rpc_client.rs` - RPC communication with nodes
- `src/inventory.rs` - Infrastructure inventory management

**Deployment Types:**
- **New Networks:** Complete fresh deployments
- **Bootstrap Deployments:** Extending existing networks
- **Client Deployments:** Deploy client VMs for testing

**Binary Management:**
Two approaches for obtaining binaries:
1. **Versioned:** Pre-built binaries fetched from S3
2. **BuildFromSource:** Custom builds from specified repository/branch

### Key Components

**TestnetDeployer** (src/lib.rs): Main orchestrator that coordinates:
- Terraform for infrastructure
- Ansible for provisioning  
- SSH for remote operations
- S3 for artifact storage

**Command Structure** (src/cmd/): 
- `deployments.rs` - Main deploy/bootstrap/upscale logic
- `nodes.rs` - Node operations (start/stop/status)
- `upgrade.rs` - Binary upgrade processes
- `logs.rs` - Log collection and management

**Node Types:**
- Genesis nodes (bootstrap the network)
- Generic nodes (standard network participants)
- Peer cache nodes (provide cached peer information)
- Private nodes (behind NAT gateways - full-cone and symmetric)

### Configuration Management
- Environment details stored in S3 (deployment type, EVM config, network ID)
- Ansible inventories generated dynamically
- Support for development, staging, and production environments
- VM sizing and node counts determined by environment type

### Resource Organization
- `resources/terraform/` - Infrastructure as code definitions
- `resources/ansible/` - Provisioning playbooks and roles
- `resources/packer/` - VM image building templates
- `resources/scripts/` - Utility scripts for log management

The tool supports complex multi-VM deployments with different node types, NAT configurations, and automatic scaling capabilities.