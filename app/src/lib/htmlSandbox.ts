/// Shared HTML sandboxing for untrusted markup (repo HTML files, HTML
/// snippets, and AI-generated `present_artifact` HTML). Rendering is always
/// done in an `<iframe sandbox="" src={data:URL}>` — the empty `sandbox`
/// attribute already blocks scripts, forms, popups and same-origin access.
/// On top of that we inject a strict Content-Security-Policy so even a
/// document that somehow slips past the sandbox (or a future relaxation of
/// the attribute) still cannot execute scripts or reach the network.
///
/// Keeping this in one place means every HTML surface in the app shares the
/// exact same policy, and vitest can pin the guarantees without a DOM.

/// The locked-down policy: no scripts, no network of any kind, no forms, no
/// base-uri tricks. Only inline styles and `data:` images/fonts/media are
/// allowed so a self-contained document still renders.
export const SANDBOX_CSP =
  "default-src 'none'; img-src data:; style-src 'unsafe-inline' data:;" +
  " font-src data:; media-src data:; child-src 'none'; frame-src 'none';" +
  " form-action 'none'; base-uri 'none';";

/// Wrap arbitrary HTML in a full, CSP-locked document. If `source` already is
/// a full document (`<html>`), the CSP `<meta>` is injected into its `<head>`;
/// otherwise the source is treated as a body fragment. Either way the strict
/// iframe sandbox is the primary defence — the CSP is defence-in-depth.
export function wrapSandboxedHtml(source: string): string {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${SANDBOX_CSP}">`;
  const hasHtmlTag = /<html[\s>]/i.test(source);
  if (hasHtmlTag) {
    if (/<head[\s>]/i.test(source)) {
      return source.replace(/<head([^>]*)>/i, `<head$1>${meta}`);
    }
    // Full document without a <head> — inject one right after <html …>.
    return source.replace(/<html([^>]*)>/i, `<html$1><head>${meta}</head>`);
  }
  return `<!doctype html>
<html>
<head>
${meta}
<style>
  :root { color-scheme: light dark; }
  body { font-family: system-ui, sans-serif; color: #222; background: #fff; padding: 16px; margin: 0; }
  @media (prefers-color-scheme: dark) { body { color: #ddd; background: #1a1a1a; } }
  img, table, pre { max-width: 100%; }
</style>
</head>
<body>${source}</body>
</html>`;
}

/// The `data:text/html` URL an iframe's `src` should point at to render
/// `source` under the sandbox policy.
export function sandboxedHtmlDataUrl(source: string): string {
  return `data:text/html;charset=utf-8,${encodeURIComponent(wrapSandboxedHtml(source))}`;
}
