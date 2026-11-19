// Where a visitor came from, from what the page saw: the link's UTM tags, an ad's click id, or
// the referring site (docs/decisions/0034-site-worker-and-measuring.md). The visits, the waitlist
// and the reports all use these names, so "chatgpt.com" from a UTM tag and from a Referer count together.

/** What the browser sends: the landing page's query parameters and `document.referrer`. */
export interface Landing {
  utm_source?: string;
  utm_medium?: string;
  utm_campaign?: string;
  gclid?: string;
  fbclid?: string;
  msclkid?: string;
  referrer?: string;
}

/** What named the source: the link's tag, an ad's click id, the referring site, or nothing (direct). */
export type Signal = "utm" | "click_id" | "referrer" | "none";

export interface Source {
  source: string;
  medium: string | null;
  campaign: string | null;
  /** The referring site's host, never its path. */
  referrer: string | null;
  signal: Signal;
}

const MAX = 100;

// Hosts that are one place under several names.
const ALIASES: Record<string, string> = {
  "t.co": "x.com",
  "twitter.com": "x.com",
  "chat.openai.com": "chatgpt.com",
  "lnkd.in": "linkedin.com",
  "youtu.be": "youtube.com",
};

const AI = new Set(["chatgpt.com", "perplexity.ai", "gemini.google.com", "copilot.microsoft.com", "claude.ai", "chat.deepseek.com", "grok.com", "you.com", "phind.com", "meta.ai"]);
const SEARCH = new Set(["google.com", "bing.com", "duckduckgo.com", "search.yahoo.com", "yandex.com", "baidu.com", "ecosia.org", "search.brave.com", "coccoc.com"]);
const SOCIAL = new Set(["facebook.com", "instagram.com", "x.com", "linkedin.com", "reddit.com", "youtube.com", "tiktok.com", "threads.net", "zalo.me", "news.ycombinator.com", "github.com", "discord.com", "t.me"]);

function clean(value: string | undefined): string | null {
  const text = value?.trim().toLowerCase().slice(0, MAX) ?? "";
  return text === "" ? null : text;
}

/** A host as people say it: no `www.` or `m.`, a link shim's real site, one name per place. */
export function place(host: string): string {
  let name = host.toLowerCase().replace(/\.$/, "");
  // Facebook's and Instagram's link shims (l.facebook.com, lm.facebook.com) and mobile hosts.
  name = name.replace(/^(www|m|l|lm|mobile|out)\./, "");
  // Every Google country site is Google: google.com.vn, google.co.uk.
  if (/^google\.[a-z.]+$/.test(name)) return "google.com";
  if (name.startsWith("bing.com")) return "bing.com";
  return ALIASES[name] ?? name;
}

/** What kind of place a source is, when the link did not say. */
export function kind(source: string): string {
  if (AI.has(source)) return "ai";
  if (SEARCH.has(source)) return "search";
  if (SOCIAL.has(source)) return "social";
  return "referral";
}

/** The source of a visit; `ownHost` is the site's own, whose links are moves inside it. */
export function sourceOf(landing: Landing, ownHost: string): Source {
  let referrer: string | null = null;
  try {
    const host = landing.referrer ? new URL(landing.referrer).hostname : "";
    if (host !== "" && place(host) !== place(ownHost)) referrer = place(host).slice(0, MAX);
  } catch {
    // Not a URL: kept as no referrer.
  }
  const campaign = clean(landing.utm_campaign);

  const tagged = clean(landing.utm_source);
  if (tagged !== null) {
    const source = place(tagged);
    return { source, medium: clean(landing.utm_medium) ?? kind(source), campaign, referrer, signal: "utm" };
  }
  // An ad's click id, when the link carries no UTM tags.
  if (clean(landing.gclid) !== null) return { source: "google.com", medium: "cpc", campaign, referrer, signal: "click_id" };
  if (clean(landing.msclkid) !== null) return { source: "bing.com", medium: "cpc", campaign, referrer, signal: "click_id" };
  // fbclid is on every link out of Facebook and Instagram, paid or not.
  if (clean(landing.fbclid) !== null) {
    const source = referrer === "instagram.com" ? "instagram.com" : "facebook.com";
    return { source, medium: "social", campaign, referrer, signal: "click_id" };
  }
  if (referrer !== null) return { source: referrer, medium: kind(referrer), campaign, referrer, signal: "referrer" };
  return { source: "direct", medium: null, campaign, referrer: null, signal: "none" };
}
