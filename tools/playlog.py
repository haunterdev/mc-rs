"""Summarize a play day from the server's latency.log.

    python tools/playlog.py [--day YYYY-MM-DD] [--log PATH]

Prints, per player: sessions, minutes played, leave reasons, browser and picture, and the
median of the 5 s samples for input to frame, ping, decode, host pipeline, skipped frames
and backlog drops; per viewer: fps and worst frame times; and the server tick stats
(p99, max, dropped ticks, dig mismatches, unhandled frames) while anyone was on.
"""
import argparse
import collections
import datetime as dt
import json
import os
import statistics


def med(xs):
    xs = [x for x in xs if x is not None]
    return round(statistics.median(xs), 1) if xs else None


def pct(xs, q):
    xs = sorted(x for x in xs if x is not None)
    return round(xs[min(len(xs) - 1, int(len(xs) * q))], 1) if xs else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--day", help="local date, default today")
    ap.add_argument("--log", default=os.path.join(os.environ.get("APPDATA", ""), "mc-rs", "server", "latency.log"))
    a = ap.parse_args()
    day = a.day or dt.date.today().isoformat()

    joins = collections.defaultdict(list)
    leaves = collections.defaultdict(list)
    browser = collections.defaultdict(list)
    viewer = collections.defaultdict(list)
    ticks = []
    for line in open(a.log, encoding="utf-8", errors="replace"):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        t = dt.datetime.fromtimestamp(d.get("time", 0))
        if t.date().isoformat() != day:
            continue
        d["_t"] = t
        src, ev, p = d.get("source"), d.get("event"), d.get("player", "")
        if src == "server" and ev == "join":
            joins[p].append(d)
        elif src == "server" and ev == "leave":
            leaves[p].append(d)
        elif src == "browser":
            browser[p].append(d)
        elif src == "viewer":
            viewer[p].append(d)
        elif src == "server" and ev == "tick" and d.get("players", 0) > 0:
            ticks.append(d)

    players = sorted(set(joins) | set(browser) | set(viewer))
    print(f"{day}: {len(players)} players, {sum(len(v) for v in joins.values())} joins")
    for p in players:
        js, ls, bs, vs = joins[p], leaves[p], browser[p], viewer[p]
        mins = 0.0
        for j in js:
            after = [l["_t"] for l in ls if l["_t"] >= j["_t"]]
            if after:
                mins += (min(after) - j["_t"]).total_seconds() / 60
        reasons = collections.Counter(l.get("reason") for l in ls)
        first = js[0]["_t"].strftime("%H:%M") if js else "?"
        print(f"\n{p}: {len(js)} sessions from {first}, {mins:.0f} min, leaves {dict(reasons)}")
        if bs:
            uas = collections.Counter((b.get("ua") or "")[:60] for b in bs)
            pics = collections.Counter(b.get("picture") for b in bs)
            print(f"  browser {uas.most_common(1)[0][0]!r}  picture {pics.most_common(1)[0][0]}  codec {bs[-1].get('codec')}")
            f = lambda k, s="p50": [b.get(k, {}).get(s) for b in bs if isinstance(b.get(k), dict)]
            print(f"  input_to_frame p50 {med(f('input_to_frame_ms'))} p95 {med(f('input_to_frame_ms', 'p95'))}"
                  f"  input_to_shown p50 {med(f('input_to_shown_ms'))}  ping {med([b.get('ping_ms') for b in bs])}"
                  f" (p95 sample {pct([b.get('ping_ms') for b in bs], 0.95)})")
            print(f"  decode p50 {med(f('decode_ms'))}  host_pipeline p50 {med(f('host_pipeline_ms'))} p95 {med(f('host_pipeline_ms', 'p95'))}"
                  f"  frame_gap p95 {med(f('frame_gap_ms', 'p95'))}  skipped {sum(b.get('skipped_frames') or 0 for b in bs)}"
                  f"  backlog drops {sum(b.get('server_backlog_drops') or 0 for b in bs)}  fps_shown {med([b.get('fps_shown') for b in bs])}"
                  f"  kbit/s {med([b.get('kbit_s') for b in bs])}")
        if vs:
            print(f"  viewer fps {med([v.get('fps') for v in vs])}  frame_ms avg {med([v.get('frame_ms_avg') for v in vs])}"
                  f" worst p95 {pct([v.get('frame_ms_worst') for v in vs], 0.95)}  pipeline avg {med([v.get('pipeline_ms_avg') for v in vs])}"
                  f"  encoder busy drops {sum(v.get('encoder_busy_drops') or 0 for v in vs)}  chunks_pending max {max((v.get('chunks_pending') or 0) for v in vs)}")
    if ticks:
        print(f"\nserver while occupied ({len(ticks)} samples): tick p50 {med([t.get('tick_p50_ms') for t in ticks])} ms,"
              f" p99 median {med([t.get('tick_p99_ms') for t in ticks])} worst {max(t.get('tick_max_ms') or 0 for t in ticks):.1f},"
              f" dropped ticks {sum(t.get('dropped_ticks') or 0 for t in ticks)}, dig mismatches {sum(t.get('dig_mismatches') or 0 for t in ticks)},"
              f" unhandled frames {sum(t.get('frames_unhandled') or 0 for t in ticks)}, max players {max(t.get('players', 0) for t in ticks)},"
              f" max mobs {max(t.get('mobs') or 0 for t in ticks)}, pathfinding worst {max(t.get('pathfinding_ms') or 0 for t in ticks):.1f} ms")


if __name__ == "__main__":
    main()
