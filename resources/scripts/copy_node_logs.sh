#!/usr/bin/env bash

# Not used, pair it with logs rsync if we need some resiliency there.

# Clear tmpdir if it exists
if [ -d "tmpdir" ]; then
  rm -r tmpdir/*
else
  mkdir tmpdir
fi

# Find all .log files and copy them to tmpdir preserving the same directory structure
rsync -avm --include='*.log*' -f 'hide,! */' /var/log/safenode/ tmpdir/
