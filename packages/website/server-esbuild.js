import { sentryEsbuildPlugin } from "@sentry/esbuild-plugin";
import esbuild from "esbuild";

// https://github.com/evanw/esbuild/pull/2067
const ESM_REQUIRE_SHIM = `
await (async () => {
  const { dirname } = await import("path");
  const { fileURLToPath } = await import("url");

  /**
   * Shim entry-point related paths.
   */
  if (typeof globalThis.__filename === "undefined") {
    globalThis.__filename = fileURLToPath(import.meta.url);
  }
  if (typeof globalThis.__dirname === "undefined") {
    globalThis.__dirname = dirname(globalThis.__filename);
  }
  /**
   * Shim require if needed.
   */
  if (typeof globalThis.require === "undefined") {
    const { default: module } = await import("module");
    globalThis.require = module.createRequire(import.meta.url);
  }
})();
`;
const banner = {
    "js": ESM_REQUIRE_SHIM
};

esbuild.build({
    sourcemap: true, // Source map generation must be turned on
    platform: "node", // Node.js platform
    target: "node22.0", // Node.js version
    entryPoints: ["./build/index.js"], // Entry point file
    outdir: "./output", // Output directory
    bundle: true, // Generate an external bundle
    splitting: true, // Enable code splitting
    format: "esm", // Output format
    loader: {
        ".node": "copy",
    },
    alias: {
        "perf_hooks": "node:perf_hooks",
    },
    banner,
    plugins: [
        // Put the Sentry esbuild plugin after all other plugins
        sentryEsbuildPlugin({
            org: "jade-ellis",
            project: "jade-website-sveltekit",
            authToken: process.env.SENTRY_AUTH_TOKEN,
            sourcemaps: {
                // Specify the directory containing build artifacts
                assets: "./output/**",
            } 
        }),
    ],
});