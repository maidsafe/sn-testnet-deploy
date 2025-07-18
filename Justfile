#!/usr/bin/env just --justfile

build-rust-build-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/build
    packer init .
    packer build -var 'region={{region}}' build.pkr.hcl
  )

build-evm-node-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/build
    packer init .
    packer build \
      -var 'size=s-4vcpu-8gb' \
      -var 'snapshot_name=evm-node' \
      -var 'region={{region}}' \
      build.pkr.hcl
  )

build-peer-cache-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-1vcpu-2gb' -var 'region={{region}}' node.pkr.hcl
  )

build-node-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-2vcpu-4gb' -var 'region={{region}}' node.pkr.hcl
  )

build-client-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-2vcpu-4gb' -var 'region={{region}}' node.pkr.hcl
  )

build-nat-gateway-image region="lon1":
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-1vcpu-2gb' -var 'region={{region}}' node.pkr.hcl
  )

# This target has been copied from another repository. On other repositories, more than one
# architecture is supported. If we want to extend for other architectures, we can do so.
build-release-artifacts arch:
  #!/usr/bin/env bash
  set -e

  arch="{{arch}}"
  supported_archs=(
    "x86_64-unknown-linux-musl"
  )

  arch_supported=false
  for supported_arch in "${supported_archs[@]}"; do
    if [[ "$arch" == "$supported_arch" ]]; then
      arch_supported=true
      break
    fi
  done

  if [[ "$arch_supported" == "false" ]]; then
    echo "$arch is not supported."
    exit 1
  fi

  if [[ "$arch" == "x86_64-unknown-linux-musl" ]]; then
    if [[ "$(grep -E '^NAME="Ubuntu"' /etc/os-release)" ]]; then
      # This is intended for use on a fresh Github Actions agent
      sudo apt update -y
      sudo apt install -y musl-tools
    fi
    rustup target add x86_64-unknown-linux-musl
  fi

  rm -rf artifacts
  mkdir artifacts
  cargo clean
  cargo build --release --target $arch

  find target/$arch/release -maxdepth 1 -type f -exec cp '{}' artifacts \;
  rm -f artifacts/.cargo-lock

upload-release-assets-to-s3:
  #!/usr/bin/env bash
  set -e

  cd deploy/testnet-deploy
  for file in *.zip *.tar.gz; do
    aws s3 cp "$file" "s3://sn-testnet-deploy/$file" --acl public-read
  done

package-release-assets:
  #!/usr/bin/env bash
  set -e

  architectures=(
    "x86_64-unknown-linux-musl"
  )
  bin="testnet-deploy"
  version=$(cat Cargo.toml | grep "^version" | awk -F '=' '{ print $2 }' | xargs)

  rm -rf deploy/$bin
  find artifacts/ -name "$bin" -exec chmod +x '{}' \;
  for arch in "${architectures[@]}" ; do
    echo "Packaging for $arch..."
    if [[ $arch == *"windows"* ]]; then bin_name="${bin}.exe"; else bin_name=$bin; fi
    zip -j $bin-$version-$arch.zip artifacts/$arch/release/$bin_name
    tar -C artifacts/$arch/release -zcvf $bin-$version-$arch.tar.gz $bin_name
  done

  mkdir -p deploy/$bin
  mv *.tar.gz deploy/$bin
  mv *.zip deploy/$bin

build-all-images region:
  #!/usr/bin/env bash
  set -e
  just build-rust-build-image {{region}}
  just build-evm-node-image {{region}}
  just build-peer-cache-image {{region}}
  just build-node-image {{region}}
  just build-client-image {{region}}
  just build-nat-gateway-image {{region}}

build-lon1-images:
  just build-all-images lon1

build-ams3-images:
  just build-all-images ams3

build-lon1-rust-build-image:
  just build-rust-build-image lon1

build-ams3-rust-build-image:
  just build-rust-build-image ams3

build-lon1-evm-node-image:
  just build-evm-node-image lon1

build-ams3-evm-node-image:
  just build-evm-node-image ams3
