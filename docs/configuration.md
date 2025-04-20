# Configuration

This chapter describes various ways to configure Continuwuity.

## Basics

Continuwuity uses a config file for the majority of the settings, but also supports
setting individual config options via commandline.

Please refer to the [example config
file](./configuration/examples.md#example-configuration) for all of those
settings.

The config file to use can be specified on the commandline when running
Continuwuity by specifying the `-c`, `--config` flag. Alternatively, you can use
the environment variable `CONDUWUIT_CONFIG` to specify the config file to used.
Conduit's environment variables are supported for backwards compatibility.

## Option commandline flag

Continuwuity supports setting individual config options in TOML format from the
`-O` / `--option` flag. For example, you can set your server name via `-O
server_name=\"example.com\"`.

Note that the config is parsed as TOML, and shells like bash will remove quotes.
So unfortunately it is required to escape quotes if the config option takes a
string. This does not apply to options that take booleans or numbers:
- `--option allow_registration=true` works ✅
- `-O max_request_size=99999999` works ✅
- `-O server_name=example.com` does not work ❌
- `--option log=\"debug\"` works ✅
- `--option server_name='"example.com'"` works ✅

## Execute commandline flag

Continuwuity supports running admin commands on startup using the commandline
argument `--execute`. The most notable use for this is to create an admin user
on first startup.

The syntax of this is a standard admin command without the prefix such as
`./conduwuit --execute "users create_user june"`

An example output of a success is:
```
INFO conduwuit_service::admin::startup: Startup command #0 completed:
Created user with user_id: @june:girlboss.ceo and password: `<redacted>`
```

This commandline argument can be paired with the `--option` flag.

## Environment variables

All of the settings that are found in the config file can be specified by using
environment variables. The environment variable names should be all caps and
prefixed with `CONDUWUIT_`.

For example, if the setting you are changing is `max_request_size`, then the
environment variable to set is `CONDUWUIT_MAX_REQUEST_SIZE`.

To modify config options not in the `[global]` context such as
`[global.well_known]`, use the `__` suffix split: `CONDUWUIT_WELL_KNOWN__SERVER`

Conduit's environment variables are supported for backwards compatibility (e.g.
`CONDUIT_SERVER_NAME`).
