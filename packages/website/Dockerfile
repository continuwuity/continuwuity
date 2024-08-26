# build the sapper app
FROM node:latest AS base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
ENV CI=1
RUN corepack enable
COPY ./../.. /app/
WORKDIR /app

# FROM base AS prod-deps
# RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --prod --frozen-lockfile

FROM base AS deps
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile
# RUN cd packages/website; --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile
FROM deps as build
ENV NODE_OPTIONS="--max-old-space-size=4096"
RUN cd packages/website; pnpm run build

RUN cd packages/website; node server-esbuild.js

FROM node:alpine

WORKDIR /app

COPY --from=build /app/packages/website/output .
COPY --from=build /app/packages/website/build/client ./client/
COPY --from=build /app/packages/website/build/prerendered ./prerendered/
COPY --from=base /app/packages/website/package.json ./package.json

ENV NODE_ENV production
EXPOSE 3000
CMD ["node", "."]