import { describe, expect, it } from 'vitest';
import {
  authorColor,
  authorIdentity,
  buildAuthorLegend,
  humanizeAge,
  recencyColor,
  recencyLegend,
} from './folderMapColors';

/// Pull `H/S/L` triples out of an `hsl(h, s%, l%)` string for numeric
/// comparison. The renderer rounds at emit time, so the test only has to
/// match the rounded values.
function parseHsl(s: string): { h: number; sat: number; light: number } {
  const m = /^hsl\(\s*(\d+),\s*(\d+)%,\s*(\d+)%\)$/.exec(s);
  if (!m) throw new Error(`not an hsl() string: ${s}`);
  return { h: Number(m[1]), sat: Number(m[2]), light: Number(m[3]) };
}

describe('recencyColor', () => {
  const day = 86_400;

  it('emits a fresh, hot orange hue for very recent commits', () => {
    // 5 minutes old — anchored at the floor (60 s clamp), still solidly
    // on the "today" side of the scale.
    const { h, sat, light } = parseHsl(recencyColor(300));
    expect(h).toBeLessThanOrEqual(40); // close to 18°, the hot anchor
    expect(sat).toBeGreaterThan(60);
    expect(light).toBeGreaterThanOrEqual(45);
  });

  it('decays toward the cool blue end past a year', () => {
    const tenYears = day * 365 * 10;
    const { h, sat } = parseHsl(recencyColor(tenYears));
    expect(h).toBeGreaterThanOrEqual(200); // close to 220°, the cool anchor
    expect(sat).toBeLessThanOrEqual(35); // saturation drops with age
  });

  it('clamps `t` to [0, 1] for inputs outside the design range', () => {
    // Negative input → secs_ago floors to 60s, t=0, hue stays at the hot
    // anchor. Same as a brand-new commit.
    const negative = recencyColor(-1_000);
    const fiveMinutes = recencyColor(300);
    expect(negative).toBe(fiveMinutes);

    // Inputs past the 1000-days knee saturate at the cool anchor —
    // a 100-year-old file should look the same as a 10-year-old file.
    expect(recencyColor(day * 365 * 100)).toBe(recencyColor(day * 365 * 10));
  });

  it('moves monotonically toward blue as commits age', () => {
    const samples = [day, day * 7, day * 30, day * 365, day * 1000].map(
      (s) => parseHsl(recencyColor(s)).h,
    );
    for (let i = 1; i < samples.length; i++) {
      expect(samples[i]).toBeGreaterThanOrEqual(samples[i - 1]);
    }
  });

  it('produces a parseable hsl() string', () => {
    expect(recencyColor(day * 30)).toMatch(/^hsl\(\d+, \d+%, \d+%\)$/);
  });
});

describe('authorColor', () => {
  it('is stable: the same identity always returns the same colour', () => {
    expect(authorColor('alice@example.com')).toBe(authorColor('alice@example.com'));
  });

  it('produces visually distinct hues for different identities', () => {
    // Three random-ish identities should land on three different hues.
    // Not asserting "all 3 unique" because djb2 + mod 360 has collisions —
    // this is just a smoke check that the hash isn't flat-lining.
    const colours = new Set([
      authorColor('alice@example.com'),
      authorColor('bob@example.com'),
      authorColor('charlie@example.com'),
    ]);
    expect(colours.size).toBeGreaterThanOrEqual(2);
  });

  it('handles unicode identities without throwing', () => {
    expect(() => authorColor('müller@example.com')).not.toThrow();
    expect(authorColor('müller@example.com')).toMatch(/^hsl\(\d+, 60%, 52%\)$/);
  });

  it('handles the empty identity by returning the seed hue', () => {
    // djb2 seed is 5381 → hue = 5381 % 360 = 341°. Pinning the seed
    // behaviour so a future refactor that changes it is loud.
    expect(authorColor('')).toBe('hsl(341, 60%, 52%)');
  });

  it('treats different-cased emails as different identities', () => {
    // The caller (authorIdentity) lowercases emails before this point, so
    // the colour function does NOT do its own normalisation. Documenting
    // that contract here so a well-meaning refactor doesn't silently
    // collapse author hues.
    expect(authorColor('Alice@example.com')).not.toBe(authorColor('alice@example.com'));
  });
});

describe('authorIdentity', () => {
  it('prefers email over name', () => {
    expect(authorIdentity('Alice', 'alice@example.com')).toBe('alice@example.com');
  });

  it('lowercases the email so case differences collapse to one hue', () => {
    expect(authorIdentity('Alice', 'ALICE@EXAMPLE.COM')).toBe('alice@example.com');
  });

  it('falls back to the trimmed name when email is missing', () => {
    expect(authorIdentity('  Alice  ', null)).toBe('Alice');
    expect(authorIdentity('Alice', '')).toBe('Alice');
    expect(authorIdentity('Alice', '   ')).toBe('Alice');
  });

  it('returns null when both inputs are empty / missing', () => {
    expect(authorIdentity(null, null)).toBeNull();
    expect(authorIdentity('', '')).toBeNull();
    expect(authorIdentity('   ', '   ')).toBeNull();
    expect(authorIdentity(undefined, undefined)).toBeNull();
  });
});

describe('recencyLegend', () => {
  const day = 86_400;

  it('has exactly the three documented stops in fresh→stale order', () => {
    const stops = recencyLegend();
    expect(stops.map((s) => s.key)).toEqual(['today', 'week', 'stale']);
    // Ages increase down the legend.
    for (let i = 1; i < stops.length; i++) {
      expect(stops[i].secs_ago).toBeGreaterThan(stops[i - 1].secs_ago);
    }
  });

  it('samples the real recencyColor so swatches never drift from the scale', () => {
    for (const stop of recencyLegend()) {
      expect(stop.color).toBe(recencyColor(stop.secs_ago));
    }
  });

  it('spans the hot→cool hue range across the three stops', () => {
    const [today, , stale] = recencyLegend();
    const hot = Number(/hsl\((\d+)/.exec(today.color)![1]);
    const cold = Number(/hsl\((\d+)/.exec(stale.color)![1]);
    expect(hot).toBeLessThanOrEqual(60); // near the hot anchor
    expect(cold).toBeGreaterThanOrEqual(150); // well toward the cool anchor
    expect(stale.secs_ago).toBeGreaterThan(day * 180); // > 6 months
  });
});

describe('buildAuthorLegend', () => {
  it('aggregates commits per author and picks the freshest touch', () => {
    const rows = buildAuthorLegend([
      { author: 'alice@example.com', secs_ago: 500 },
      { author: 'alice@example.com', secs_ago: 100 },
      { author: 'bob@example.com', secs_ago: 900 },
    ]);
    expect(rows).toHaveLength(2);
    const alice = rows.find((r) => r.identity === 'alice@example.com')!;
    expect(alice.commits).toBe(2);
    expect(alice.lastTouchedSecsAgo).toBe(100); // freshest of the two
    expect(alice.color).toBe(authorColor('alice@example.com'));
  });

  it('sorts by commit count desc, then identity asc — deterministically', () => {
    const facts = [
      { author: 'bob@example.com', secs_ago: 10 },
      { author: 'alice@example.com', secs_ago: 10 },
      { author: 'alice@example.com', secs_ago: 20 },
      { author: 'carol@example.com', secs_ago: 10 }, // ties bob on 1 commit
    ];
    const first = buildAuthorLegend(facts).map((r) => `${r.identity}:${r.commits}`);
    // Alice (2) leads; bob & carol tie at 1 → identity ascending (bob < carol).
    expect(first).toEqual([
      'alice@example.com:2',
      'bob@example.com:1',
      'carol@example.com:1',
    ]);
    // Re-running on a shuffled input yields the same order.
    const shuffled = buildAuthorLegend([facts[3], facts[1], facts[0], facts[2]]).map(
      (r) => `${r.identity}:${r.commits}`,
    );
    expect(shuffled).toEqual(first);
  });

  it('skips files with no author and never throws on empty input', () => {
    expect(buildAuthorLegend([])).toEqual([]);
    expect(
      buildAuthorLegend([
        { author: null, secs_ago: 100 },
        { author: 'alice@example.com', secs_ago: 100 },
      ]),
    ).toHaveLength(1);
  });
});

describe('humanizeAge', () => {
  const day = 86_400;

  it('collapses sub-minute / negative ages to "just now"', () => {
    expect(humanizeAge(0)).toBe('just now');
    expect(humanizeAge(59)).toBe('just now');
    expect(humanizeAge(-100)).toBe('just now');
  });

  it('steps through the units as age grows', () => {
    expect(humanizeAge(60 * 5)).toBe('5m');
    expect(humanizeAge(3600 * 3)).toBe('3h');
    expect(humanizeAge(day * 3)).toBe('3d');
    expect(humanizeAge(day * 14)).toBe('2w');
    expect(humanizeAge(day * 90)).toBe('3mo');
    expect(humanizeAge(day * 365 * 2)).toBe('2y');
  });
});
