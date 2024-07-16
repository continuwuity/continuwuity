import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";
import { ViteImageOptimizer } from "vite-plugin-image-optimizer";
import dynamicImport from 'vite-plugin-dynamic-import'
import typeAsJsonSchemaPlugin from "rollup-plugin-type-as-json-schema";
// import dynamicImportVars from '@rollup/plugin-dynamic-import-vars';
import path from "node:path";
import { mdsvex } from 'mdsvex';
import mdsvexConfig from "./mdsvex.config.js";
import { extname } from 'node:path';

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
const fallback: {[key: string]: string} = {
	'.avif': 'png',
	'.gif': 'gif',
	'.heif': 'jpg',
	'.jpeg': 'jpg',
	'.jpg': 'jpg',
	'.png': 'png',
	'.tiff': 'jpg',
	'.webp': 'png'
};

export default defineConfig({
    resolve: {
        alias: {
            "Notes": path.join(__dirname, "node_modules/Notes")
        }
    },
    plugins: [
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
