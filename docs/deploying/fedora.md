# RPM Installation Guide

Continuwuity is available as RPM packages for Fedora, RHEL, and compatible distributions.

The RPM packaging files are maintained in the `fedora/` directory:
- `continuwuity.spec.rpkg` - RPM spec file using rpkg macros for building from git
- `continuwuity.service` - Systemd service file for the server
- `RPM-GPG-KEY-continuwuity.asc` - GPG public key for verifying signed packages

RPM packages built by CI are signed with our GPG key (Ed25519, ID: `5E0FF73F411AAFCA`).

```bash
# Import the signing key
sudo rpm --import https://forgejo.ellis.link/continuwuation/continuwuity/raw/branch/main/fedora/RPM-GPG-KEY-continuwuity.asc

# Verify a downloaded package
rpm --checksig continuwuity-*.rpm
```

## Installation methods

**Stable releases** (recommended)

```bash
# Add the repository and install
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/stable/continuwuation.repo
sudo dnf install continuwuity
```

**Development builds** from main branch

```bash
# Add the dev repository and install
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/dev/continuwuation.repo
sudo dnf install continuwuity
```

**Feature branch builds** (example: `tom/new-feature`)

```bash
# Branch names are sanitized (slashes become hyphens, lowercase only)
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/tom-new-feature/continuwuation.repo
sudo dnf install continuwuity
```

**Direct installation** without adding repository

```bash
# Latest stable release
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/stable/continuwuity

# Latest development build
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/dev/continuwuity

# Specific feature branch
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/branch-name/continuwuity
```

**Manual repository configuration** (alternative method)

```bash
cat << 'EOF' | sudo tee /etc/yum.repos.d/continuwuity.repo
[continuwuity]
name=Continuwuity - Matrix homeserver
baseurl=https://forgejo.ellis.link/api/packages/continuwuation/rpm/stable
enabled=1
gpgcheck=1
gpgkey=https://forgejo.ellis.link/continuwuation/continuwuity/raw/branch/main/fedora/RPM-GPG-KEY-continuwuity.asc
EOF

sudo dnf install continuwuity
```

## Package management

**Automatic updates** with DNF Automatic

```bash
# Install and configure
sudo dnf install dnf-automatic
sudo nano /etc/dnf/automatic.conf  # Set: apply_updates = yes
sudo systemctl enable --now dnf-automatic.timer
```

**Manual updates**

```bash
# Check for updates
sudo dnf check-update continuwuity

# Update to latest version
sudo dnf update continuwuity
```

**Switching channels** (stable/dev/feature branches)

```bash
# List enabled repositories
dnf repolist | grep continuwuation

# Disable current repository
sudo dnf config-manager --set-disabled continuwuation-stable  # or -dev, or branch name

# Enable desired repository
sudo dnf config-manager --set-enabled continuwuation-dev  # or -stable, or branch name

# Update to the new channel's version
sudo dnf update continuwuity
```

**Verifying installation**

```bash
# Check installed version
rpm -q continuwuity

# View package information
rpm -qi continuwuity

# List installed files
rpm -ql continuwuity

# Verify package integrity
rpm -V continuwuity
```

## Service management and removal

**Systemd service commands**

```bash
# Start the service
sudo systemctl start conduwuit

# Enable on boot
sudo systemctl enable conduwuit

# Check status
sudo systemctl status conduwuit

# View logs
sudo journalctl -u conduwuit -f
```

**Uninstallation**

```bash
# Stop and disable the service
sudo systemctl stop conduwuit
sudo systemctl disable conduwuit

# Remove the package
sudo dnf remove continuwuity

# Remove the repository (optional)
sudo rm /etc/yum.repos.d/continuwuation-*.repo
```

## Troubleshooting

**GPG key errors**: Temporarily disable GPG checking

```bash
sudo dnf --nogpgcheck install continuwuity
```

**Repository metadata issues**: Clear and rebuild cache

```bash
sudo dnf clean all
sudo dnf makecache
```

**Finding specific versions**

```bash
# List all available versions
dnf --showduplicates list continuwuity

# Install a specific version
sudo dnf install continuwuity-<version>
```

## Building locally

Build the RPM locally using rpkg:

```bash
# Install dependencies
sudo dnf install rpkg rpm-build cargo-rpm-macros systemd-rpm-macros

# Clone the repository
git clone https://forgejo.ellis.link/continuwuation/continuwuity.git
cd continuwuity

# Build SRPM
rpkg srpm

# Build RPM
rpmbuild --rebuild *.src.rpm
```
