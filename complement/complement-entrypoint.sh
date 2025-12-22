#!/usr/bin/env bash
set -xe
# If we have no $SERVER_NAME set, abort
if [ -z "$SERVER_NAME" ]; then
  echo "SERVER_NAME is not set, aborting"
  exit 1
fi

# If /complement/ca/ca.crt or /complement/ca/ca.key are missing, abort
if [ ! -f /complement/ca/ca.crt ] || [ ! -f /complement/ca/ca.key ]; then
  echo "/complement/ca/ca.crt or /complement/ca/ca.key is missing, aborting"
  exit 1
fi

# Add the root cert to the local trust store
echo 'Installing Complement CA certificate to local trust store'
cp /complement/ca/ca.crt /usr/local/share/ca-certificates/complement-ca.crt
update-ca-certificates

# Sign a certificate for our $SERVER_NAME
echo "Generating and signing certificate for $SERVER_NAME"
openssl genrsa -out "/$SERVER_NAME.key" 2048

echo "Generating CSR for $SERVER_NAME"
openssl req -new -sha256 \
  -key "/$SERVER_NAME.key" \
  -out "/$SERVER_NAME.csr" \
  -subj "/C=US/ST=CA/O=Continuwuity, Inc./CN=$SERVER_NAME"\
  -addext "subjectAltName=DNS:$SERVER_NAME"
openssl req -in "$SERVER_NAME.csr" -noout -text

echo "Signing certificate for $SERVER_NAME with Complement CA"
cat <<EOF > ./cert.ext
authorityKeyIdentifier=keyid,issuer
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment, dataEncipherment, nonRepudiation
extendedKeyUsage = serverAuth
subjectAltName = @alt_names
[alt_names]
DNS.1 = *.docker.internal
DNS.2 = hs1
DNS.3 = hs2
DNS.4 = hs3
DNS.5 = hs4
DNS.6 = $SERVER_NAME
IP.1 = 127.0.0.1
EOF
openssl x509 \
  -req \
  -in "/$SERVER_NAME.csr" \
  -CA /complement/ca/ca.crt \
  -CAkey /complement/ca/ca.key \
  -CAcreateserial \
  -out "/$SERVER_NAME.crt" \
  -days 1 \
  -sha256 \
  -extfile ./cert.ext

# Tell continuwuity where to find the certs
export CONTINUWUITY_TLS__KEY="/$SERVER_NAME.key"
export CONTINUWUITY_TLS__CERTS="/$SERVER_NAME.crt"
# And who it is
export CONTINUWUITY_SERVER_NAME="$SERVER_NAME"

echo "Starting Continuwuity with SERVER_NAME=$SERVER_NAME"
# Start continuwuity
/usr/local/bin/conduwuit --config /etc/continuwuity/config.toml
