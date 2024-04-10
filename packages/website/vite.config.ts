import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { ViteImageOptimizer } from 'vite-plugin-image-optimizer';
import typeAsJsonSchemaPlugin from 'rollup-plugin-type-as-json-schema';


export default defineConfig({
	plugins: [
		
        typeAsJsonSchemaPlugin(),
		ViteImageOptimizer({
			/* pass your config */
		  }),
		sveltekit()
	],
	build: {
		assetsInlineLimit: 0
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
			/* ... */],
    },
});
