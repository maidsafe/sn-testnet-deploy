#!/usr/bin/env bash

mkdir tmpdir
cp -R ~/.local/share/safe/node tmpdir/
find tmpdir -iname '*.log*' | tar -zcvf log_files.tar.gz --files-from -
rm -r tmpdir