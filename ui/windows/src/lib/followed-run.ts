/**
 * The run a query page follows: whose events it shows, and the id Stop cancels.
 *
 * Each run sends its events on a channel of its own, so the channel tells whose
 * events they are: the page takes them while the run is the latest it began,
 * and drops a run left behind by another connection opening.
 *
 * The id comes back from `run_query`, and may come back after the run ended: a
 * fast statement's events, `finished` among them, can all arrive first. An id
 * that comes back for an ended run is not kept, so Stop never aims at it.
 */
export class FollowedRun {
  private latest = 0;
  private id: number | null = null;
  private ended = true;

  /** Follows a new run in place of any other; `deliver` gets its events while it is the latest. */
  begin<E>(deliver: (event: E) => void): { token: number; events: (event: E) => void } {
    const token = ++this.latest;
    this.id = null;
    this.ended = false;
    return {
      token,
      events: (event) => {
        if (token === this.latest) deliver(event);
      },
    };
  }

  /** The id `run_query` returned for the run `token` began. */
  started(token: number, id: number) {
    if (token === this.latest && !this.ended) this.id = id;
  }

  /** The run followed finished, or never started. */
  end() {
    this.ended = true;
    this.id = null;
  }

  /** Stops following: the run's later events, if any, are dropped. */
  drop() {
    this.latest++;
    this.end();
  }

  /** The id Stop cancels: null when no run is going on, or before its id came back. */
  get cancellable(): number | null {
    return this.id;
  }
}
