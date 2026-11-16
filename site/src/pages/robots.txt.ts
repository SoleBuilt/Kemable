// Every crawler is welcome, AI assistants' too: being read is how Kemable gets
// found and recommended. A host's own setting can still block AI crawlers in
// front of this file (Cloudflare's "Block AI bots"); keep it off.
import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site }) =>
  new Response(`User-agent: *\nAllow: /\n\nSitemap: ${new URL("/sitemap.xml", site)}\n`, {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
