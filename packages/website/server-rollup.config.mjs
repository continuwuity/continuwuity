// rollup.config.mjs
import { nodeResolve } from '@rollup/plugin-node-resolve';
import commonjs from '@rollup/plugin-commonjs';
import json from "@rollup/plugin-json";

export default {
    input: 'build/index.js',
    output: {
        dir: "output",
        format: 'esm'
    },
    // external: id => id.startsWith("@resvg/resvg-js-"),
    external: ["@resvg/resvg-js"],
    plugins: [nodeResolve(), json(), commonjs()]
};