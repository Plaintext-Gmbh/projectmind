import { describe, expect, it } from 'vitest';
import { firstHighlightRange } from './walkthroughHighlight';

describe('firstHighlightRange (#175)', () => {
  it('returns null when the step has no highlights', () => {
    expect(firstHighlightRange(undefined)).toBeNull();
    expect(firstHighlightRange(null)).toBeNull();
    expect(firstHighlightRange([])).toBeNull();
  });

  it('returns the only range as-is', () => {
    expect(firstHighlightRange([{ from: 58, to: 77 }])).toEqual({ from: 58, to: 77 });
  });

  it('picks the topmost range, not the first in array order', () => {
    expect(
      firstHighlightRange([
        { from: 120, to: 130 },
        { from: 12, to: 18 },
        { from: 58, to: 77 },
      ]),
    ).toEqual({ from: 12, to: 18 });
  });

  it('keeps the first-encountered range on equal starts', () => {
    expect(
      firstHighlightRange([
        { from: 40, to: 45 },
        { from: 40, to: 60 },
      ]),
    ).toEqual({ from: 40, to: 45 });
  });
});
