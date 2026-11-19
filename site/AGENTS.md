# Kemable's website

Static pages built with Astro, React and Tailwind, served by a Cloudflare Worker with static assets: the home page, the Terms and the Privacy Policy. The root `AGENTS.md` holds the rules every folder shares; `docs/decisions/0030-website.md` has why this stack and how it is deployed. `README.md` here has the deploy steps.

## Principles

1. **Fast is the feature here too.** Every page is plain HTML and CSS. JavaScript ships only for a part that needs the browser, as a React island loaded `client:visible` or `client:idle`, never `client:load` above the fold; the one script on every page is `Measure.astro`'s, small and deferred, and Google's and Meta's load only after consent. Images are already sized and compressed in `public/`; each `<img>` and `<video>` has its `width` and `height`, and images below the fold load `lazy`.
2. **Nothing for a third party without consent.** Fonts are bundled (`@fontsource-variable/geist`), no embeds. Visits are counted by our own Worker with no cookie and no identifier; cookies, Google Analytics and the Meta Pixel load only after a visitor allows them in the notice (`src/components/Measure.astro`, `docs/decisions/0034-site-worker-and-measuring.md`). The Privacy Policy lists every cookie, tool and field; a change that adds one changes that page in the same change.
3. **The pages say what the app does.** The Terms and the Privacy Policy describe the app's real behaviour, from the ADRs they name in their front matter. When the app changes what it sends or keeps, the page changes with it, with its date.
4. **The app's look.** Black and white with the icon's ember, Geist, dark by default. The tokens are in `src/styles/global.css` (`--bg`, `--fg`, `--line`, `--ember` and the rest), used through Tailwind's `bg-bg`, `text-fg2`, `border-line` and so on; no colour outside them.

## Map

- `src/pages/`: `index.astro` (the home page), `terms.astro`, `privacy.astro`, `security.astro` (what the app does to keep secrets and connections safe; it points to `SECURITY.md` for reporting). A file here is a page at its name: `terms.astro` is `/terms` (`build.format: "file"`, so it is served with no redirect).
- `src/layouts/Base.astro`: the head (the link preview `og.png`, the page's schema.org data), the bar at the top and the footer. `src/links.ts`: the addresses more than one page uses.
- `src/seo.ts`: what search engines and AI assistants read about Kemable: the summary, the engines, the home page's Questions and its schema.org data (`SoftwareApplication`, `FAQPage`). `src/pages/llms.txt.ts` (`/llms.txt`, the site in Markdown for AI assistants), `robots.txt.ts` (every crawler welcome) and `sitemap.xml.ts` (a new page is listed there) are built from it. An answer states what the app does today; a change to the app that makes one untrue changes it.
- `src/components/`: `ThemeToggle.tsx` (the one React island: dark or light, kept in the browser's storage), `DraftNotice.astro` (the note at the top of the Terms and the Privacy Policy until a lawyer has read them).
- `public/`: copied as is: the app's icon (`app-icon.svg`, the favicon and the logo; `apple-touch-icon.png`, 180 px), `og.png` (the 1200 by 630 link preview: the icon, the headline and the demo's poster), the screenshots and the demo video (H.264 only: a VP9 webm failed in GPU decoders), `_headers` (the headers Cloudflare serves the files with: long caching for `/_astro/*`).
- `src/pages/waitlist/`: `joined.astro` and `failed.astro`, where the waitlist's form leads (`noindex`, not in the sitemap).
- `src/components/Measure.astro`: the cookie notice and the one script on every page: a beacon per arrival and per Download click, the waitlist's hidden source fields, and Google Analytics and the Meta Pixel after consent. `src/analytics.ts`: their ids (empty leaves a tool out) and what the notice says.
- `worker/`: the Worker, run for `/api/*` only. `index.ts`: `POST /api/waitlist` and `POST /api/visit`, into D1; it declares the few Cloudflare types it uses instead of a types package. `source.ts`: where a visit came from (UTM tags, click ids, the Referer), with `source.test.ts` (`pnpm test`). `migrations/`: the D1 tables. `wrangler.toml`: the Worker, its assets, its domain and its D1 binding. `pnpm dev` does not run the Worker; `README.md` says how to try it.
- `astro.config.mjs`: the site's address (`site`), the Worker's custom domain, which the app opens too (`README.md`, The domain).
- `pnpm-workspace.yaml`: packages at least two weeks old (`minimumReleaseAge`), and no `sharp`.

## Commands

From `site/`:

```bash
pnpm install
pnpm dev        # http://localhost:4321 with hot reload
pnpm check      # astro check: types in .astro and .tsx
pnpm build      # the pages into dist/
pnpm test       # worker/source.test.ts
pnpm worker     # the Worker on :8787 with a local D1; `pnpm dev` forwards /api/* to it
```

`node scripts/verify.mjs` installs, tests and builds the site; `.github/workflows/site.yml` also runs `pnpm check`, then deploys from `main`. To run the Worker with the pages: `README.md`, On this PC.

## Dependencies

Exact versions, each at least two weeks old when added. Licenses permissive, as for the app (`docs/decisions/0023-licensing.md`): check with `pnpm licenses list --prod`. Nothing from here ships inside the app, so the site's packages are not in `ui/windows/THIRD-PARTY-NOTICES.md`.
