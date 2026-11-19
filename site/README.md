# Kemable's website

The home page, the Terms and the Privacy Policy, built with Astro and served by a Cloudflare Worker with static assets at `https://kemable.ducht.info.vn`, the address the app opens too. Why, and how it fits the app: `docs/decisions/0030-website.md` and `0034-site-worker-and-measuring.md`. Working on it: `AGENTS.md`.

```bash
pnpm install
pnpm dev        # http://localhost:4321, the pages with hot reload; /api/* goes to `pnpm worker`
pnpm worker     # the Worker on http://127.0.0.1:8787, with the built pages and a local D1
pnpm test       # where a visit came from (worker/source.ts)
pnpm build      # dist/
```

## On this PC: the pages with the Worker

`pnpm dev` serves the pages but cannot run the Worker (`/api/*`: the waitlist and the visit counts); it forwards those requests to `http://127.0.0.1:8787`. So, in two terminals in `site/`:

1. `pnpm worker`: builds the pages, makes the local database's tables in `.wrangler/` (`scripts/worker.mjs`), and runs the Worker. Run it again after a change to `worker/` or `migrations/`, and after a `pnpm build` of your own: a build empties `dist/` under the running Worker, which then answers 404.
2. `pnpm dev`: open `http://localhost:4321`. The form and the counts work; a page change shows at once.

Without `pnpm worker`, a form sent from `pnpm dev` ends on a 404. `http://127.0.0.1:8787` alone is the site as it is deployed, from the last build.

To read the local tables: `npx wrangler@4.156.0 d1 execute kemable-site --local --command "SELECT * FROM waitlist"`.

## Deploying

`.github/workflows/site.yml` checks, tests and builds the site on every pull request that touches `site/`. On a push to `main` it also applies the D1 migrations and runs `wrangler deploy`, which uploads `dist/` and `worker/` as the Worker `kemable` and binds it to its domain (`wrangler.toml`). Until the two secrets below exist, it builds and says it did not deploy.

### Done once: the database

The D1 database `kemable-site` exists (created 2026-11-18, its id in `wrangler.toml`) with its tables, migrations 0001 to 0003 applied. A new file in `migrations/` is applied by the workflow before the deploy that needs it; by hand: `npx wrangler@4.156.0 d1 migrations apply kemable-site --remote`.

### Once: the secrets

1. Cloudflare dashboard › My Profile › API Tokens › Create Token › **Custom token**, with these permissions:
   - Account › **Workers Scripts** › Edit
   - Account › **D1** › Edit
   - Account › **Account Settings** › Read
   - Zone › **Workers Routes** › Edit, and Zone › **DNS** › Edit (the Worker's custom domain), with Zone Resources › Include › Specific zone › `ducht.info.vn`
   - User › **Memberships** › Read, and User › **User Details** › Read (Wrangler reads them)

   Account Resources: your account only. Copy the token.
2. Your **Account ID**: Cloudflare dashboard › Workers & Pages, on the right; or the dashboard's URL, `dash.cloudflare.com/<account id>/...`.
3. GitHub › SoleBuilt/Kemable › Settings › Secrets and variables › Actions › New repository secret: `CLOUDFLARE_API_TOKEN` with the token, `CLOUDFLARE_ACCOUNT_ID` with the account ID.
4. Actions › Site › Run workflow on `main`, or merge a change to `site/`. The deploy step prints the Worker's address.

The first deploy adds `kemable.ducht.info.vn` to the Worker and its DNS record; the certificate takes a few minutes. A DNS record named `kemable` already in the zone makes it fail: delete that record first.

### The domain

`https://kemable.ducht.info.vn` is `site` in `astro.config.mjs`, `routes` in `wrangler.toml`, the links in `README.md` and `README.vi.md`, and `TERMS_OF_SERVICE` and `PRIVACY_POLICY` in `ui/windows/src-tauri/src/commands.rs`. The app's two stay as they are for good (`docs/decisions/0030-website.md`, The address).

Moving to a domain of Kemable's own later:

1. Add the new domain's zone to Cloudflare.
2. In one change: `site` in `astro.config.mjs`, `routes` in `wrangler.toml` (the new domain, and keep `kemable.ducht.info.vn` out of it), the links in the two READMEs. Deploy.
3. Zone `ducht.info.vn` › Rules › **Redirect Rules** › Create: when the hostname equals `kemable.ducht.info.vn`, a dynamic redirect to `concat("https://<new domain>", http.request.uri.path)` with status 301 and **Preserve query string**. Keep a proxied DNS record for `kemable` (an `AAAA` to `100::` is enough) so the rule sees the requests.
4. Check: `https://kemable.ducht.info.vn/privacy?utm_source=test` lands on the new domain's `/privacy?utm_source=test`. The app needs no release.

## Where visitors come from

`docs/decisions/0034-site-worker-and-measuring.md` has what is counted and why. The tables, read from `site/` (or open `kemable-site` in Kemable itself, Cloudflare D1):

```bash
# Arrivals and downloads by source, the last 30 days
npx wrangler@4.156.0 d1 execute kemable-site --remote --command "SELECT source, medium, SUM(kind = 'view') AS arrivals, SUM(kind = 'download') AS downloads FROM visits WHERE at >= datetime('now', '-30 days') GROUP BY source, medium ORDER BY arrivals DESC"

# How many AI assistants sent, and what told it: their own utm tag or only the Referer
npx wrangler@4.156.0 d1 execute kemable-site --remote --command "SELECT source, signal, COUNT(*) AS arrivals FROM visits WHERE medium = 'ai' GROUP BY source, signal ORDER BY arrivals DESC"

# Tagged links opened somewhere else than the tag says (a ChatGPT link shared on Facebook)
npx wrangler@4.156.0 d1 execute kemable-site --remote --command "SELECT source, referrer, COUNT(*) AS arrivals FROM visits WHERE signal = 'utm' AND referrer IS NOT NULL AND referrer <> source GROUP BY source, referrer"

# The waitlist: how many want each system, and from where
npx wrangler@4.156.0 d1 execute kemable-site --remote --command "SELECT os, source, COUNT(*) AS people FROM waitlist GROUP BY os, source ORDER BY os, people DESC"
```

Tag every link you post yourself, so it is counted under its place and campaign even when the app it is opened in hides the Referer: `https://kemable.ducht.info.vn/?utm_source=facebook&utm_medium=social&utm_campaign=launch`.

### Google Analytics and the Meta Pixel

Both load only for a visitor who clicks **Allow** in the cookie notice. Each is off until its id is set in `src/analytics.ts`:

1. **Google Analytics 4**: analytics.google.com › Admin › Create › Property, then a **Web** data stream for `https://kemable.ducht.info.vn`. Copy its **Measurement ID** (`G-...`) into `GOOGLE_ANALYTICS_ID`.
2. **Meta Pixel**: Meta Events Manager › Connect data sources › Web › Meta Pixel, named Kemable. Copy its ID (digits) into `META_PIXEL_ID`. Skip the partner and code set-up: the site already has the code.
3. Merge to `main`. The notice then names the tool, and the Privacy Policy already describes both.

What they get: a page view, `file_download` (Google) and a custom `Download` (Meta) on a Download click, and `generate_lead` (Google) and `Lead` (Meta) when someone joins the waitlist.

SoleBuilt's own site links here once the site is live.
