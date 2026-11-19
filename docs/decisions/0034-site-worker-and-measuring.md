# 0034: The website is a Worker with static assets; it counts where visitors come from, and asks before cookies, Google Analytics and Meta Pixel

- **Status**: Accepted
- **Date**: 2026-11-18
- **Amends**: `docs/decisions/0030-website.md` (served by Cloudflare Pages; nothing for a third party, no analytics, no cookies). Decided with the user on 2026-11-18.

## Context

The site was static pages on Cloudflare Pages, with one Pages Function for the macOS and Linux waitlist (ADR 0030). Before its first deploy, two things changed:

- Cloudflare now puts its new work into Workers with static assets, and recommends them for new projects. Pages keeps running, but scheduled jobs (to write to the waitlist once a version ships), rate limits and the Worker's logs come to Workers first. The site had not been deployed and the app not released, so moving cost nothing yet.
- The user wants to know where visitors come from: which links (Facebook, Instagram, X, a newsletter) and whether AI assistants recommend Kemable. ChatGPT adds `utm_source=chatgpt.com` to its links; other assistants and social sites leave only a Referer. Later the user will run ads on Google and Meta, which measure through Google Analytics and the Meta Pixel.

Google Analytics and the Meta Pixel set cookies and send the visit to Google and Meta. Under the GDPR and the ePrivacy rules, a visitor in the EU must agree first; Vietnam's Decree 13/2023 asks for consent to process personal data too. Counting a visit with no identifier, no cookie and no storage on the device needs no consent.

## Decision

**A Worker with static assets** (`site/wrangler.toml`, `site/worker/index.ts`). The built pages in `site/dist/` are served straight from Cloudflare's edge, as before; the Worker runs only for `/api/*` (`run_worker_first`), so a page view runs no code. On a PC, `pnpm worker` runs it with a local D1 and `pnpm dev` forwards `/api/*` to it (`site/README.md`); its redirects are paths, not full addresses, so they work behind that forward. The Worker's name is `kemable`, its one address the custom domain `kemable.ducht.info.vn` (no `workers.dev` address, no preview addresses). `.github/workflows/site.yml` tests the Worker, builds, applies the D1 migrations and runs `wrangler deploy` on a push to `main`, as the Pages deploy did; Cloudflare's own Git builds stay off, so a deploy is always what passed the checks.

**One D1 database, `kemable-site`** (`site/migrations/`): `waitlist` (ADR 0030) and `visits`. Its id is in `wrangler.toml`; it is not a secret. A new table is a new migration file, applied by the workflow before the deploy that needs it.

**Counting, with no consent needed** (`site/src/components/Measure.astro`, `site/worker/source.ts`):

- One beacon (`POST /api/visit`) when a visitor arrives from outside or opens the site directly, not for moves inside the site, reloads or Back; and one when they click a Download link. Each carries the page's path, the link's UTM tags (`utm_source`, `utm_medium`, `utm_campaign`), an ad's click id (`gclid`, `fbclid`, `msclkid`) and the referring site's origin.
- The Worker names the source in one place, `sourceOf`: a UTM tag first, then a click id, then the referring host, else `direct`; with link shims and aliases folded (`l.facebook.com` is `facebook.com`, `t.co` is `x.com`, every Google country site is `google.com`), and a medium when the link gave none: `ai` (ChatGPT, Perplexity, Gemini, Copilot, Claude...), `search`, `social`, `referral`. ChatGPT counts as `chatgpt.com` whether it sent a tag or only a Referer.
- A `visits` row keeps the time, the kind, the path, the source, medium and campaign, the referring host (never its path), Cloudflare's two-letter country, and the `signal` that named the source: `utm`, `click_id`, `referrer` or `none` (added 2026-11-18, `site/migrations/0003_signal.sql`). Folding names keeps one place on one row, and the signal and the referring host keep what told it: ChatGPT by its tag or by its Referer, or a tagged link shared somewhere else. No IP address, no browser details, no identifier: a row cannot be tied to a person.
- The waitlist's form carries the same fields as hidden inputs, so each entry says where its person came from.

**Cookies, Google Analytics and the Meta Pixel, only with consent**:

- A notice at the bottom of every page until the visitor chooses: **Allow** or **No thanks**, the same size, with a link to the Privacy Policy. Footer › Cookies opens it again. The choice is kept in `kemable_consent` for a year (needed to remember it).
- With **Allow**: `kemable_src` keeps the first link the visitor came by, for 90 days, so a Download or a waitlist entry days later is credited to it; Google Analytics 4 (`gtag.js`) and the Meta Pixel (`fbevents.js`) are loaded when their ids are set in `site/src/analytics.ts`. A Download click is `file_download` and a custom `Download`; `/waitlist/joined` is `generate_lead` and `Lead`.
- With **No thanks**, or before a choice: nothing is set, nothing of Google's or Meta's is loaded. Taking an Allow back deletes the cookies the site and those tools set, and reloads the page.
- An empty id leaves its tool out, and the notice names only the tools that are set. The ids are empty until the user creates the property and the pixel.

## Rationale

- **Worker over Pages**: where Cloudflare adds features; the same edge serving for static files and the same free tier; one address of our own instead of a `pages.dev` one. A Worker's custom domain needs its zone on Cloudflare, which `ducht.info.vn` is.
- **CI deploys over Cloudflare's Git builds**: the dashboard is the same either way; CI runs the tests and `astro check` first, deploys only `main`, and keeps the token in GitHub's secrets.
- **A beacon of our own over Cloudflare Web Analytics**: Web Analytics does not read UTM tags, and the question is precisely which links bring people.
- **Arrivals, not page views**: a move inside the site says nothing about where people come from, and would fill the table with `direct`.
- **Consent before the ad tools**: required in the EU and in Vietnam, and Google's and Meta's own terms ask for it. Without consent the counts still answer the main question.
- **No package for Cloudflare's or the tools' types**: the Worker declares the few it uses, and the snippets are Google's and Meta's own, written in TypeScript.

## Consequences

- The site sets cookies and loads third-party scripts for visitors who allow them; `site/AGENTS.md`'s principles and the Privacy Policy say so. A new tool, cookie or field changes `privacy.astro` in the same change.
- Every page loads one small deferred script (`Measure.astro`); the pages stay static and paint before it runs.
- The token in `CLOUDFLARE_API_TOKEN` needs Workers and D1 rights and the zone's Workers Routes (`site/README.md`).
- Visitors who block scripts or beacons are not counted; the counts are a floor.
- `visits` grows by a row per arrival. At D1's free limits that is years of traffic; old rows can be summed and deleted when needed.
- The manual checks are in `docs/testing/preview.md`, C17.

## Alternatives considered

- **Stay on Pages**: works today, with a CNAME on any DNS, but the features the site will need come to Workers first, and the app would carry a `pages.dev` address.
- **Cloudflare Web Analytics or Zaraz**: no UTM tags in Web Analytics; Zaraz could load Google's and Meta's tools from the edge, but adds a second place to configure them and its own consent manager.
- **Google Analytics alone, for everything**: needs consent, so it would not count anyone who says no, and the source would live only in Google's account.
- **A cookie for everyone, without asking**: against the GDPR, Decree 13/2023, and Google's and Meta's terms.
