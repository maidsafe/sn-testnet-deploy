# Deploy Chunk Trackers - Example Commands

This document provides example commands for deploying chunk tracker services using the `clients deploy-chunk-trackers` command.

## Basic Usage

### Deploy with Versioned Binary

Deploy chunk trackers using a specific version of the ant binary:

```bash
cargo run -- clients deploy-chunk-trackers \
  --name my-testnet \
  --ant-version 0.1.0 \
  --network-id 50 \
  --chunk-tracker-services 3 \
  --chunk-tracker-data-addresses addr1,addr2,addr3 \
  --network-contacts-url https://example.com/network-contacts \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12 \
  --start-chunk-trackers
```

### Deploy with Custom Branch

Deploy chunk trackers using a custom branch for testing:

```bash
cargo run -- clients deploy-chunk-trackers \
  --name my-testnet \
  --branch feature/chunk-tracker-improvements \
  --repo-owner myorg \
  --network-id 50 \
  --chunk-tracker-services 2 \
  --chunk-tracker-data-addresses addr1,addr2 \
  --peer /ip4/127.0.0.1/tcp/12345/p2p/12D3KooWRBhwfeP \
  --evm-network-type custom \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12 \
  --evm-rpc-url https://rpc.example.com
```

### Deploy with Environment Variables

Deploy chunk trackers with custom logging configuration:

```bash
cargo run -- clients deploy-chunk-trackers \
  --name my-testnet \
  --ant-version 0.1.0 \
  --network-id 50 \
  --chunk-tracker-services 1 \
  --chunk-tracker-data-addresses addr1,addr2,addr3,addr4,addr5 \
  --network-contacts-url https://example.com/network-contacts \
  --client-env CLIENT_LOG=all,RUST_LOG=debug \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12
```

## Advanced Options

### Development Environment with Multiple VMs

```bash
cargo run -- clients deploy-chunk-trackers \
  --name dev-chunk-trackers \
  --ant-version 0.1.0 \
  --network-id 100 \
  --environment-type development \
  --client-vm-count 5 \
  --chunk-tracker-services 4 \
  --chunk-tracker-data-addresses addr1,addr2,addr3,addr4,addr5,addr6,addr7,addr8 \
  --network-contacts-url https://example.com/network-contacts \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12 \
  --start-chunk-trackers
```

### Production Environment with Custom VM Size

```bash
cargo run -- clients deploy-chunk-trackers \
  --name prod-chunk-trackers \
  --ant-version 0.2.0 \
  --network-id 1 \
  --environment-type production \
  --client-vm-count 20 \
  --client-vm-size s-4vcpu-8gb \
  --chunk-tracker-services 8 \
  --chunk-tracker-data-addresses addr1,addr2,addr3,addr4,addr5 \
  --network-contacts-url https://network.autonomi.com/contacts \
  --disable-metrics \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12 \
  --start-chunk-trackers
```

### AWS Deployment with Ansible Customization

```bash
cargo run -- clients deploy-chunk-trackers \
  --name aws-chunk-trackers \
  --ant-version 0.1.5 \
  --network-id 50 \
  --provider aws \
  --region us-east-1 \
  --chunk-tracker-services 2 \
  --chunk-tracker-data-addresses addr1,addr2,addr3 \
  --network-contacts-url https://example.com/network-contacts \
  --ansible-verbose \
  --forks 100 \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12
```

### Skip Binary Build (Re-run Failed Deployment)

```bash
cargo run -- clients deploy-chunk-trackers \
  --name my-testnet \
  --branch feature/new-feature \
  --repo-owner myorg \
  --network-id 50 \
  --chunk-tracker-services 2 \
  --chunk-tracker-data-addresses addr1,addr2 \
  --network-contacts-url https://example.com/network-contacts \
  --skip-binary-build \
  --evm-network-type arbitrum-one \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12
```

## Key Arguments

### Required Arguments

- `--name`: Name of the deployment environment
- `--network-id`: Network ID for the ant binary (1-255)
- `--evm-network-type`: EVM network type (arbitrum-one or custom)
- `--evm-data-payments-address`: Address of the data payments contract
- `--evm-payment-token-address`: Address of the payment token contract

### Binary Source (Mutually Exclusive)

**Option 1: Versioned Binary**
- `--ant-version`: Version number (without 'v' prefix)

**Option 2: Build from Source**
- `--branch`: GitHub branch name
- `--repo-owner`: GitHub repository owner/organization
- `--chunk-size`: Optional chunk size for custom builds

### Network Connection (Choose One)

- `--network-contacts-url`: URL to network contacts file
- `--peer`: Multiaddr of a peer to connect to

### Chunk Tracker Configuration

- `--chunk-tracker-services`: Number of tracker services per VM (default: 1)
- `--chunk-tracker-data-addresses`: Comma-separated list of data addresses to track
- `--start-chunk-trackers`: Start services immediately after provisioning

### Infrastructure Options

- `--environment-type`: development, staging, or production (default: development)
- `--client-vm-count`: Number of client VMs to create
- `--client-vm-size`: Override the VM size
- `--provider`: Cloud provider (aws or digital-ocean, default: digital-ocean)
- `--region`: Deployment region (default: lon1 for Digital Ocean)

### Other Options

- `--client-env`: Environment variables for the client (e.g., CLIENT_LOG=all,RUST_LOG=debug)
- `--disable-metrics`: Disable metrics collection
- `--ansible-verbose`: Enable verbose Ansible output
- `--forks`: Maximum Ansible forks (default: 50)
- `--skip-binary-build`: Skip binary build if already built

## Custom EVM Network Example

When using a custom EVM network, you must provide all EVM-related arguments:

```bash
cargo run -- clients deploy-chunk-trackers \
  --name custom-evm-trackers \
  --ant-version 0.1.0 \
  --network-id 50 \
  --chunk-tracker-services 2 \
  --chunk-tracker-data-addresses addr1,addr2,addr3 \
  --network-contacts-url https://example.com/network-contacts \
  --evm-network-type custom \
  --evm-data-payments-address 0x1234567890abcdef1234567890abcdef12345678 \
  --evm-payment-token-address 0xabcdef1234567890abcdef1234567890abcdef12 \
  --evm-rpc-url https://custom-rpc.example.com
```

## Notes

1. The `--branch` and `--repo-owner` arguments must be used together
2. Version arguments and branch/repo-owner arguments are mutually exclusive
3. For custom EVM networks, all three EVM arguments (data-payments-address, payment-token-address, rpc-url) are required
4. Chunk tracker services will select data addresses from the provided list based on their service number
5. If no data addresses are provided, services will randomly select from CSV files
6. Use `--skip-binary-build` when re-running a failed deployment with the same binary configuration
