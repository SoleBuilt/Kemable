# Kemable's website

The home page, the Terms and the Privacy Policy, built with Astro and served by Cloudflare Pages at `https://kemable.pages.dev` until a domain is bought. Why, and how it fits the app: `docs/decisions/0030-website.md`. Working on it: `AGENTS.md`.

```bash
pnpm install
pnpm dev        # http://localhost:4321
pnpm build      # dist/
```

## Deploying

`.github/workflows/site.yml` builds the site on every pull request that touches `site/`, and on a push to `main` deploys `dist/` to the Pages project `kemable` with Wrangler. Until the two secrets below exist, it builds and says it did not deploy.

### Once: the Pages project

1. Cloudflare dashboard › Workers & Pages › Create › Pages › **Upload assets** (Direct Upload, not a Git connection: GitHub Actions does the uploads).
2. Name the project `kemable`, so its address is `https://kemable.pages.dev`. If Cloudflare gives another address (the name is taken), change `site` in `astro.config.mjs`, `--project-name` in the workflow, and `TERMS_OF_SERVICE` and `PRIVACY_POLICY` in `ui/windows/src-tauri/src/commands.rs`, in one change.
3. When the dashboard asks for files, upload `site/dist/` from `pnpm build`. The production branch is `main`.

From a terminal instead: `npx wrangler@4.156.0 pages project create kemable --production-branch=main`.

### Once: the secrets

1. Cloudflare dashboard › My Profile › API Tokens › Create Token › **Custom token**, with one permission: Account › **Cloudflare Pages** › **Edit**, for your account only. Copy the token.
2. Your **Account ID**: Cloudflare dashboard › Workers & Pages, on the right; or the dashboard's URL, `dash.cloudflare.com/<account id>/...`.
3. GitHub › SoleBuilt/Kemable › Settings › Secrets and variables › Actions › New repository secret: `CLOUDFLARE_API_TOKEN` with the token, `CLOUDFLARE_ACCOUNT_ID` with the account ID.
4. Actions › Site › Run workflow on `main`, or merge a change to `site/`. The run's last step says where it deployed.

### Later: the domain

1. Buy the domain, and add it to Cloudflare (its DNS on Cloudflare is simplest).
2. Pages project `kemable` › Custom domains › Set up a custom domain. Cloudflare adds the DNS record and the certificate.
3. In one change: `site` in `astro.config.mjs`, and `TERMS_OF_SERVICE` and `PRIVACY_POLICY` in `ui/windows/src-tauri/src/commands.rs`, to the new domain; and `docs/decisions/0030-website.md`.
4. Keep `kemable.pages.dev` serving: installed apps open it until they update. It stays live beside the custom domain; to send its visitors to the domain, add a Bulk Redirect in Cloudflare from `kemable.pages.dev` to the domain, not a change here.

SoleBuilt's own site links here once the site is live.
