#!/usr/bin/env bash

rg "is listening on " ~/.local/share/safe/node | \
  rg -v "ip4/10." | rg -v "ip4/127." | sort -k1.90,1.119 | head -n +1 | \
  sed -n 's/.*"\(.*\)".*/\1/p'
