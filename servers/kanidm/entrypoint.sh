#!/bin/bash

set -e

# if [ ! -f /data/certs/cert.pem ]; then
#     echo "Generating certs"
#     /sbin/kanidmd cert-generate -c /data/server.toml
# fi

/sbin/kanidmd server -c /data/server.toml