// rollup.config.mjs
import { nodeResolve } from '@rollup/plugin-node-resolve';
import commonjs from '@rollup/plugin-commonjs';
import json from "@rollup/plugin-json";
import { sentryRollupPlugin } from "@sentry/rollup-plugin";

export default {
    input: 'build/index.js',
    output: {
        dir: "output",
        format: 'esm',
        sourcemap: true,
    },
    // external: id => id.startsWith("@resvg/resvg-js-"),
    external: ["@resvg/resvg-js"],
    plugins: [
        nodeResolve(), json(), commonjs(),
        sentryRollupPlugin({
            org: "jade-ellis",
            project: "jade-website-sveltekit",
            authToken: process.env.SENTRY_AUTH_TOKEN,
        }),
    ]
};