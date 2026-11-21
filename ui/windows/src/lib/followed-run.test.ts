// `pnpm test` (node --test): which run's events a query page takes, and what Stop cancels.
import assert from "node:assert/strict";
import { test } from "node:test";
import { FollowedRun } from "./followed-run.ts";

type Event = { run: number; type: "statementStarted" | "setStarted" | "finished" };

/** A page as App.svelte keeps one: running until its run's `finished` arrives. */
function page() {
  const followed = new FollowedRun();
  const state = { running: false, shown: [] as Event[] };
  const take = (event: Event) => {
    state.shown.push(event);
    if (event.type === "finished") {
      state.running = false;
      followed.end();
    }
  };
  const begin = () => {
    state.running = true;
    return followed.begin<Event>(take);
  };
  return { followed, state, begin };
}

const runOf = (run: number): Event[] => [
  { run, type: "statementStarted" },
  { run, type: "setStarted" },
  { run, type: "finished" },
];

test("a fast run whose events all come before its id leaves nothing for Stop, and the next run is shown", () => {
  const { followed, state, begin } = page();

  // SQLite in memory: every event, `finished` too, reaches the page before run_query returns.
  const first = begin();
  for (const event of runOf(2)) first.events(event);
  followed.started(first.token, 2);
  assert.equal(state.running, false);
  assert.equal(followed.cancellable, null, "the ended run's id is not kept for Stop");

  const second = begin();
  for (const event of runOf(4)) second.events(event);
  followed.started(second.token, 4);
  assert.equal(state.running, false, "the second run's `finished` reaches the page");
  assert.deepEqual(
    state.shown.filter((event) => event.run === 4).map((event) => event.type),
    ["statementStarted", "setStarted", "finished"],
  );
});

test("a run whose id comes first can be stopped until it finishes", () => {
  const { followed, state, begin } = page();
  const run = begin();
  followed.started(run.token, 7);
  run.events({ run: 7, type: "statementStarted" });
  assert.equal(followed.cancellable, 7);
  run.events({ run: 7, type: "finished" });
  assert.equal(state.running, false);
  assert.equal(followed.cancellable, null);
});

test("a run left behind by another connection opening reaches the page no more", () => {
  const { followed, state, begin } = page();
  const old = begin();
  followed.started(old.token, 3);
  followed.drop();
  assert.equal(followed.cancellable, null);
  old.events({ run: 3, type: "setStarted" });
  old.events({ run: 3, type: "finished" });
  assert.deepEqual(state.shown, []);

  // Nor does a run's late id or event once a newer run began.
  const newer = begin();
  followed.started(old.token, 3);
  assert.equal(followed.cancellable, null);
  old.events({ run: 3, type: "finished" });
  assert.equal(state.running, true);
  followed.started(newer.token, 5);
  assert.equal(followed.cancellable, 5);
});
