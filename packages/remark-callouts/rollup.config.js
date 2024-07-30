// rollup.config.js
import typescript from '@rollup/plugin-typescript';
import { dts } from "rollup-plugin-dts";
import pkg from './package.json' with { type: "json" };

export default [
    {
        input: 'src/index.ts',
        output: [
            { file: pkg.exports.require, format: 'cjs' },
            { file: pkg.exports.import, format: 'es' }
        ],
        plugins: [typescript()]
    },
    {
        input: 'src/index.ts',
        output: [{ file: pkg.exports.types, format: 'es' }],
        plugins: [dts()],
    }
];