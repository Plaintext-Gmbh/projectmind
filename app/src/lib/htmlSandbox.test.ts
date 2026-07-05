import { describe, it, expect } from 'vitest';
import { SANDBOX_CSP, wrapSandboxedHtml, sandboxedHtmlDataUrl } from './htmlSandbox';

describe('htmlSandbox', () => {
  it('locks the CSP down: no scripts, no network', () => {
    expect(SANDBOX_CSP).toContain("default-src 'none'");
    expect(SANDBOX_CSP).toContain("form-action 'none'");
    expect(SANDBOX_CSP).toContain("base-uri 'none'");
    // No script-src escape hatch — default-src 'none' governs scripts, and we
    // never widen it to 'unsafe-inline'/'unsafe-eval' for script.
    expect(SANDBOX_CSP).not.toContain('script-src');
    expect(SANDBOX_CSP).not.toContain("'unsafe-eval'");
  });

  it('wraps a fragment into a full CSP-locked document', () => {
    const html = wrapSandboxedHtml('<p>hello</p>');
    expect(html).toContain('<!doctype html>');
    expect(html).toContain(`content="${SANDBOX_CSP}"`);
    expect(html).toContain('<p>hello</p>');
  });

  it('keeps an AI-authored <script> verbatim (inert, not executed by us)', () => {
    // The script must survive as *text* — the sandbox iframe + CSP neutralise
    // it. We must never strip-and-inject it into the app DOM.
    const evil = '<h1>Report</h1><script>alert(1)</script>';
    const html = wrapSandboxedHtml(evil);
    expect(html).toContain('<script>alert(1)</script>');
    // And the CSP that renders it inert is present.
    expect(html).toContain("default-src 'none'");
  });

  it('injects the CSP meta into an existing <head>', () => {
    const doc = '<html><head><title>T</title></head><body>x</body></html>';
    const html = wrapSandboxedHtml(doc);
    expect(html).toContain('Content-Security-Policy');
    // meta lands inside the head, before the title.
    expect(html.indexOf('Content-Security-Policy')).toBeLessThan(html.indexOf('<title>'));
  });

  it('adds a head when a full document lacks one', () => {
    const doc = '<html><body>x</body></html>';
    const html = wrapSandboxedHtml(doc);
    expect(html).toContain('<head>');
    expect(html).toContain('Content-Security-Policy');
  });

  it('produces an encoded data:text/html URL', () => {
    const url = sandboxedHtmlDataUrl('<p>a & b</p>');
    expect(url.startsWith('data:text/html;charset=utf-8,')).toBe(true);
    // The ampersand is percent-encoded, so nothing breaks the URL.
    expect(url).toContain('%3Cp%3E');
  });
});
