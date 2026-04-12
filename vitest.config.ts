import { sveltekit } from "@sveltejs/kit/vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";
import path from "node:path";

export default defineConfig({
  plugins: [sveltekit()],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, "src/app/lib"),
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});