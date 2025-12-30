# Continuwuity 0.5.0 (2025-12-30)

**This release contains a CRITICAL vulnerability patch, and you must update as soon as possible**

## Features

- Enabled the OLTP exporter in default builds, and allow configuring the exporter protocol. (@Jade). (#1251)

## Bug Fixes

- Don't allow admin room upgrades, as this can break the admin room (@timedout) (#1245)
- Fix invalid creators in power levels during upgrade to v12 (@timedout) (#1245)
