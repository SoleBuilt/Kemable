// `pnpm test` (node --test): the names visits and the waitlist are counted under.
import assert from "node:assert/strict";
import { test } from "node:test";
import { place, sourceOf } from "./source.ts";

const own = "kemable.ducht.info.vn";

test("a UTM tag wins over the referrer, and says it was the tag", () => {
  assert.deepEqual(sourceOf({ utm_source: "chatgpt.com", referrer: "https://www.google.com/" }, own), {
    source: "chatgpt.com",
    medium: "ai",
    campaign: null,
    referrer: "google.com",
    signal: "utm",
  });
});

test("the link's own medium and campaign are kept, lowercased", () => {
  const result = sourceOf({ utm_source: "Facebook", utm_medium: "CPC", utm_campaign: "Launch-1" }, own);
  assert.deepEqual(result, { source: "facebook", medium: "cpc", campaign: "launch-1", referrer: null, signal: "utm" });
});

test("ChatGPT's referrer and its UTM tag count as one place, told apart by the signal", () => {
  const byReferrer = sourceOf({ referrer: "https://chatgpt.com/" }, own);
  const byTag = sourceOf({ utm_source: "chatgpt.com" }, own);
  assert.equal(byReferrer.source, byTag.source);
  assert.equal(byReferrer.medium, "ai");
  assert.equal(byReferrer.signal, "referrer");
  assert.equal(byTag.signal, "utm");
  assert.equal(sourceOf({ referrer: "https://chat.openai.com/c/123" }, own).source, "chatgpt.com");
});

test("a tagged link shared somewhere else keeps the tag as source and the real referrer beside it", () => {
  const result = sourceOf({ utm_source: "chatgpt.com", referrer: "https://l.facebook.com/" }, own);
  assert.equal(result.source, "chatgpt.com");
  assert.equal(result.referrer, "facebook.com");
  assert.equal(result.signal, "utm");
});

test("link shims and country sites name the real place", () => {
  assert.equal(place("l.facebook.com"), "facebook.com");
  assert.equal(place("lm.facebook.com"), "facebook.com");
  assert.equal(place("l.instagram.com"), "instagram.com");
  assert.equal(place("t.co"), "x.com");
  assert.equal(place("www.google.com.vn"), "google.com");
  assert.equal(sourceOf({ referrer: "https://www.google.com.vn/" }, own).medium, "search");
});

test("ad click ids name the network when there are no UTM tags", () => {
  assert.deepEqual(sourceOf({ gclid: "abc" }, own), {
    source: "google.com",
    medium: "cpc",
    campaign: null,
    referrer: null,
    signal: "click_id",
  });
  assert.equal(sourceOf({ fbclid: "x", referrer: "https://l.instagram.com/" }, own).source, "instagram.com");
  assert.equal(sourceOf({ fbclid: "x" }, own).signal, "click_id");
});

test("a move inside the site, or nothing at all, is direct with no signal", () => {
  assert.equal(sourceOf({ referrer: "https://kemable.ducht.info.vn/terms" }, own).source, "direct");
  assert.deepEqual(sourceOf({}, own), { source: "direct", medium: null, campaign: null, referrer: null, signal: "none" });
  assert.equal(sourceOf({ referrer: "not a url" }, own).signal, "none");
});

test("a referrer keeps only its host", () => {
  const result = sourceOf({ referrer: "https://news.ycombinator.com/item?id=1" }, own);
  assert.equal(result.referrer, "news.ycombinator.com");
  assert.equal(result.medium, "social");
});

test("long values are cut, so a crafted link cannot fill the table", () => {
  const result = sourceOf({ utm_source: "a".repeat(500), utm_campaign: "b".repeat(500) }, own);
  assert.equal(result.source.length, 100);
  assert.equal(result.campaign?.length, 100);
});
