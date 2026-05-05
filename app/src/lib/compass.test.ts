import { describe, expect, it } from 'vitest';
import { compassFor, compassIconFor } from './compass';

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

  it('returns no crumbs for an undefined target', () => {
    expect(compassFor(undefined)).toEqual([]);
  });
});

describe('compassIconFor', () => {
  it.each([
    ['class', 'C'],
    ['file', 'F'],
    ['diff', 'Δ'],
    ['note', '·'],
  ])('maps %s to %s', (kind, expected) => {
    expect(compassIconFor({ kind } as WalkthroughStep['target'])).toBe(expected);
  });

  it('returns empty for an undefined target', () => {
    expect(compassIconFor(undefined)).toBe('');
  });
});

// Tiny shim for the type-only import the it.each block needs at compile time.
type WalkthroughStep = import('./api').WalkthroughStep;
