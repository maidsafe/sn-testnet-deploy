#!/usr/bin/env bash

# Clear tmpdir if it exists
if [ -d "tmpdir" ]; then
  rm -r tmpdir/*
else
  mkdir tmpdir
fi

# Find all .log files and copy them to tmpdir with the same directory structure
rsync -avm --include='*.log*' -f 'hide,! */' ~/.local/share/safe/node/ tmpdir/