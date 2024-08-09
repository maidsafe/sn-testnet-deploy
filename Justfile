#!/usr/bin/env just --justfile

build-staging-bootstrap-image:
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-1vcpu-2gb' node.pkr.hcl
  )

build-staging-node-image:
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-2vcpu-4gb' node.pkr.hcl
  )

build-staging-uploader-image:
  #!/usr/bin/env bash
  (
    cd resources/packer/node
    packer init .
    packer build -var 'size=s-2vcpu-4gb' node.pkr.hcl
  )
