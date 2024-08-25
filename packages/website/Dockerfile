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
RUN cd packages/website; pnpm run build

RUN cd packages/website; pnpm exec rollup -c server-rollup.config.mjs
# copy node_modules/ and other build files over
FROM node:latest as deploy-deps
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
ENV CI=1
RUN corepack enable

COPY --from=base /app/packages/website/package.json ./package.json
COPY --from=base /app/pnpm-lock.yaml ./pnpm-lock.yaml
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile --prod

FROM node:alpine

WORKDIR /app

COPY --from=build /app/packages/website/output .
COPY --from=build /app/packages/website/build/client ./client/
COPY --from=build /app/packages/website/build/prerendered ./prerendered/
COPY --from=base /app/packages/website/package.json ./package.json
COPY --from=deploy-deps /app/packages/website/node_modules ./node_modules/
COPY --from=deploy-deps /app/node_modules/.pnpm ./node_modules/.pnpm/

ENV NODE_ENV production
EXPOSE 3000
CMD ["node", "."]