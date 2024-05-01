import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";
import { ViteImageOptimizer } from "vite-plugin-image-optimizer";
import dynamicImport from 'vite-plugin-dynamic-import'
import typeAsJsonSchemaPlugin from "rollup-plugin-type-as-json-schema";
import dynamicImportVars from '@rollup/plugin-dynamic-import-vars';

import { mdsvex } from 'mdsvex';
import mdsvexConfig from "./mdsvex.config.js";
import { extname } from 'node:path';

function mdsvex_transform() {
    return {
        name: "Mdsvex transformer",
        async transform(code: string, id: string) {
            if (extname(id) !== ".md") return;

            const c = (
                await mdsvex(mdsvexConfig).markup({ content: code, filename: id })
            )?.code;
            return c;
            // return `export default \`${c.replace(/`/g, "\\`").trim()}\`;`;
        }
    };
}
export default defineConfig({
    resolve: {
        alias: {
            "Notes": "node_modules/Notes"
        }
    },
    plugins: [
        typeAsJsonSchemaPlugin(),
        ViteImageOptimizer({
            /* pass your config */
        }),
        // mdsvex_transform(),
        sveltekit(),
        dynamicImport({
        }),
        // dynamicImportVars({
        //   // options
        // })

    ],
    build: {
        assetsInlineLimit: 0,
    },
    optimizeDeps: {
        exclude: [
            "codemirror",
            // "@codemirror/lang-javascript",
            // "@codemirror/state",
            // "@codemirror/lint",
            // "@codemirror/autocomplete",
            // "@codemirror/language",
            // "thememirror"
            /* ... */
        ],
    },
});
