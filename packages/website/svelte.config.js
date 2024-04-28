import adapter from '@sveltejs/adapter-node';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import htmlMinifierAdaptor from "sveltekit-html-minifier";

import cspDirectives from './csp.js';
/** @type {import('@sveltejs/kit').Config} */


const config = {
	// Consult https://kit.svelte.dev/docs/integrations#preprocessors
	// for more information about preprocessors
	preprocess: vitePreprocess(),

	kit: {
		// adapter-auto only supports some environments, see https://kit.svelte.dev/docs/adapter-auto for a list.
		// If your environment is not supported or you settled on a specific environment, switch out the adapter.
		// See https://kit.svelte.dev/docs/adapters for more information about adapters.
		adapter: 
		// Disabled html min as it runs after precompress
		// htmlMinifierAdaptor(
			adapter(),
			// {
			// 	pages: "build/prerendered/",
			// 	minifierOptions: {
			// 		minifyURLs: true,

			// 		// Because of CSP hashing
			// 		minifyJS: false, // We can pass a function here
			// 		collapseWhitespace: false,
			// 		sortAttributes: true,
			// 		sortClassName: true,
			// 		removeRedundantAttributes: true
			// 	}
			// }),
		csp: {
			mode: "auto",
			directives: cspDirectives,
		},
	}
};

export default config;
