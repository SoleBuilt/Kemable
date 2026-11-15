// Kemable's website (docs/decisions/0030-website.md): static pages, served by
// Cloudflare Pages. React only for the few parts that run in the browser.
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";

export default defineConfig({
  // The Pages address until a domain is bought; the app opens /terms and /privacy here.
  site: "https://kemable.pages.dev",
  // terms.html, not terms/index.html: Pages serves it at /terms with no redirect.
  build: { format: "file" },
  trailingSlash: "never",
  integrations: [react()],
  vite: { plugins: [tailwindcss()] },
});
