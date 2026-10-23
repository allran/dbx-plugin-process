import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  plugins: [
    svelte(),
    {
      name: "dbx-build-signal",
      closeBundle() {
        console.log("DBX_UI_BUILD_SUCCESS");
      },
    },
  ],
  build: {
    outDir: "ui",
    emptyOutDir: true,
  },
});
