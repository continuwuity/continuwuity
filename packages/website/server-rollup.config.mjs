// rollup.config.mjs
import { nodeResolve } from '@rollup/plugin-node-resolve';
import commonjs from '@rollup/plugin-commonjs';

export default {
    input: 'build/index.js',
    output: {
        dir: "output",
        format: 'esm'
    },
    plugins: [nodeResolve(), commonjs()]
};