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
- **The pages**: `/` (the approved design, ported), `/terms` and `/privacy`. The Terms and the Privacy Policy describe what the app does, from the ADRs that own each fact (0019 the data folder, 0028 the update check, 0015 and 0024 Credential Manager, 0023 the licenses), and say at the top that they are drafts for legal review. `build.format: "file"`, so `/terms` is served as is, with no redirect to `/terms/`.
- **Nothing for a third party.** The fonts are bundled from `@fontsource-variable/geist`, not loaded from a font service; no analytics, no cookies. The theme choice is kept in the browser's storage.
- **Exact versions, two weeks old**: each dependency is pinned without a range, at a version published at least two weeks before it is added, and `minimumReleaseAge` in `site/pnpm-workspace.yaml` holds every package, transitive ones too, to the same. `sharp` is not installed (`ignoredOptionalDependencies`): images are already sized and compressed in `site/public/`.
- **Served by Cloudflare Pages**, project `kemable`, at `https://kemable.pages.dev` until a domain is bought. `.github/workflows/site.yml` checks the types and builds on every pull request that touches `site/`, and on a push to `main` deploys `dist/` with Wrangler (Direct Upload) when the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets exist; without them it builds and says it did not deploy.
- **Checked on every change**: `node scripts/verify.mjs` installs the site with its frozen lockfile and builds it (with the UI steps). Both are fast and offline once the packages are in pnpm's store. `astro check` is too slow for every change; the workflow runs it.
- **The app opens it**: `open_terms` (`ui/windows/src-tauri/src/commands.rs`) knows `https://kemable.pages.dev/terms` and `/privacy`, and nothing else. The terms dialog stays hidden until the drafts are reviewed; Settings › About still opens GitHub.

## Rationale

- **In this repository**: the pages that describe the app change in the same pull request as the app, reviewed together; one place for issues and history.
- **Static pages**: a landing page and two documents need no server. Static HTML is the fastest page there is, and Pages serves it from Cloudflare's edge at no cost.
- **Astro with React islands**: HTML by default, JavaScript only where asked, and React ready for the parts that will need it (a changelog, a pricing page) without a second framework.
- **Direct Upload from GitHub Actions** over Pages' own Git connection: the build runs with the repository's pinned actions and lockfile, beside the other workflows, and a pull request is checked the same way the deploy is built. The Cloudflare token stays in GitHub's secrets, scoped to Pages.
- **Bundled fonts**: a font service would see every visitor's address, which the Privacy Policy says nothing else does.

## Consequences

- When the app changes what it sends or keeps, `site/src/pages/privacy.astro` changes in the same change, with its date. The terms dialog's `TERMS_VERSION` is raised when the reviewed pages replace the drafts, so everyone is asked once.
- A domain changes `site` in `site/astro.config.mjs` and the two addresses in `open_terms`, in one change. `kemable.pages.dev` keeps serving, since installed apps open it until they update (`site/README.md`, Later: the domain).
- If the Pages project cannot be named `kemable`, its address differs: the same three places change (`site/README.md`).
- The site's packages are not in `ui/windows/THIRD-PARTY-NOTICES.md`: nothing from `site/` ships in the app. They keep to the permissive licenses of ADR 0023, checked by hand (`pnpm licenses list --prod`) when one is added.
- Every `node scripts/verify.mjs --ui` takes a few seconds more.
- Still open: the legal review of the Terms and the Privacy Policy; the domain; SoleBuilt's site linking here.

## Alternatives considered

- **Pages on SoleBuilt's site**: one site to keep, but the app's pages would change outside the app's repository and review.
- **GitHub Pages**: free too, but no headers file for caching and no edge redirects for the move to a domain; Cloudflare was the user's choice.
- **Plain HTML, no framework**: enough for three pages today, but the header and footer copied into each page, and nothing ready for interactive parts.
- **Pages' Git integration**: builds on Cloudflare with its own Node and pnpm, outside the repository's checks, and needs the Cloudflare GitHub app on the organisation.
- **`cloudflare/wrangler-action`**: one more action to pin and trust for what one `npx wrangler@<version>` line does.
