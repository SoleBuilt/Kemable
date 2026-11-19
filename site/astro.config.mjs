// Kemable's website (docs/decisions/0030-website.md): static pages, served by
// Cloudflare Pages. React only for the few parts that run in the browser.
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";

export default defineConfig({
  // The site's address, the app's too: canonical links, the sitemap, llms.txt and link previews
  // use it, and the Worker answers on it (wrangler.toml; docs/decisions/0030-website.md, The address).
  site: "https://kemable.ducht.info.vn",
  // terms.html, not terms/index.html: Pages serves it at /terms with no redirect.
  build: { format: "file" },
  trailingSlash: "never",
  integrations: [react()],
  vite: {
    plugins: [tailwindcss()],
    // `pnpm dev` serves the pages only; /api/* goes to the Worker of `pnpm worker`, run beside it.
    server: { proxy: { "/api": "http://127.0.0.1:8787" } },
  },
});
