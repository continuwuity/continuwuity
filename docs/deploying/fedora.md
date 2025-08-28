# RPM Installation Guide

Continuwuity is available as RPM packages for Fedora, RHEL, and compatible distributions.

## Quick install (stable)

For the latest stable version from release tags:

```bash
# Add the Continuwuity repository
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/stable/continuwuation.repo

# Install Continuwuity
sudo dnf install continuwuity

# Enable automatic updates (optional)
sudo dnf install dnf-automatic
sudo systemctl enable --now dnf-automatic.timer
```

## Development builds

For development builds from the main branch:

```bash
# Add the dev repository
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/dev/continuwuation.repo

# Install Continuwuity
sudo dnf install continuwuity
```

## Feature branch builds

Feature branches are published to their own groups. For example, for branch `tom/new-feature`:

```bash
# Add the branch-specific repository (replace branch name)
sudo dnf config-manager addrepo --from-repofile=https://forgejo.ellis.link/api/packages/continuwuation/rpm/tom-new-feature/continuwuation.repo

# Install Continuwuity
sudo dnf install continuwuity
```

Note: Branch names are sanitized (slashes become hyphens, lowercase only).

## Direct package installation

To install a specific version without adding the repository:

```bash
# Latest stable release
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/stable/continuwuity

# Latest development build (main branch)
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/dev/continuwuity

# Specific feature branch (replace branch-name)
sudo dnf install https://forgejo.ellis.link/api/packages/continuwuation/rpm/branch-name/continuwuity
```

## Managing automatic updates

### Option 1: DNF Automatic (recommended)

```bash
# Install dnf-automatic
sudo dnf install dnf-automatic

# Configure update policy
sudo nano /etc/dnf/automatic.conf
# Set: apply_updates = yes

# Enable the service
sudo systemctl enable --now dnf-automatic.timer
```

### Option 2: Manual updates

```bash
# Check for updates
sudo dnf check-update continuwuity

# Update to latest version
sudo dnf update continuwuity
```

## Switching between channels

```bash
# List enabled repositories
dnf repolist | grep continuwuation

# Disable current repository (use actual repo name from above)
sudo dnf config-manager --set-disabled continuwuation-stable
# or
sudo dnf config-manager --set-disabled continuwuation-dev
# or for feature branches
sudo dnf config-manager --set-disabled continuwuation-branch-name

# Enable desired repository
sudo dnf config-manager --set-enabled continuwuation-stable
# or
sudo dnf config-manager --set-enabled continuwuation-dev

# Update to the new channel's version
sudo dnf update continuwuity
```

## Verifying installation

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

## Systemd service management

Continuwuity includes a systemd service file:

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

## Uninstallation

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

### GPG key issues

If you encounter GPG key errors, you can temporarily disable GPG checking:

```bash
sudo dnf --nogpgcheck install continuwuity
```

### Repository metadata issues

Clear and rebuild the cache:

```bash
sudo dnf clean all
sudo dnf makecache
```

### Finding specific versions

List all available versions:

```bash
dnf --showduplicates list continuwuity
```

Install a specific version:

```bash
sudo dnf install continuwuity-<version>
```
