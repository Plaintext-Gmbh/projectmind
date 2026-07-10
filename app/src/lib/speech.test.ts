import { describe, expect, it } from 'vitest';
import { narrationToSpeech } from './speech';

describe('narrationToSpeech', () => {
  it('strips fenced code blocks', () => {
    const md = 'Look here:\n```java\nvoid f() {}\n```\nand carry on.';
    expect(narrationToSpeech(md)).toBe('Look here: and carry on.');
  });

  it('keeps inline code content without the backticks', () => {
    expect(narrationToSpeech('Call `validateToken` first.')).toBe('Call validateToken first.');
  });

  it('drops emphasis and heading markers', () => {
    expect(narrationToSpeech('# Title\nThis is **bold** and _italic_.')).toBe(
      'Title This is bold and italic.',
    );
  });

  it('keeps the link label and drops the URL', () => {
    expect(narrationToSpeech('See [the docs](https://example.com/x) now.')).toBe(
      'See the docs now.',
    );
  });

  it('collapses whitespace and trims', () => {
    expect(narrationToSpeech('  a\n\n\n b   c  ')).toBe('a b c');
  });

  it('returns empty string for empty/whitespace input', () => {
    expect(narrationToSpeech('')).toBe('');
    expect(narrationToSpeech('   \n\t')).toBe('');
  });
});
