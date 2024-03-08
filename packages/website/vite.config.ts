import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { ViteImageOptimizer } from 'vite-plugin-image-optimizer';


export default defineConfig({
	plugins: [
		ViteImageOptimizer({
			/* pass your config */
		  }),
		sveltekit()
	],
	build: {
		assetsInlineLimit: 0
	}
});
