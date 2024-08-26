import { sentrySvelteKit } from "@sentry/sveltekit";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig, type PluginOption } from "vite";
import { ViteImageOptimizer } from "vite-plugin-image-optimizer";
import dynamicImport from 'vite-plugin-dynamic-import'
import typeAsJsonSchemaPlugin from "rollup-plugin-type-as-json-schema";
// import dynamicImportVars from '@rollup/plugin-dynamic-import-vars';
import path from "node:path";
import { mdsvex } from 'mdsvex';
import mdsvexConfig from "./mdsvex.config.js";
import { extname, relative } from 'node:path';
// import { realpath } from 'node:fs';

import { thumbHash } from 'vite-plugin-thumbhash-svg'
// import { imagetools } from 'vite-imagetools'

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

import { resolve, dirname } from 'node:path';
import { createFilter } from '@rollup/pluginutils'
type Options =
    | {
        include?: Array<string | RegExp> | string | RegExp
        exclude?: Array<string | RegExp> | string | RegExp
        rootdir?: string
    }
    | undefined
function relativeResolver({ include, exclude, rootdir: rootDirCfg }: Options = {}): import('vite').Plugin {
    const rootDir = resolve(rootDirCfg || process.cwd());
    const filter = createFilter(include, exclude)
    // console.log(rootDir)
    return {
        name: "relative resolver",
        async resolveId(file, origin, opt) {
            // if (file.includes("Design")) {
            //     console.log(file, origin, !filter(origin), opt.isEntry)
            // }

            if (opt.isEntry) return

            if (!filter(origin)) {
                // console.log(origin, "not filter")
                return null
            }

            // console.log("relatively resolving")
            // console.log(relative(rootDir, resolve(dirname(origin as string), decodeURIComponent(file))))
            // if (!isThumbHash(file)) return 
            // Your local include path must either starts with `./` or `../`
            // if (file.startsWith('./') || file.startsWith('../')) {
            // console.log(file, 'from', origin, 'to', resolve(dirname(origin as string), file))
            // Return an absolute include path
            return relative(rootDir, resolve(dirname(origin as string), decodeURIComponent(file)));
            // }
            return null; // Continue to the next plugins!
        },
    }
}
export const blurRE = /(\?|&)blurhash(?:&|$)/
const isThumbHash = (id: string) => {
    return id.endsWith('?th') || id.endsWith('?thumb')
}
function blurhash_transform() {
    return {
        name: "blurhash transformer",
        async transform(code: string, id: string) {

            // if (!blurRE.test(id)) return; 
            if (!isThumbHash(id)) return;
            // console.log(id, code)
            // console.log(id.includes("blurhash"), id)
            return code;
            // return `export default \`${c.replace(/`/g, "\\`").trim()}\`;`;
        }
    };
}
const fallback: { [key: string]: string } = {
    '.avif': 'png',
    '.gif': 'gif',
    '.heif': 'jpg',
    '.jpeg': 'jpg',
    '.jpg': 'jpg',
    '.png': 'png',
    '.tiff': 'jpg',
    '.webp': 'png'
};


import { visualizer } from "rollup-plugin-visualizer";

export default defineConfig({
    resolve: {
        alias: {
            "Notes": path.join(__dirname, "node_modules/Notes")
        }
    },
    plugins: [
        sentrySvelteKit({
            sourceMapsUploadOptions: {
                org: "jade-ellis",
                project: "jade-website-sveltekit"
            }
        }),
        // relativeResolver({include: [/node_modules\/Notes/]}),
        // blurhash_transform(),
        typeAsJsonSchemaPlugin(),
        ViteImageOptimizer({
            /* pass your config */
        }),
        // imagetools({
        //     namedExports: false,
        //     defaultDirectives: async (url, metadata) => {
        //         console.log("vite", url)
        //         // if (!url.searchParams.has('svex-enhanced')) return new URLSearchParams();

        //         // const img_width = url.searchParams.get('imgWidth');
        //         // const width = img_width ? parseInt(img_width) : (await metadata()).width;
        //         // if (!width) {
        //         //     console.warn(`Could not determine width of image ${url.href}`);
        //             return new URLSearchParams();
        //         // }

        //         // return new URLSearchParams({
        //         //     'metadata': '',
        //         //     // format: `avif;webp;${fallback[path.extname(url.href)] ?? 'png'}`
        //         // });
        //     },
        // }),
        // mdsvex_transform(),
        sveltekit(),
        dynamicImport({
            filter(id) {
                if (id.includes('node_modules/Notes')) {
                    return true
                }
            }
        }),
        // blurhash_transform(),
        thumbHash({
            // exclude: [/\.svg/]
        }),
        // dynamicImportVars({
        //   // options
        // })
        // visualizer({
        //     emitFile: true,
        //     filename: "stats.html",
        //   }) as PluginOption
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