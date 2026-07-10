import { describe, expect, it } from 'vitest';
import {
  changedBadgeFor,
  changedStatusGlyph,
  compassFor,
  compassIconFor,
  fileTrailFor,
} from './compass';
import type { ChangedFile } from './api';

describe('compassFor', () => {
  it('returns last three FQN segments for a deeply-nested class', () => {
    expect(compassFor({ kind: 'class', fqn: 'com.example.svc.user.UserService' })).toEqual([
      'svc',
      'user',
      'UserService',
    ]);
  });

  it('keeps the full FQN when there is only one segment', () => {
    expect(compassFor({ kind: 'class', fqn: 'Standalone' })).toEqual(['Standalone']);
  });

  it('caps file paths at four segments and keeps the basename', () => {
    expect(
      compassFor({
        kind: 'file',
        path: '/Users/mad/work/repo/src/main/java/com/example/UserCtrl.java',
      }),
    ).toEqual(['java', 'com', 'example', 'UserCtrl.java']);
  });

  it('returns short paths verbatim', () => {
    expect(compassFor({ kind: 'file', path: 'src/lib/foo.ts' })).toEqual([
      'src',
      'lib',
      'foo.ts',
    ]);
  });

  it('formats diff targets with `..` for explicit ranges', () => {
    expect(compassFor({ kind: 'diff', reference: 'HEAD~5', to: 'HEAD' })).toEqual([
      'HEAD~5..HEAD',
    ]);
  });

  it('marks working-tree diffs explicitly', () => {
    expect(compassFor({ kind: 'diff', reference: 'main' })).toEqual([
      'main → working tree',
    ]);
  });

  it('returns no crumbs for note targets', () => {
    expect(compassFor({ kind: 'note' })).toEqual([]);
  });

  it('shows last three FQN segments for a risk target (#160)', () => {
    expect(compassFor({ kind: 'risk', fqn: 'com.example.svc.user.UserService' })).toEqual([
      'svc',
      'user',
      'UserService',
    ]);
  });

  it('shows pattern id and scope for a pattern target (#160)', () => {
    expect(compassFor({ kind: 'pattern', pattern: 'Repository', scope: 'module:auth' })).toEqual([
      'Repository',
      'module:auth',
    ]);
    expect(compassFor({ kind: 'pattern', pattern: 'Layered' })).toEqual(['Layered']);
  });

  it('shows module or repo for an atlas target (#160)', () => {
    expect(compassFor({ kind: 'atlas', module: 'auth' })).toEqual(['atlas · auth']);
    expect(compassFor({ kind: 'atlas' })).toEqual(['atlas · repo']);
  });

  it('returns no crumbs for an undefined target', () => {
    expect(compassFor(undefined)).toEqual([]);
  });
});

describe('compassIconFor', () => {
  it.each([
    ['class', 'C'],
    ['file', 'F'],
    ['diff', 'Δ'],
    ['risk', 'R'],
    ['pattern', 'P'],
    ['atlas', '▦'],
    ['note', '·'],
  ])('maps %s to %s', (kind, expected) => {
    expect(compassIconFor({ kind } as WalkthroughStep['target'])).toBe(expected);
  });

  it('returns empty for an undefined target', () => {
    expect(compassIconFor(undefined)).toBe('');
  });
});

const CHANGED: ChangedFile[] = [
  { path: 'src/a.ts', status: 'modified' },
  { path: 'src/lib/b.ts', status: 'added' },
  { path: 'old/gone.ts', status: 'deleted' },
];

describe('changedBadgeFor', () => {
  it('reports `unknown` when there is no change data', () => {
    expect(changedBadgeFor({ kind: 'file', path: 'src/a.ts' }, [])).toEqual({
      status: 'unknown',
    });
  });

  it('reports `unknown` for targets without an addressable path', () => {
    expect(changedBadgeFor({ kind: 'diff', reference: 'HEAD~1' }, CHANGED).status).toBe('unknown');
    expect(changedBadgeFor({ kind: 'note' }, CHANGED).status).toBe('unknown');
    expect(changedBadgeFor({ kind: 'class', fqn: 'com.x.Y' }, CHANGED).status).toBe('unknown');
  });

  it('marks a file target that is in the changed set as `changed` with its status', () => {
    const b = changedBadgeFor({ kind: 'file', path: 'src/lib/b.ts' }, CHANGED);
    expect(b.status).toBe('changed');
    expect(b.file?.status).toBe('added');
  });

  it('matches an absolute target path against a repo-relative changed path', () => {
    const b = changedBadgeFor({ kind: 'file', path: '/home/me/repo/src/a.ts' }, CHANGED);
    expect(b.status).toBe('changed');
    expect(b.file?.status).toBe('modified');
  });

  it('marks a file target that is not in the changed set as `unchanged`', () => {
    expect(changedBadgeFor({ kind: 'file', path: 'src/untouched.ts' }, CHANGED).status).toBe(
      'unchanged',
    );
  });

  it('does not match on a partial (non-boundary) path tail', () => {
    // `a.ts` is a suffix of `banana.ts` textually but not on a segment boundary.
    expect(changedBadgeFor({ kind: 'file', path: 'src/banana.ts' }, CHANGED).status).toBe(
      'unchanged',
    );
  });
});

describe('changedStatusGlyph', () => {
  it.each([
    ['added', 'A'],
    ['modified', 'M'],
    ['deleted', 'D'],
    ['renamed', 'R'],
    ['type_change', '?'],
    ['other', '?'],
  ] as const)('maps %s to %s', (status, glyph) => {
    expect(changedStatusGlyph(status)).toBe(glyph);
  });
});

describe('fileTrailFor', () => {
  it('returns no dots without change data', () => {
    expect(fileTrailFor({ kind: 'file', path: 'src/a.ts' }, [])).toEqual([]);
  });

  it('builds one dot per changed file in order', () => {
    const dots = fileTrailFor({ kind: 'note' }, CHANGED);
    expect(dots.map((d) => d.path)).toEqual(['src/a.ts', 'src/lib/b.ts', 'old/gone.ts']);
    expect(dots.every((d) => !d.active)).toBe(true);
  });

  it('flags the dot matching the current file target as active', () => {
    const dots = fileTrailFor({ kind: 'file', path: 'src/lib/b.ts' }, CHANGED);
    expect(dots.find((d) => d.active)?.path).toBe('src/lib/b.ts');
    expect(dots.filter((d) => d.active)).toHaveLength(1);
  });

  it('matches an absolute active path to its repo-relative dot', () => {
    const dots = fileTrailFor({ kind: 'file', path: '/repo/src/a.ts' }, CHANGED);
    expect(dots.find((d) => d.active)?.path).toBe('src/a.ts');
  });

  it('caps the trail and keeps the active dot when it is past the cap', () => {
    const many: ChangedFile[] = Array.from({ length: 30 }, (_, i) => ({
      path: `f${i}.ts`,
      status: 'modified' as const,
    }));
    const dots = fileTrailFor({ kind: 'file', path: 'f29.ts' }, many, 10);
    expect(dots).toHaveLength(10);
    expect(dots.find((d) => d.active)?.path).toBe('f29.ts');
  });
});

// Tiny shim for the type-only import the it.each block needs at compile time.
type WalkthroughStep = import('./api').WalkthroughStep;
