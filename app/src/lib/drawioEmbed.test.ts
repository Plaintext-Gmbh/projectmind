import { describe, expect, it } from 'vitest';
import {
  EMBED_ORIGIN,
  embedUrl,
  loadMessage,
  parseEmbedMessage,
  savedAtLabel,
  savedStatusMessage,
} from './drawioEmbed';

describe('embedUrl', () => {
  it('view mode strips the editor chrome and keeps the json protocol', () => {
    const url = embedUrl('view');
    expect(url.startsWith(`${EMBED_ORIGIN}/?`)).toBe(true);
    expect(url).toContain('embed=1');
    expect(url).toContain('proto=json');
    expect(url).toContain('chrome=0');
    expect(url).toContain('toolbar=0');
  });

  it('edit mode keeps the chrome (Save / Exit buttons) and the shape libraries', () => {
    const url = embedUrl('edit');
    expect(url).toContain('embed=1');
    expect(url).toContain('proto=json');
    expect(url).not.toContain('chrome=0');
    expect(url).not.toContain('toolbar=0');
    expect(url).toContain('libraries=1');
  });

  it('both modes stay on the same origin so the CSP frame-src allowlist holds', () => {
    for (const mode of ['view', 'edit'] as const) {
      expect(new URL(embedUrl(mode)).origin).toBe(EMBED_ORIGIN);
    }
  });
});

describe('parseEmbedMessage', () => {
  it('decodes the JSON string the protocol sends', () => {
    expect(parseEmbedMessage(JSON.stringify({ event: 'init' }))).toEqual({ event: 'init' });
  });

  it('carries the xml on save and autosave, rejects them without xml', () => {
    expect(parseEmbedMessage({ event: 'save', xml: '<mxfile/>' })).toEqual({
      event: 'save',
      xml: '<mxfile/>',
    });
    expect(parseEmbedMessage({ event: 'autosave', xml: '<x/>' })).toEqual({
      event: 'autosave',
      xml: '<x/>',
    });
    expect(parseEmbedMessage({ event: 'save' })).toBeNull();
  });

  it('normalises exit.modified to a boolean', () => {
    expect(parseEmbedMessage({ event: 'exit', modified: true })).toEqual({
      event: 'exit',
      modified: true,
    });
    expect(parseEmbedMessage({ event: 'exit' })).toEqual({ event: 'exit', modified: false });
  });

  it('ignores unrelated window messages and garbage', () => {
    expect(parseEmbedMessage('not json')).toBeNull();
    expect(parseEmbedMessage(null)).toBeNull();
    expect(parseEmbedMessage(42)).toBeNull();
    expect(parseEmbedMessage({ type: 'webpackOk' })).toBeNull();
    expect(parseEmbedMessage({ event: 7 })).toBeNull();
  });

  it('passes unknown event names through so callers can log them', () => {
    expect(parseEmbedMessage({ event: 'configure' })).toEqual({ event: 'configure' });
  });
});

describe('messages to the editor', () => {
  it('load carries the xml and keeps autosave off', () => {
    const msg = JSON.parse(loadMessage('<mxfile/>', 'architecture.drawio'));
    expect(msg).toEqual({
      action: 'load',
      xml: '<mxfile/>',
      autosave: 0,
      title: 'architecture.drawio',
    });
    expect(JSON.parse(loadMessage('<x/>'))).not.toHaveProperty('title');
  });

  it('saved status clears the modified flag', () => {
    expect(JSON.parse(savedStatusMessage())).toEqual({
      action: 'status',
      messageKey: 'allChangesSaved',
      modified: false,
    });
  });
});

describe('savedAtLabel', () => {
  it('zero-pads hours and minutes', () => {
    expect(savedAtLabel(new Date(2026, 7, 28, 9, 5))).toBe('09:05');
    expect(savedAtLabel(new Date(2026, 7, 28, 14, 30))).toBe('14:30');
  });
});
