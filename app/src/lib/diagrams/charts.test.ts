import { describe, expect, it } from 'vitest';
import { esc, formatBytes, shortLabel } from './common';
import { renderLanguageStats, type LanguageStats } from './languageStats';
import { renderModuleChord, type ModuleChord } from './moduleChord';
import { renderArchitectureFlow, type ArchitectureFlow } from './architectureFlow';
import { renderActivityHeatmap, type ActivityHeatmap } from './activityHeatmap';

describe('common helpers', () => {
  it('esc escapes the five XML-significant characters', () => {
    expect(esc(`<a href="x" data='y'>&`)).toBe(
      '&lt;a href=&quot;x&quot; data=&#39;y&#39;&gt;&amp;',
    );
  });

  it('formatBytes scales B / KB / MB / GB', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe('3.00 GB');
  });

  it('shortLabel truncates with an ellipsis past the limit', () => {
    expect(shortLabel('short', 10)).toBe('short');
    expect(shortLabel('abcdefghij', 5)).toBe('abcd…');
  });
});

describe('renderLanguageStats', () => {
  const stats: LanguageStats = {
    root: '.',
    total_files: 4,
    total_bytes: 4096,
    truncated: false,
    buckets: [
      { language: 'TypeScript', extension: 'ts', files: 3, bytes: 3072 },
      { language: 'Rust', extension: 'rs', files: 1, bytes: 1024 },
    ],
  };

  it('placeholders when there are no buckets', () => {
    expect(renderLanguageStats({ ...stats, buckets: [] })).toContain('No files found');
  });

  it('renders one bar row per bucket with correct percentages', () => {
    const svg = renderLanguageStats(stats);
    expect((svg.match(/class="row"/g) ?? []).length).toBe(2);
    // 3 of 4 files = 75.0%, 1 of 4 = 25.0%.
    expect(svg).toContain('3 · 75.0%');
    expect(svg).toContain('1 · 25.0%');
  });

  it('the longest bucket bar is widest (proportional to file count)', () => {
    const svg = renderLanguageStats(stats);
    const widths = [...svg.matchAll(/<rect x="212" y="\d+" width="(\d+)"/g)].map((m) =>
      Number(m[1]),
    );
    expect(widths).toHaveLength(2);
    expect(widths[0]).toBeGreaterThan(widths[1]);
  });
});

describe('renderModuleChord', () => {
  it('placeholders when there are no modules', () => {
    const empty: ModuleChord = { root: '.', modules: [], edges: [], total_relations: 0 };
    expect(renderModuleChord(empty)).toContain('Keine Module erkannt');
  });

  it('draws one rim arc per module and one chord per edge', () => {
    const chord: ModuleChord = {
      root: '.',
      modules: [
        { id: 'core', label: 'core', classes: 10, outgoing: 2, incoming: 1, internal: 5 },
        { id: 'api', label: 'api', classes: 4, outgoing: 1, incoming: 2, internal: 1 },
      ],
      edges: [{ from: 'core', to: 'api', count: 3 }],
      total_relations: 6,
    };
    const svg = renderModuleChord(chord);
    expect((svg.match(/class="rim"/g) ?? []).length).toBe(2);
    expect(svg).toContain('core → api: 3');
  });
});

describe('renderArchitectureFlow', () => {
  it('placeholders when no classes were parsed', () => {
    const empty: ArchitectureFlow = {
      root: '.',
      total_classes: 0,
      total_modules: 0,
      cross_module_edges: 0,
      layers: [],
      edges: [],
    };
    expect(renderArchitectureFlow(empty)).toContain('Keine Klassen erkannt');
  });

  it('renders a chip per class and escapes fqns in tooltips', () => {
    const flow: ArchitectureFlow = {
      root: '.',
      total_classes: 1,
      total_modules: 1,
      cross_module_edges: 0,
      layers: [
        {
          id: 'web',
          label: 'Web',
          description: 'controllers',
          color: '#3b82f6',
          classes: [{ fqn: 'com.x.A<B>', name: 'A', module: 'm', stereotype: 'Controller' }],
          stereotypes: { Controller: 1 },
        },
      ],
      edges: [],
    };
    const svg = renderArchitectureFlow(flow);
    expect((svg.match(/class="chip"/g) ?? []).length).toBe(1);
    expect(svg).toContain('com.x.A&lt;B&gt;');
  });
});

describe('renderActivityHeatmap', () => {
  it('placeholders when there are no days', () => {
    const empty: ActivityHeatmap = {
      root: '.',
      start_date: '2024-01-01',
      end_date: '2024-01-01',
      days: [],
      total_commits: 0,
      distinct_authors: 0,
      top_authors: [],
      max_commits_per_day: 0,
      longest_streak_days: 0,
      truncated: false,
      no_git: false,
    };
    expect(renderActivityHeatmap(empty)).toContain('Keine Aktivität verfügbar');
  });

  it('renders one cell per day and lists the top authors', () => {
    const heat: ActivityHeatmap = {
      root: '.',
      start_date: '2024-01-01',
      end_date: '2024-01-03',
      days: [
        { date: '2024-01-01', commits: 0, top_authors: [] },
        { date: '2024-01-02', commits: 5, top_authors: [{ name: 'Ada', commits: 5 }] },
        { date: '2024-01-03', commits: 2, top_authors: [{ name: 'Ada', commits: 2 }] },
      ],
      total_commits: 7,
      distinct_authors: 1,
      top_authors: [{ name: 'Ada', commits: 7 }],
      max_commits_per_day: 5,
      longest_streak_days: 2,
      truncated: false,
      no_git: false,
    };
    const svg = renderActivityHeatmap(heat);
    // 3 day cells + 6 legend swatches = 9 rounded rects with rx="2".
    expect((svg.match(/rx="2"/g) ?? []).length).toBe(3 + 6);
    expect(svg).toContain('1. Ada — 7');
  });

  it('shows the no-git warning instead of stats', () => {
    const heat: ActivityHeatmap = {
      root: '.',
      start_date: '2024-01-01',
      end_date: '2024-01-01',
      days: [{ date: '2024-01-01', commits: 0, top_authors: [] }],
      total_commits: 0,
      distinct_authors: 0,
      top_authors: [],
      max_commits_per_day: 0,
      longest_streak_days: 0,
      truncated: false,
      no_git: true,
    };
    const svg = renderActivityHeatmap(heat);
    expect(svg).toContain('Keine Git-Historie verfügbar');
    expect(svg).not.toContain('Übersicht');
  });
});
