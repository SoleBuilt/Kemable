// The ad and analytics tools the website loads, only for a visitor who allows cookies
// (docs/decisions/0034-site-worker-and-measuring.md). An empty id leaves that tool out entirely:
// nothing is loaded and the cookie notice does not name it.

/** Google Analytics 4: Admin › Data streams › the web stream › Measurement ID, such as "G-ABC123DEF4". */
export const GOOGLE_ANALYTICS_ID = "";

/** Meta Pixel: Events Manager › Data sources › the pixel's ID, digits only. */
export const META_PIXEL_ID = "";

/** The first-party cookie that keeps the link a visitor first came by, for 90 days. */
export const FIRST_TOUCH_DAYS = 90;

/** What the cookie notice says the visitor allows, built from the tools that are set. */
export function cookieUses(): string {
  const tools = [GOOGLE_ANALYTICS_ID !== "" && "Google Analytics", META_PIXEL_ID !== "" && "Meta Pixel"].filter(
    (tool): tool is string => tool !== false,
  );
  const remember = "remember which link first brought you here";
  return tools.length === 0 ? remember : `${remember}, and load ${tools.join(" and ")} to measure our ads`;
}
