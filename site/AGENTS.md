# Kemable's website

Static pages built with Astro, React and Tailwind, served by Cloudflare Pages: the home page, the Terms and the Privacy Policy. The root `AGENTS.md` holds the rules every folder shares; `docs/decisions/0030-website.md` has why this stack and how it is deployed. `README.md` here has the deploy steps.

## Principles

1. **Fast is the feature here too.** Every page is plain HTML and CSS. JavaScript ships only for a part that needs the browser, as a React island loaded `client:visible` or `client:idle`, never `client:load` above the fold. Images are already sized and compressed in `public/`; each `<img>` and `<video>` has its `width` and `height`, and images below the fold load `lazy`.
2. **Nothing leaves for a third party.** Fonts are bundled (`@fontsource-variable/geist`), no analytics, no cookies, no embeds. The Privacy Policy says so; a change that adds any of these changes that page in the same change.
3. **The pages say what the app does.** The Terms and the Privacy Policy describe the app's real behaviour, from the ADRs they name in their front matter. When the app changes what it sends or keeps, the page changes with it, with its date.
4. **The app's look.** Black and white with the icon's ember, Geist, dark by default. The tokens are in `src/styles/global.css` (`--bg`, `--fg`, `--line`, `--ember` and the rest), used through Tailwind's `bg-bg`, `text-fg2`, `border-line` and so on; no colour outside them.

## Map

- `src/pages/`: `index.astro` (the home page), `terms.astro`, `privacy.astro`. A file here is a page at its name: `terms.astro` is `/terms` (`build.format: "file"`, so Pages serves it with no redirect).
- `src/layouts/Base.astro`: the head, the bar at the top and the footer. `src/links.ts`: the addresses more than one page uses.
- `src/components/`: `ThemeToggle.tsx` (the one React island: dark or light, kept in the browser's storage), `DraftNotice.astro` (the note at the top of the Terms and the Privacy Policy until a lawyer has read them).
- `public/`: copied as is: the app's icon (`app-icon.svg`, the favicon and the logo), the screenshots and the demo video, `_headers` (Cloudflare Pages' headers: long caching for `/_astro/*`).
- `astro.config.mjs`: the site's address (`site`), which is the Pages address until a domain is bought.
- `pnpm-workspace.yaml`: packages at least two weeks old (`minimumReleaseAge`), and no `sharp`.

## Commands

From `site/`:

```bash
pnpm install
pnpm dev        # http://localhost:4321 with hot reload
pnpm check      # astro check: types in .astro and .tsx
pnpm build      # the pages into dist/
pnpm preview    # dist/ served as Pages would
```

`node scripts/verify.mjs` installs and builds the site; `.github/workflows/site.yml` also runs `pnpm check`, then deploys from `main`.

## Dependencies

Exact versions, each at least two weeks old when added. Licenses permissive, as for the app (`docs/decisions/0023-licensing.md`): check with `pnpm licenses list --prod`. Nothing from here ships inside the app, so the site's packages are not in `ui/windows/THIRD-PARTY-NOTICES.md`.
