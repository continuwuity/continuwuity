# Continuwuity for Docker

## Docker

To run Continuwuity with Docker, you can either build the image yourself or pull it
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

When you have the image, you can simply run it with

```bash
docker run -d -p 8448:6167 \
    -v db:/var/lib/continuwuity/ \
    -e CONTINUWUITY_SERVER_NAME="your.server.name" \
    -e CONTINUWUITY_ALLOW_REGISTRATION=false \
    --name continuwuity $LINK
```

or you can use [Docker Compose](#docker-compose).

The `-d` flag lets the container run in detached mode. You may supply an
optional `continuwuity.toml` config file, the example config can be found
[here](../configuration/examples.md). You can pass in different env vars to
change config values on the fly. You can even configure Continuwuity completely by
using env vars. For an overview of possible values, please take a look at the
[`docker-compose.yml`](docker-compose.yml) file.

If you just want to test Continuwuity for a short time, you can use the `--rm`
flag, which cleans up everything related to your container after you stop
it.

### Docker-compose

If the `docker run` command is not suitable for you or your setup, you can also use one
of the provided `docker-compose` files.

Depending on your proxy setup, you can use one of the following files:

- If you already have a `traefik` instance set up, use
[`docker-compose.for-traefik.yml`](docker-compose.for-traefik.yml)
- If you don't have a `traefik` instance set up and would like to use it, use
[`docker-compose.with-traefik.yml`](docker-compose.with-traefik.yml)
- If you want a setup that works out of the box with `caddy-docker-proxy`, use
[`docker-compose.with-caddy.yml`](docker-compose.with-caddy.yml) and replace all
`example.com` placeholders with your own domain
- For any other reverse proxy, use [`docker-compose.yml`](docker-compose.yml)

When picking the Traefik-related compose file, rename it to
`docker-compose.yml`, and rename the override file to
`docker-compose.override.yml`. Edit the latter with the values you want for your
server.

When picking the `caddy-docker-proxy` compose file, it's important to first
create the `caddy` network before spinning up the containers:

```bash
docker network create caddy
```

After that, you can rename it to `docker-compose.yml` and spin up the
containers!

Additional info about deploying Continuwuity can be found [here](generic.md).

### Build

Official Continuwuity images are built using **Docker Buildx** and the Dockerfile found at [`docker/Dockerfile`][dockerfile-path]. This approach uses common Docker tooling and enables efficient multi-platform builds.

The resulting images are widely compatible with Docker and other container runtimes like Podman or containerd.

The images *do not contain a shell*. They contain only the Continuwuity binary, required libraries, TLS certificates, and metadata. Please refer to the [`docker/Dockerfile`][dockerfile-path] for the specific details of the image composition.

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

If you have already built the image or want to use one from the registries, you
can start the container and everything else in the compose file in detached
mode with:

```bash
docker compose up -d
```

> **Note:** Don't forget to modify and adjust the compose file to your needs.

### Use Traefik as Proxy

As a container user, you probably know about Traefik. It is an easy-to-use
reverse proxy for making containerized apps and services available through the
web. With the two provided files,
[`docker-compose.for-traefik.yml`](docker-compose.for-traefik.yml) (or
[`docker-compose.with-traefik.yml`](docker-compose.with-traefik.yml)) and
[`docker-compose.override.yml`](docker-compose.override.yml), it is equally easy
to deploy and use Continuwuity, with a small caveat. If you have already looked at
the files, you should have seen the `well-known` service, which is the
small caveat. Traefik is simply a proxy and load balancer and cannot
serve any kind of content. For Continuwuity to federate, we need to either
expose ports `443` and `8448` or serve two endpoints: `.well-known/matrix/client`
and `.well-known/matrix/server`.

With the service `well-known`, we use a single `nginx` container that serves
those two files.

Alternatively, you can use Continuwuity's built-in delegation file capability. Set up the delegation files in the configuration file, and then proxy paths under `/.well-known/matrix` to continuwuity. For example, the label ``traefik.http.routers.continuwuity.rule=(Host(`matrix.ellis.link`) || (Host(`ellis.link`) && PathPrefix(`/.well-known/matrix`)))`` does this for the domain `ellis.link`.

## Voice communication

See the [TURN](../turn.md) page.

[nix-buildlayeredimage]: https://ryantm.github.io/nixpkgs/builders/images/dockertools/#ssec-pkgs-dockerTools-buildLayeredImage
