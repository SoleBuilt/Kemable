# 0030: Kemable's website lives in `site/`: static Astro pages on Cloudflare Pages, with the Terms and the Privacy Policy

- **Status**: Accepted
- **Date**: 2026-11-15
- **Amends**: `docs/decisions/0019-data-folder.md` and `docs/decisions/0023-licensing.md` (where the terms' links lead). Decided with the user on 2026-11-15 (`docs/preview-audit.md`, C17).

## Context

The terms dialog (ADR 0019) and Settings' links needed pages Kemable does not have: its Terms and Privacy Policy were placeholder pages, so the dialog was hidden (ADR 0023). The first preview also needs a page to send people to: what the app is, a download, the source. The design was made and approved on 2026-11-15: one dark page with a demo video, the engines, three measured numbers, three features with screenshots, four small cards and a closing call to download.

The user decided: a site of Kemable's own, in this repository, built with Astro, React and Tailwind, served by Cloudflare Pages; no payments until there is something to sell; the domain bought later, the Pages address until then. SoleBuilt's site links to it.

Measured on 2026-11-15 on the development PC, with other builds running: `pnpm install --frozen-lockfile` from pnpm's store 2 to 3 s; `pnpm build` 3 to 11 s for the three pages; `astro check` 40 to 85 s. The built home page is 18 KB of HTML and 20 KB of CSS, with no JavaScript until the footer scrolls into view.

## Decision

- **`site/`** at the repository's root, its own pnpm project (`site/package.json`, `site/pnpm-lock.yaml`), apart from `ui/windows`. `site/AGENTS.md` is its guide; `site/README.md` its deploy steps.
- **Astro 7, static output.** Every page is HTML and CSS built ahead. React (`@astrojs/react`) only for a part that needs the browser, as an island loaded when visible or idle; today one, the theme switch in the footer. Tailwind 4 through its Vite plugin, over the app's tokens (black and white, the icon's ember, Geist), dark by default.
- **The pages**: `/` (the approved design, ported), `/terms`, `/privacy` and, since 2026-11-18, `/security` (how the app keeps passwords, connections and updates safe, from ADRs 0004, 0024, 0028 and 0033, and how to report a vulnerability, pointing to `SECURITY.md`). The Terms and the Privacy Policy describe what the app does, from the ADRs that own each fact (0019 the data folder, 0028 the update check, 0015 and 0024 Credential Manager, 0023 the licenses), and say at the top that they are drafts for legal review. `build.format: "file"`, so `/terms` is served as is, with no redirect to `/terms/`.
- **Nothing for a third party without consent.** The fonts are bundled from `@fontsource-variable/geist`, not loaded from a font service. The theme choice is kept in the browser's storage. Since 2026-11-18 the site counts where visitors come from with no cookie, and sets cookies and loads Google Analytics and the Meta Pixel only for a visitor who allows them (`docs/decisions/0034-site-worker-and-measuring.md`).
- **Exact versions, two weeks old**: each dependency is pinned without a range, at a version published at least two weeks before it is added, and `minimumReleaseAge` in `site/pnpm-workspace.yaml` holds every package, transitive ones too, to the same. `sharp` is not installed (`ignoredOptionalDependencies`): images are already sized and compressed in `site/public/`.
- **Served by Cloudflare**, as the Worker `kemable` with static assets since 2026-11-18 (`docs/decisions/0034-site-worker-and-measuring.md`; Pages until then). `.github/workflows/site.yml` checks the types, tests and builds on every pull request that touches `site/`, and on a push to `main` deploys with Wrangler when the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets exist; without them it builds and says it did not deploy.
- **Checked on every change**: `node scripts/verify.mjs` installs the site with its frozen lockfile and builds it (with the UI steps). Both are fast and offline once the packages are in pnpm's store. `astro check` is too slow for every change; the workflow runs it.
- **Found by search engines and AI assistants** (added 2026-11-16): one source, `site/src/seo.ts`, gives the home page its Questions and its schema.org data (`SoftwareApplication`, `FAQPage`), and `/llms.txt` the same in Markdown. `/robots.txt` welcomes every crawler, AI assistants' included, since being read is how a free tool gets recommended; `/sitemap.xml` lists the pages, written by hand rather than through a package. Link previews use `og.png` (1200 by 630, PNG, as some previews read no WebP). The answers state only what the app does today, from the ADRs, as the Terms and the Privacy Policy do.
- **The waitlist** (added 2026-11-18): the hero's "macOS and Linux later" leads to a form at `/#notify`: a name, an email, a toggle per system (one or both, changed 2026-11-18 from a submit button per system) and one submit button. It is a plain HTML form that works with no JavaScript: each ticked system is sent as `os` (`macos`, `linux`) to `POST /api/waitlist` (`site/worker/index.ts`), which keeps one row per email and system, both in one batch, in the D1 database `kemable-site` (`site/migrations/`), with where the person came from (ADR 0034), and answers with a redirect to `/waitlist/joined` or `/waitlist/failed`, both static and out of search results. A hidden field keeps simple bots out. No IP address is kept; the Privacy Policy says what is kept and why. The counts are read with Wrangler or with Kemable's own D1 driver (`site/README.md`).
- **The address** (decided 2026-11-18): `https://kemable.ducht.info.vn`, the Worker's custom domain (`routes` in `site/wrangler.toml`), on the user's own domain, which is on Cloudflare. It is `site` in `site/astro.config.mjs`, so the canonical links, the sitemap, `/llms.txt` and link previews name it, and the READMEs and the app link to it. It stays: when Kemable gets a domain of its own, the site moves there and this address redirects to it with its path (a Redirect Rule in the `ducht.info.vn` zone), so every installed version keeps opening the right page and a new domain never needs a release (`site/README.md`, The domain).
- **The app opens it**: `open_terms` (`ui/windows/src-tauri/src/commands.rs`) knows `https://kemable.ducht.info.vn/terms` and `/privacy`, and nothing else. The terms dialog stays hidden until the drafts are reviewed; Settings › About still opens GitHub.

## Rationale

- **In this repository**: the pages that describe the app change in the same pull request as the app, reviewed together; one place for issues and history.
- **Static pages**: a landing page and two documents need no server. Static HTML is the fastest page there is, and Pages serves it from Cloudflare's edge at no cost.
- **Astro with React islands**: HTML by default, JavaScript only where asked, and React ready for the parts that will need it (a changelog, a pricing page) without a second framework.
- **Direct Upload from GitHub Actions** over Pages' own Git connection: the build runs with the repository's pinned actions and lockfile, beside the other workflows, and a pull request is checked the same way the deploy is built. The Cloudflare token stays in GitHub's secrets, scoped to Pages.
- **Bundled fonts**: a font service would see every visitor's address, which the Privacy Policy says nothing else does.

## Consequences

- When the app changes what it sends or keeps, `site/src/pages/privacy.astro` changes in the same change, with its date. The terms dialog's `TERMS_VERSION` is raised when the reviewed pages replace the drafts, so everyone is asked once.
- A new domain changes `site` in `site/astro.config.mjs`, `routes` in `site/wrangler.toml` and the READMEs' links in one change, and adds the redirect from `kemable.ducht.info.vn`; `open_terms` stays as it is, and `ducht.info.vn` must be kept. (Until 2026-11-18 the app named `kemable.pages.dev`; no release had shipped with it.)
- If the Pages project cannot be named `kemable`, its address differs: the same three places change (`site/README.md`).
- The site's packages are not in `ui/windows/THIRD-PARTY-NOTICES.md`: nothing from `site/` ships in the app. They keep to the permissive licenses of ADR 0023, checked by hand (`pnpm licenses list --prod`) when one is added.
- Every `node scripts/verify.mjs --ui` takes a few seconds more.
- Still open: the legal review of the Terms and the Privacy Policy; a domain of Kemable's own; SoleBuilt's site linking here.

## Alternatives considered

- **Pages on SoleBuilt's site**: one site to keep, but the app's pages would change outside the app's repository and review.
- **GitHub Pages**: free too, but no headers file for caching and no edge redirects for the move to a domain; Cloudflare was the user's choice.
- **Plain HTML, no framework**: enough for three pages today, but the header and footer copied into each page, and nothing ready for interactive parts.
- **Pages' Git integration**: builds on Cloudflare with its own Node and pnpm, outside the repository's checks, and needs the Cloudflare GitHub app on the organisation.
- **`cloudflare/wrangler-action`**: one more action to pin and trust for what one `npx wrangler@<version>` line does.
