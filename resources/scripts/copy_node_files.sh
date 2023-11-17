#!/usr/bin/env bash

# Clear tmpdir if it exists
if [ -d "tmpdir" ]; then
  rm -r tmpdir/*
else
  mkdir tmpdir
fi

cp -R ~/.local/share/safe/node tmpdir/