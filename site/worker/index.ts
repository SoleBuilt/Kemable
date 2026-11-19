// Kemable's website as a Worker (docs/decisions/0034-site-worker-and-measuring.md). The pages are
// static assets served before this runs; it answers only /api/* (`run_worker_first` in wrangler.toml):
//   POST /api/waitlist  the home page's form: a plain HTML form, answered with a redirect.
//   POST /api/visit     a page view or a Download click, with where the visitor came from.
import { type Landing, sourceOf } from "./source.ts";

// The few parts of Cloudflare's types this file uses, so the site needs no package for them.
interface D1PreparedStatement {
  bind(...values: (string | null)[]): D1PreparedStatement;
  run(): Promise<unknown>;
}
interface D1Database {
  prepare(query: string): D1PreparedStatement;
  batch(statements: D1PreparedStatement[]): Promise<unknown>;
}
interface Env {
  DB: D1Database;
  ASSETS: { fetch(request: Request): Promise<Response> };
}
interface ExecutionContext {
  waitUntil(promise: Promise<unknown>): void;
}

const SYSTEMS = new Set(["macos", "linux"]);
const NAME_MAX = 100;
const EMAIL_MAX = 254;
const PATH_MAX = 200;
// What a browser's type="email" accepts, roughly: something@something.something.
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
// A beacon is a few hundred bytes; anything larger is not one.
const BEACON_MAX = 4096;
const LANDING_KEYS = ["utm_source", "utm_medium", "utm_campaign", "gclid", "fbclid", "msclkid", "referrer"] as const;

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** The landing's fields from a form or a beacon's JSON, strings only. */
function landingFrom(get: (key: string) => unknown): Landing {
  const landing: Landing = {};
  for (const key of LANDING_KEYS) {
    const value = text(get(key));
    if (value !== "") landing[key] = value;
  }
  return landing;
}

/** Cloudflare's two-letter country of the request, without keeping the address it came from. */
function country(request: Request): string | null {
  const cf = (request as Request & { cf?: { country?: unknown } }).cf;
  return typeof cf?.country === "string" ? cf.country.slice(0, 2) : null;
}

async function waitlist(request: Request, env: Env): Promise<Response> {
  // A path, not a full address: behind `pnpm dev`'s proxy the request's own URL names the Worker's
  // port, and the browser must stay on the page's.
  const to = (path: string) => new Response(null, { status: 303, headers: { Location: path } });

  let form: FormData;
  try {
    form = await request.formData();
  } catch {
    return to("/waitlist/failed");
  }
  // A field people never see; a bot that fills every field is told it worked and kept out.
  if (text(form.get("website")) !== "") return to("/waitlist/joined");

  const name = text(form.get("name"));
  const email = text(form.get("email")).toLowerCase();
  // One checkbox per system; a person may tick both, and gets a row for each.
  const systems = [...new Set(form.getAll("os").map(text))].filter((os) => SYSTEMS.has(os));
  const valid = name !== "" && name.length <= NAME_MAX && email.length <= EMAIL_MAX && EMAIL.test(email) && systems.length > 0;
  if (!valid) return to("/waitlist/failed");

  const from = sourceOf(landingFrom((key) => form.get(key)), new URL(request.url).hostname);
  const joinedAt = new Date().toISOString();
  try {
    // Joining again for the same system keeps the first date and source, and takes the newer name.
    // One batch, so both systems are kept or neither.
    await env.DB.batch(
      systems.map((os) =>
        env.DB.prepare(
          "INSERT INTO waitlist (email, os, name, joined_at, source, medium, campaign, signal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT (email, os) DO UPDATE SET name = excluded.name",
        ).bind(email, os, name, joinedAt, from.source, from.medium, from.campaign, from.signal),
      ),
    );
  } catch {
    return to("/waitlist/failed");
  }
  return to("/waitlist/joined");
}

async function visit(request: Request, env: Env, context: ExecutionContext): Promise<Response> {
  // sendBeacon sends text/plain, so the body is read as text and parsed here.
  const body = await request.text();
  if (body.length > BEACON_MAX) return new Response(null, { status: 413 });
  let data: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(body);
    if (typeof parsed !== "object" || parsed === null) return new Response(null, { status: 400 });
    data = parsed as Record<string, unknown>;
  } catch {
    return new Response(null, { status: 400 });
  }
  const kind = text(data.kind);
  const path = text(data.path);
  if ((kind !== "view" && kind !== "download") || !path.startsWith("/")) return new Response(null, { status: 400 });

  const from = sourceOf(landingFrom((key) => data[key]), new URL(request.url).hostname);
  // The browser does not wait for the row: the answer goes at once, the write after it.
  context.waitUntil(
    env.DB.prepare(
      "INSERT INTO visits (at, kind, path, source, medium, campaign, referrer, country, signal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
      .bind(new Date().toISOString(), kind, path.slice(0, PATH_MAX), from.source, from.medium, from.campaign, from.referrer, country(request), from.signal)
      .run()
      .catch(() => undefined),
  );
  return new Response(null, { status: 204 });
}

export default {
  async fetch(request: Request, env: Env, context: ExecutionContext): Promise<Response> {
    const { pathname } = new URL(request.url);
    if (request.method === "POST" && pathname === "/api/waitlist") return waitlist(request, env);
    if (request.method === "POST" && pathname === "/api/visit") return visit(request, env, context);
    if (pathname.startsWith("/api/")) return new Response("Not found", { status: 404 });
    return env.ASSETS.fetch(request);
  },
};
