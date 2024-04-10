// rollup.config.mjs
import { nodeResolve } from '@rollup/plugin-node-resolve';

export default {
    input: 'build/index.js',
    output: {
        dir: "output",
        format: 'esm'
    },
    plugins: [nodeResolve()]
};