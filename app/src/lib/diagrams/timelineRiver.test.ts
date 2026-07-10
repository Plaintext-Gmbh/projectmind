import { describe, expect, it } from 'vitest';
import {
  bandY,
  renderTimelineRiver,
  timeX,
  type CommitActivity,
} from './timelineRiver';

const WINDOW = 24 * 30 * 86_400; // 24 months, mirrors ACTIVITY_WINDOW_SECS

describe('timeX (log time axis)', () => {
  it('anchors now to the right edge and the window edge to the left', () => {
    expect(timeX(0, WINDOW, 100, 900)).toBeCloseTo(900, 5); // now → right
    expect(timeX(WINDOW, WINDOW, 100, 900)).toBeCloseTo(100, 5); // edge → left
  });

  it('is monotonically decreasing in age (older = further left)', () => {
    let prev = Infinity;
    for (const age of [0, 3600, 86_400, 7 * 86_400, 30 * 86_400, WINDOW]) {
      const x = timeX(age, WINDOW, 100, 900);
      expect(x).toBeLessThanOrEqual(prev);
      prev = x;
    }
  });

  it('clamps ages beyond the window to the left edge', () => {
    expect(timeX(WINDOW * 5, WINDOW, 100, 900)).toBeCloseTo(100, 5);
  });

  it('never returns NaN for a degenerate zero window', () => {
    expect(Number.isFinite(timeX(0, 0, 100, 900))).toBe(true);
  });
});

describe('bandY (module → row)', () => {
  it('is monotonically increasing and evenly spaced per row', () => {
    const y0 = bandY(0);
    const y1 = bandY(1);
    const y2 = bandY(2);
    expect(y1).toBeGreaterThan(y0);
    expect(y2 - y1).toBeCloseTo(y1 - y0, 5);
  });
});

function payload(over: Partial<CommitActivity> = {}): CommitActivity {
  return {
    root: '/repo',
    now_secs: 1_800_000_000,
    window_secs: WINDOW,
    modules: [
      {
        module: 'auth',
        commits: [
          { secs_ago: 100, sha: 'aaaaaaa', summary: 'recent' },
          { secs_ago: 90 * 86_400, sha: 'bbbbbbb', summary: 'old-ish' },
        ],
      },
      {
        module: 'billing',
        commits: [{ secs_ago: 5 * 86_400, sha: 'ccccccc', summary: 'weekly' }],
      },
    ],
    total_commits: 3,
    truncated: false,
    no_git: false,
    ...over,
  };
}

describe('renderTimelineRiver', () => {
  it('renders an SVG with one drop per commit', () => {
    const svg = renderTimelineRiver(payload());
    expect(svg).toContain('<svg');
    // 3 commit drops (#38bdf8) + 2 freshness dots. Count the blue drops.
    const drops = svg.match(/fill="#38bdf8"/g) ?? [];
    expect(drops.length).toBe(3);
    // Module labels present.
    expect(svg).toContain('auth');
    expect(svg).toContain('billing');
  });

  it('empty history yields a valid, non-throwing empty SVG', () => {
    const svg = renderTimelineRiver(payload({ modules: [], total_commits: 0 }));
    expect(svg).toContain('<svg');
    expect(svg).not.toContain('#38bdf8');
    expect(svg).toContain('Keine Commit-Aktivität');
  });

  it('no-git payload yields the no-git empty state', () => {
    const svg = renderTimelineRiver(
      payload({ modules: [], total_commits: 0, no_git: true }),
    );
    expect(svg).toContain('Kein Git-Repository');
  });

  it('escapes commit summaries into tooltips', () => {
    const svg = renderTimelineRiver(
      payload({
        modules: [
          {
            module: 'x',
            commits: [{ secs_ago: 10, sha: 'ddddddd', summary: 'fix <b> & "q"' }],
          },
        ],
        total_commits: 1,
      }),
    );
    expect(svg).toContain('&lt;b&gt;');
    expect(svg).not.toContain('<b>');
  });
});
