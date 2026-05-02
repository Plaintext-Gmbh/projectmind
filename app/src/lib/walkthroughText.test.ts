import { describe, expect, it } from 'vitest';
import { expandStepRefs } from './walkthroughText';

describe('expandStepRefs', () => {
  it('rewrites short form to a 0-based pm:step link', () => {
    const out = expandStepRefs('see [step:3] for context');
    expect(out).toBe('see [step 3](pm:step:2) for context');
  });

  it('uses the custom label when provided', () => {
    const out = expandStepRefs('jump to [step:5|the bean graph stop]');
    expect(out).toBe('jump to [the bean graph stop](pm:step:4)');
  });

  it('rewrites every match, not just the first', () => {
    const out = expandStepRefs('compare [step:1] with [step:7]');
    expect(out).toBe('compare [step 1](pm:step:0) with [step 7](pm:step:6)');
  });

  it('trims whitespace inside the custom label', () => {
    const out = expandStepRefs('see [step:2|  bean graph  ]');
    expect(out).toBe('see [bean graph](pm:step:1)');
  });

  it('falls back to default label when the explicit label is empty', () => {
    // `[step:2|]` and `[step:2|   ]` should both yield "step 2", not an
    // empty link text — Markdown renders an empty `[]()` as nothing.
    expect(expandStepRefs('a [step:2|] b')).toBe('a [step 2](pm:step:1) b');
    expect(expandStepRefs('a [step:2|   ] b')).toBe('a [step 2](pm:step:1) b');
  });

  it('leaves out-of-range numeric refs untouched so the typo is visible', () => {
    // 0-based (`[step:0]`) is a typo — the UI calls them step 1, 2, ….
    expect(expandStepRefs('typo: [step:0]')).toBe('typo: [step:0]');
  });

  it('leaves non-numeric refs untouched', () => {
    expect(expandStepRefs('see [step:abc]')).toBe('see [step:abc]');
  });

  it('leaves regular markdown links alone', () => {
    const md = '[link](https://example.com) and [step:1]';
    expect(expandStepRefs(md)).toBe(
      '[link](https://example.com) and [step 1](pm:step:0)',
    );
  });
});
