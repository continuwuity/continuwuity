# build the sapper app
FROM node:slim AS base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
COPY ./../.. /app/
WORKDIR /app

# FROM base AS prod-deps
# RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --prod --frozen-lockfile

FROM base AS build
ENV CI=1 
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile
RUN mkdir ../node_modules
RUN cp -Lr node_modules/.pnpm/@resvg+resvg-*@*/node_modules/* ../node_modules/
RUN cp -Lr node_modules/.pnpm/@sentry+sveltekit-*@*/node_modules/* ../node_modules/
# RUN cd packages/website; --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile

RUN cd packages/website; pnpm run build

RUN cd packages/website; pnpm exec rollup -c server-rollup.config.mjs
# copy node_modules/ and other build files over
FROM node:alpine

WORKDIR /app

COPY --from=build /app/packages/website/output .
COPY --from=build /app/packages/website/build/client ./client/
COPY --from=build /app/packages/website/build/prerendered ./prerendered/
COPY --from=build /app/packages/website/package.json ./package.json
COPY --from=build /node_modules ./node_modules/

ENV NODE_ENV production
EXPOSE 3000
CMD ["node", "."]