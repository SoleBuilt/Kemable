// One Live page's listening and what it heard (docs/decisions/0020-live-tab.md).
// It belongs to its page, not to the screen: it goes on listening while another
// page is shown, so a NOTIFY or PUBLISH sent from a query page is there on return.

import { asCoreError, ipc } from "../ipc";
import type { Condition } from "../grid/FilterRow.svelte";
import { nextSortKeys, type SortKey } from "../grid/engine";
import type { ColumnMeta, CoreError, LiveEvent, ResultFilter } from "../types";

export type LiveStatus = "idle" | "starting" | "listening" | "stopping";

export class LiveSession {
  /** The names as typed: split at spaces and commas by the core, "quoted" to keep a name as written. */
  channels = $state("");
  patterns = $state("");
  status: LiveStatus = $state("idle");
  /** The database the listening started in. */
  database: string | null = $state(null);
  /** The result set the events are rows of; null until a first listening is taken. */
  result: number | null = $state(null);
  columns: ColumnMeta[] = $state.raw([]);
  /** The events held, and the ones the filter shows. */
  total = $state(0);
  shown = $state(0);
  /** What the driver said beside the events. */
  notices: string[] = $state.raw([]);
  error: CoreError | null = $state.raw(null);
  /** The listening stopped because the set holds as many events as a Live tab keeps. */
  limited = $state(false);
  /** Keep the newest event in view. */
  follow = $state(true);
  filtersOpen = $state(false);
  filter: { conditions: Condition[]; matchAny: boolean } | null = $state.raw(null);
  sort: SortKey[] = $state.raw([]);
  /** Bumped when the core arranged the rows again (a filter, a sort), so the grid reads them again. */
  arranged = $state(0);

  private run: number | null = null;
  private closed = false;

  get busy(): boolean {
    return this.status !== "idle";
  }

  get hasTopics(): boolean {
    return this.channels.trim() !== "" || this.patterns.trim() !== "";
  }

  /** Starts listening; what was heard before stays until the server has taken the topics. */
  async start(conn: number, database: string | null) {
    if (this.busy || !this.hasTopics) return;
    this.status = "starting";
    this.error = null;
    this.limited = false;
    this.notices = [];
    this.database = database;
    try {
      this.run = await ipc.startLive(conn, database, this.channels, this.patterns, (event) => this.onEvent(event));
      // The page closed, or Stop was pressed, before the id came back. (The
      // cast undoes the narrowing from the assignment above: events and Stop
      // change the status while the call waits.)
      if (this.closed || (this.status as LiveStatus) === "stopping") void ipc.cancelRun(this.run);
    } catch (caught) {
      this.error = asCoreError(caught);
      this.status = "idle";
    }
  }

  stop() {
    if (this.status !== "starting" && this.status !== "listening") return;
    this.status = "stopping";
    if (this.run !== null) void ipc.cancelRun(this.run);
  }

  /** Forgets what was heard; a listening goes on into an empty list only by starting again. */
  clear() {
    if (this.busy) return;
    this.release();
    this.error = null;
    this.limited = false;
    this.notices = [];
  }

  /** The page is closing: stop, and let go of the events. */
  close() {
    this.closed = true;
    clearTimeout(this.resortTimer);
    if (this.run !== null) void ipc.cancelRun(this.run);
    this.release();
  }

  private release() {
    if (this.result !== null) void ipc.releaseResult(this.result);
    this.result = null;
    this.columns = [];
    this.total = 0;
    this.shown = 0;
    this.sort = [];
  }

  /**
   * A Tauri channel hands on its next message only once this one returned:
   * a throw here would hold every later event back, and the tab would sit
   * on its count with nothing said. So a failure is logged and the tab goes on.
   */
  private onEvent(event: LiveEvent) {
    try {
      this.take(event);
    } catch (caught) {
      console.error("A Live event could not be shown", event, caught);
    }
  }

  private resortTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * The core puts events that arrive after a sort after the rows it sorted,
   * in arrival order. A Live tab sorts them in again, at most a few times a
   * second, so a sort by `received` descending keeps the newest on top.
   */
  private resortSoon() {
    if (this.sort.length === 0 || this.resortTimer !== undefined) return;
    this.resortTimer = setTimeout(() => {
      this.resortTimer = undefined;
      const result = this.result;
      const keys = this.sort;
      if (result === null || keys.length === 0) return;
      void ipc.sortResult(result, keys).then(
        () => {
          if (this.result === result) this.arranged++;
        },
        () => {},
      );
    }, 300);
  }

  private take(event: LiveEvent) {
    if (this.closed) {
      // The page went while its listening was still ending: nothing shows this set.
      if (event.type === "listening") void ipc.releaseResult(event.result);
      return;
    }
    switch (event.type) {
      case "listening":
        this.release();
        this.result = event.result;
        this.columns = event.columns;
        if (this.status === "starting") this.status = "listening";
        // The filter is the page's, so it goes on over the new events.
        if (this.filter) void this.applyFilter(this.filter.conditions, this.filter.matchAny);
        break;
      case "events":
        if (event.total !== this.total) this.resortSoon();
        this.total = event.total;
        this.shown = event.shown;
        break;
      case "notice":
        this.notices = [...this.notices, event.text];
        break;
      case "finished":
        if (event.total !== this.total) this.resortSoon();
        this.total = event.total;
        this.shown = event.shown;
        this.limited = event.limited;
        this.error = event.error;
        this.status = "idle";
        this.run = null;
        break;
    }
  }

  /** Filters the events held, in the core; the ones that arrive later are tested as they come. */
  async applyFilter(conditions: Condition[], matchAny: boolean): Promise<CoreError | null> {
    const result = this.result;
    const filters: ResultFilter[] = conditions.flatMap((condition) =>
      condition.op === "raw" ? [] : [{ column: condition.column, op: condition.op, value: condition.value, second: condition.second }],
    );
    this.filter = filters.length > 0 ? { conditions, matchAny } : null;
    if (result === null) return null;
    try {
      const counts = await ipc.filterResult(result, filters, matchAny);
      if (this.result !== result) return null;
      this.total = counts.totalRows;
      this.shown = counts.shownRows;
      this.arranged++;
      return null;
    } catch (caught) {
      this.filter = null;
      return asCoreError(caught);
    }
  }

  /** A header click: the core sorts the events held; later ones follow in the order they arrive. */
  async sortBy(column: number, additive: boolean): Promise<CoreError | null> {
    const result = this.result;
    if (result === null) return null;
    const keys = nextSortKeys(this.sort, column, additive);
    try {
      await ipc.sortResult(result, keys);
      if (this.result !== result) return null;
      this.sort = keys;
      this.arranged++;
      return null;
    } catch (caught) {
      return asCoreError(caught);
    }
  }
}
