# Continuwuity for Docker

## Docker

To run Continuwuity with Docker you can either build the image yourself or pull it
from a registry.

### Use a registry

OCI images for Continuwuity are available in the registries listed below.

| Registry        | Image                                                           | Notes                  |
| --------------- | --------------------------------------------------------------- | -----------------------|
| Forgejo Registry| [forgejo.ellis.link/continuwuation/continuwuity:latest][fj]     | Latest tagged image.   |
| Forgejo Registry| [forgejo.ellis.link/continuwuation/continuwuity:main][fj]       | Main branch image.     |

[fj]: https://forgejo.ellis.link/continuwuation/-/packages/container/continuwuity

Use

```bash
docker image pull $LINK
```

to pull it to your machine.

### Run

When you have the image you can simply run it with

```bash
docker run -d -p 8448:6167 \
    -v db:/var/lib/conduwuit/ \
    -e CONDUWUIT_SERVER_NAME="your.server.name" \
    -e CONDUWUIT_ALLOW_REGISTRATION=false \
    --name conduwuit $LINK
```

or you can use [docker compose](#docker-compose).

The `-d` flag lets the container run in detached mode. You may supply an
optional `conduwuit.toml` config file, the example config can be found
[here](../configuration/examples.md). You can pass in different env vars to
change config values on the fly. You can even configure Continuwuity completely by
using env vars. For an overview of possible values, please take a look at the
[`docker-compose.yml`](docker-compose.yml) file.

If you just want to test Continuwuity for a short time, you can use the `--rm`
flag, which will clean up everything related to your container after you stop
it.

### Docker-compose

If the `docker run` command is not for you or your setup, you can also use one
of the provided `docker-compose` files.

Depending on your proxy setup, you can use one of the following files;

- If you already have a `traefik` instance set up, use
[`docker-compose.for-traefik.yml`](docker-compose.for-traefik.yml)
- If you don't have a `traefik` instance set up and would like to use it, use
[`docker-compose.with-traefik.yml`](docker-compose.with-traefik.yml)
- If you want a setup that works out of the box with `caddy-docker-proxy`, use
[`docker-compose.with-caddy.yml`](docker-compose.with-caddy.yml) and replace all
`example.com` placeholders with your own domain
- For any other reverse proxy, use [`docker-compose.yml`](docker-compose.yml)

When picking the traefik-related compose file, rename it so it matches
`docker-compose.yml`, and rename the override file to
`docker-compose.override.yml`. Edit the latter with the values you want for your
server.

When picking the `caddy-docker-proxy` compose file, it's important to first
create the `caddy` network before spinning up the containers:

```bash
docker network create caddy
```

After that, you can rename it so it matches `docker-compose.yml` and spin up the
containers!

Additional info about deploying Continuwuity can be found [here](generic.md).

### Build

Official Continuwuity images are built using **Docker Buildx** and the Dockerfile found at [`docker/Dockerfile`][dockerfile-path]. This approach uses common Docker tooling and enables multi-platform builds efficiently.

The resulting images are broadly compatible with Docker and other container runtimes like Podman or containerd.

The images *do not contain a shell*. They contain only the Continuwuity binary, required libraries, TLS certificates and metadata. Please refer to the [`docker/Dockerfile`][dockerfile-path] for the specific details of the image composition.

To build an image locally using Docker Buildx, you can typically run a command like:

```bash
# Build for the current platform and load into the local Docker daemon
docker buildx build --load --tag continuwuity:latest -f docker/Dockerfile .

# Example: Build for specific platforms and push to a registry.
# docker buildx build --platform linux/amd64,linux/arm64 --tag registry.io/org/continuwuity:latest -f docker/Dockerfile . --push

# Example: Build binary optimized for the current CPU
# docker buildx build --load --tag continuwuity:latest --build-arg TARGET_CPU=native -f docker/Dockerfile .
```

Refer to the Docker Buildx documentation for more advanced build options.

[dockerfile-path]: ../../docker/Dockerfile

### Run

If you already have built the image or want to use one from the registries, you
can just start the container and everything else in the compose file in detached
mode with:

```bash
docker compose up -d
```

> **Note:** Don't forget to modify and adjust the compose file to your needs.

### Use Traefik as Proxy

As a container user, you probably know about Traefik. It is a easy to use
reverse proxy for making containerized app and services available through the
web. With the two provided files,
[`docker-compose.for-traefik.yml`](docker-compose.for-traefik.yml) (or
[`docker-compose.with-traefik.yml`](docker-compose.with-traefik.yml)) and
[`docker-compose.override.yml`](docker-compose.override.yml), it is equally easy
to deploy and use Continuwuity, with a little caveat. If you already took a look at
the files, then you should have seen the `well-known` service, and that is the
little caveat. Traefik is simply a proxy and loadbalancer and is not able to
serve any kind of content, but for Continuwuity to federate, we need to either
expose ports `443` and `8448` or serve two endpoints `.well-known/matrix/client`
and `.well-known/matrix/server`.

With the service `well-known` we use a single `nginx` container that will serve
those two files.

## Voice communication

See the [TURN](../turn.md) page.

[nix-buildlayeredimage]: https://ryantm.github.io/nixpkgs/builders/images/dockertools/#ssec-pkgs-dockerTools-buildLayeredImage
