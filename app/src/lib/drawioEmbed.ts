/// Pure helpers for the diagrams.net embed protocol used by DrawIoFrame —
/// URL construction and message (de)serialisation, kept out of the Svelte
/// component so they can be unit-tested (#142, draw.io edit path).
///
/// Protocol summary (proto=json):
///   iframe → host  {event:'init'}                    editor ready, send `load`
///   host → iframe  {action:'load', xml, autosave}    load a diagram
///   iframe → host  {event:'save', xml}               user pressed Save
///   iframe → host  {event:'exit', modified}          user pressed Exit
///   host → iframe  {action:'status', modified:false} clear "unsaved" marker
///
/// Privacy: the diagram XML crosses into an iframe served by
/// embed.diagrams.net — a third-party service — in BOTH modes. Editing adds
/// no new data flow over viewing, which the app already did.
export const EMBED_ORIGIN = 'https://embed.diagrams.net';

export type EmbedMode = 'view' | 'edit';

/// Embed URL for a mode.
///   view — canvas only (`chrome=0`), the host wrapper pans/zooms.
///   edit — full editor chrome with the embed's Save / Exit buttons.
/// Common: `embed=1` + `proto=json` (postMessage protocol), no splash,
/// follow OS dark mode.
export function embedUrl(mode: EmbedMode): string {
  const common = 'embed=1&ui=atlas&proto=json&splash=0&dark=auto';
  if (mode === 'edit') return `${EMBED_ORIGIN}/?${common}&spin=1&libraries=1`;
  return `${EMBED_ORIGIN}/?${common}&toolbar=0&libraries=0&chrome=0`;
}

export type EmbedEvent =
  | { event: 'init' }
  | { event: 'save'; xml: string }
  | { event: 'exit'; modified: boolean }
  | { event: 'autosave'; xml: string }
  | { event: string };

/// Decode a postMessage payload from the embed. Accepts the JSON string the
/// protocol sends (and a pre-parsed object, for tests). Returns null for
/// anything that is not an object carrying an `event` string, so callers
/// can ignore unrelated window messages safely.
export function parseEmbedMessage(data: unknown): EmbedEvent | null {
  let obj: unknown = data;
  if (typeof data === 'string') {
    try {
      obj = JSON.parse(data);
    } catch {
      return null;
    }
  }
  if (!obj || typeof obj !== 'object') return null;
  const rec = obj as Record<string, unknown>;
  if (typeof rec.event !== 'string') return null;
  switch (rec.event) {
    case 'save':
    case 'autosave':
      return typeof rec.xml === 'string' ? { event: rec.event, xml: rec.xml } : null;
    case 'exit':
      return { event: 'exit', modified: rec.modified === true };
    default:
      return { event: rec.event };
  }
}

/// The `load` action: hand the XML to the editor. `autosave` stays off in
/// both modes — the user saves explicitly, which is the only moment the file
/// on disk changes.
export function loadMessage(xml: string, title?: string): string {
  const msg: Record<string, unknown> = { action: 'load', xml, autosave: 0 };
  if (title) msg.title = title;
  return JSON.stringify(msg);
}

/// Tell the editor the last `save` landed so it drops its "unsaved changes"
/// marker.
export function savedStatusMessage(): string {
  return JSON.stringify({ action: 'status', messageKey: 'allChangesSaved', modified: false });
}

/// Clock label for the "Saved 14:07" status.
export function savedAtLabel(date: Date): string {
  const hh = String(date.getHours()).padStart(2, '0');
  const mm = String(date.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}
