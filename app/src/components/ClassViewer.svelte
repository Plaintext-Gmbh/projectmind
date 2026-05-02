<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import type {
    ClassEntry,
    ClassOutline,
    MethodOutline,
    FieldOutline,
    AnnotationRef,
  } from '../lib/api';
  import { classOutline as fetchOutline } from '../lib/api';
  import { classes, selectedClass } from '../lib/store';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
  import { t } from '../lib/i18n';

  export let klass: ClassEntry;
  export let source: string;
  export let meta: { file: string; line_start: number; line_end: number } | null;
  /// Walk-through highlight ranges (1-based, inclusive). When set, these
  /// take precedence over the default class-bounds highlight and use a
  /// more vivid colour so the LLM-pointed lines stand out.
  export let highlightRanges: Array<{ from: number; to: number }> = [];

  $: lines = source.split('\n');
  $: defaultFrom = meta?.line_start ?? 0;
  $: defaultTo = meta?.line_end ?? 0;

  function inWalkthroughRange(line: number): boolean {
    return highlightRanges.some((r) => line >= r.from && line <= r.to);
  }

  // Shift + wheel zoom, persisted under the per-component key.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.classviewer.zoom');

  // ----- Outline panel -----------------------------------------------------

  // The outline ships methods + fields without source so it's cheap to fetch
  // every time the selected class changes. Backend uses the same data path
  // as the `class_outline` MCP tool, so what the GUI shows here is exactly
  // what the LLM sees.
  let outline: ClassOutline | null = null;
  let outlineFqn: string | null = null;
  // Persist the panel's open/closed state across class switches and reloads.
  const OUTLINE_KEY = 'projectmind.classviewer.outlineOpen';
  const GUTTER_KEY = 'projectmind.classviewer.gutterOpen';
  let outlineOpen = readBoolPref(OUTLINE_KEY, true);
  // Gutter defaults to *open* too — same data, same discovery rationale as
  // the side panel. Cheap to suppress with the toolbar toggle if it ever
  // gets in the way.
  let gutterOpen = readBoolPref(GUTTER_KEY, true);
  let sourceEl: HTMLPreElement | null = null;
  let lastFlash: number | null = null;

  function readBoolPref(key: string, defaultValue: boolean): boolean {
    try {
      const v = localStorage.getItem(key);
      return v === null ? defaultValue : v === '1';
    } catch {
      return defaultValue;
    }
  }

  function writeBoolPref(key: string, v: boolean) {
    try {
      localStorage.setItem(key, v ? '1' : '0');
    } catch {
      // ignore
    }
  }

  function toggleOutline() {
    outlineOpen = !outlineOpen;
    writeBoolPref(OUTLINE_KEY, outlineOpen);
  }

  function toggleGutter() {
    gutterOpen = !gutterOpen;
    writeBoolPref(GUTTER_KEY, gutterOpen);
  }

  // Whenever the selected class changes, refetch its outline. We dedupe by
  // fqn so re-renders that don't actually change the class don't re-fire.
  $: if (klass && klass.fqn !== outlineFqn) {
    outlineFqn = klass.fqn;
    outline = null;
    void loadOutline(klass.fqn);
  }

  async function loadOutline(fqn: string) {
    try {
      const o = await fetchOutline(fqn);
      // Race guard: discard if the user clicked another class meanwhile.
      if (outlineFqn === fqn) outline = o;
    } catch (err) {
      // Non-fatal — the class viewer is still useful without the outline.
      console.warn('class_outline failed:', err);
      if (outlineFqn === fqn) outline = null;
    }
  }

  function jumpToLine(line: number) {
    if (!sourceEl) return;
    const target = sourceEl.querySelector<HTMLElement>(`[data-line-no="${line}"]`);
    if (!target) return;
    target.scrollIntoView({ behavior: 'smooth', block: 'center' });
    // Brief flash so the eye finds the row even when the surrounding code
    // looks similar. Reuses the .flash style already in this component.
    if (lastFlash !== null) clearTimeout(lastFlash);
    target.classList.add('flash');
    lastFlash = window.setTimeout(() => {
      target.classList.remove('flash');
      lastFlash = null;
    }, 1200);
  }

  function visibilityGlyph(v: MethodOutline['visibility'] | FieldOutline['visibility']): string {
    switch (v) {
      case 'public':
        return '+';
      case 'protected':
        return '#';
      case 'private':
        return '-';
      default:
        return '~';
    }
  }

  // ----- Annotated gutter -------------------------------------------------

  // What we render on a single line of the gutter. The class-level entry
  // also carries stereotypes (so we can show framework-recognised badges
  // like `service` / `controller` next to the class declaration); per-member
  // entries carry the visibility glyph instead. `annotations` is the full
  // list — first one shows up as the primary chip, the rest live in the
  // tooltip.
  type GutterItem = {
    kind: 'class' | 'method' | 'field';
    name?: string;
    visibility?: MethodOutline['visibility'];
    isStatic?: boolean;
    annotations: AnnotationRef[];
    stereotypes?: string[];
  };

  // Reactive map keyed by source-line number. Only includes lines that have
  // *something* to show — method/field declarations and the class header.
  $: gutterByLine = (() => {
    const map = new Map<number, GutterItem>();
    if (!outline) return map;
    if (outline.stereotypes.length > 0 || outline.annotations.length > 0) {
      map.set(outline.line_start, {
        kind: 'class',
        annotations: outline.annotations,
        stereotypes: outline.stereotypes,
      });
    }
    for (const m of outline.methods) {
      // Don't clobber the class-level marker if a method coincidentally
      // shares a line (very rare in practice — would only happen on a
      // single-line class). Class wins because it tells the bigger story.
      if (map.has(m.line_start)) continue;
      map.set(m.line_start, {
        kind: 'method',
        name: m.name,
        visibility: m.visibility,
        isStatic: m.is_static,
        annotations: m.annotations,
      });
    }
    for (const f of outline.fields) {
      if (map.has(f.line)) continue;
      map.set(f.line, {
        kind: 'field',
        name: f.name,
        visibility: f.visibility,
        isStatic: f.is_static,
        annotations: f.annotations,
      });
    }
    return map;
  })();

  /// Render a single annotation as `@Name` plus its raw arg list when present
  /// (e.g. `@RequestMapping(value="/users", method=GET)`). The args text is
  /// passed through verbatim — it's the same string the language plugin
  /// extracted from source.
  function formatAnnotation(a: AnnotationRef): string {
    const args = a.raw_args ?? '';
    return args.length > 0 ? `@${a.name}(${args})` : `@${a.name}`;
  }

  function gutterTooltip(item: GutterItem): string {
    const parts: string[] = [];
    if (item.kind === 'class') {
      if (item.stereotypes && item.stereotypes.length > 0) {
        parts.push(`stereotypes: ${item.stereotypes.join(', ')}`);
      }
    } else if (item.name) {
      parts.push(
        `${item.visibility ?? ''}${item.isStatic ? ' static' : ''} ${item.name}`.trim(),
      );
    }
    if (item.annotations.length > 0) {
      // Newline separator so each annotation lands on its own row in the
      // browser's title-tooltip rendering. Falls back to ` · ` joins on
      // platforms that strip newlines from `title=` (rare).
      parts.push(item.annotations.map(formatAnnotation).join('\n'));
    }
    return parts.join('\n');
  }

  // The badge shown on the row itself — annotation first, then stereotype
  // for class-level rows that have no annotation. Returns `null` when
  // there's nothing meaningful to display.
  function gutterChip(item: GutterItem): { text: string; kind: 'anno' | 'stereo' } | null {
    if (item.annotations.length > 0) {
      const extra = item.annotations.length > 1 ? `+${item.annotations.length - 1}` : '';
      return { text: `@${item.annotations[0].name}${extra}`, kind: 'anno' };
    }
    if (item.kind === 'class' && item.stereotypes && item.stereotypes.length > 0) {
      const extra = item.stereotypes.length > 1 ? `+${item.stereotypes.length - 1}` : '';
      return { text: `⌗${item.stereotypes[0]}${extra}`, kind: 'stereo' };
    }
    return null;
  }

  // ----- Inheritance crumb ------------------------------------------------

  // Strip generic argument lists for the crumb label so we don't blow up the
  // header on `Map<String, List<User>>`. Keeps a leading qualifier when one
  // is present (`java.io.Serializable` stays readable).
  function shortTypeName(raw: string): string {
    const cut = raw.indexOf('<');
    const head = cut === -1 ? raw : raw.slice(0, cut);
    return head.trim();
  }

  // Resolve a parent-type name against the loaded class list. Handles three
  // cases in order of confidence:
  //   1. Already an FQN that matches an entry directly.
  //   2. Same-package match against the current class.
  //   3. Simple name with exactly one global match (unambiguous).
  // Returns the matching FQN or null when no resolution is confident.
  function superTypeFqn(rawName: string): string | null {
    const head = shortTypeName(rawName);
    const all = get(classes);
    if (all.some((c) => c.fqn === head)) return head;
    const simple = head.includes('.') ? head.slice(head.lastIndexOf('.') + 1) : head;
    // Same-package match wins over global single-match: a `User` referenced
    // from `com.example.UserService` should resolve to `com.example.User`
    // ahead of any other `…User` elsewhere.
    const dotIdx = klass.fqn.lastIndexOf('.');
    const myPkg = dotIdx === -1 ? '' : klass.fqn.slice(0, dotIdx);
    if (myPkg !== '') {
      const samePkg = all.find((c) => c.fqn === `${myPkg}.${simple}`);
      if (samePkg) return samePkg.fqn;
    }
    const matches = all.filter((c) => c.name === simple);
    if (matches.length === 1) return matches[0].fqn;
    return null;
  }

  function jumpToSuperType(rawName: string) {
    const fqn = superTypeFqn(rawName);
    if (!fqn) return;
    const target = get(classes).find((c) => c.fqn === fqn);
    if (target) selectedClass.set(target);
  }


  onMount(() => {
    return () => {
      if (lastFlash !== null) clearTimeout(lastFlash);
    };
  });
</script>

<div class="root" use:zoomAction style="font-size: {$zoom}em;">
  <div class="header">
    <div class="title-block">
      {#if outline && outline.super_types.length > 0}
        <nav class="crumb" aria-label={$t('outline.inheritance')}>
          {#each outline.super_types as t, i (t.kind + ':' + t.name + ':' + i)}
            {#if i > 0}
              <span class="crumb-sep" aria-hidden="true">·</span>
            {/if}
            <span class="crumb-kind {t.kind}" title={t.kind}>{t.kind === 'extends' ? '↑' : '◇'}</span>
            <button
              type="button"
              class="crumb-name"
              class:linked={superTypeFqn(t.name) !== null}
              on:click={() => jumpToSuperType(t.name)}
              title={superTypeFqn(t.name) ? superTypeFqn(t.name) : t.name}
            >{shortTypeName(t.name)}</button>
          {/each}
          <span class="crumb-arrow" aria-hidden="true">→</span>
        </nav>
      {/if}
      <h2>{klass.name}</h2>
      <p class="fqn">{klass.fqn}</p>
    </div>
    <div class="meta">
      {#each klass.stereotypes as s}
        <span class="badge {s}">{s}</span>
      {/each}
      {#if meta}
        <span class="file">{meta.file}:{meta.line_start}–{meta.line_end}</span>
      {/if}
      <button
        type="button"
        class="header-toggle"
        class:active={gutterOpen}
        on:click={toggleGutter}
        title={gutterOpen ? $t('gutter.hide') : $t('gutter.show')}
        aria-label={gutterOpen ? $t('gutter.hide') : $t('gutter.show')}
        aria-pressed={gutterOpen}
      >
        ◧
      </button>
      <button
        type="button"
        class="header-toggle"
        class:active={outlineOpen}
        on:click={toggleOutline}
        title={outlineOpen ? $t('outline.hide') : $t('outline.show')}
        aria-label={outlineOpen ? $t('outline.hide') : $t('outline.show')}
        aria-pressed={outlineOpen}
      >
        ☰
      </button>
    </div>
  </div>

  <div class="body" class:has-outline={outlineOpen}>
    <pre
      class="source"
      class:has-gutter={gutterOpen && gutterByLine.size > 0}
      bind:this={sourceEl}
    ><code>{#each lines as line, i (i)}{@const lineNo = i + 1}{@const item = gutterByLine.get(lineNo)}{@const chip = item ? gutterChip(item) : null}<span
          class="line"
          data-line-no={lineNo}
          class:highlight={highlightRanges.length === 0 &&
            lineNo >= defaultFrom &&
            lineNo <= defaultTo}
          class:wt-highlight={highlightRanges.length > 0 && inWalkthroughRange(lineNo)}
        ><span class="lineno">{lineNo}</span>{#if gutterOpen && gutterByLine.size > 0}<span
            class="gutter"
            class:has-item={item !== undefined}
            title={item ? gutterTooltip(item) : ''}
          >{#if item}{#if item.kind !== 'class' && item.visibility}<span class="vis"
                  >{visibilityGlyph(item.visibility)}{item.isStatic ? 's' : ''}</span
                >{/if}{#if chip}<span class="chip {chip.kind}">{chip.text}</span>{/if}{/if}</span
          >{/if}<span class="content">{line}</span>
</span>{/each}</code></pre>

    {#if outlineOpen}
      <aside class="outline" aria-label={$t('outline.title')}>
        {#if outline === null}
          <div class="outline-placeholder">…</div>
        {:else if outline.methods.length === 0 && outline.fields.length === 0}
          <div class="outline-placeholder">{$t('outline.empty')}</div>
        {:else}
          {#if outline.methods.length > 0}
            <div class="outline-section">
              <h3>{$t('outline.methods')} <span class="count">{outline.methods.length}</span></h3>
              <ul>
                {#each outline.methods as m (m.name + ':' + m.line_start)}
                  <li>
                    <button
                      type="button"
                      class="outline-row"
                      on:click={() => jumpToLine(m.line_start)}
                      title={[
                        `${m.visibility}${m.is_static ? ' static' : ''} ${m.name} · L${m.line_start}-${m.line_end}`,
                        ...m.annotations.map(formatAnnotation),
                      ].join('\n')}
                    >
                      <span class="vis">{visibilityGlyph(m.visibility)}</span>
                      <span class="name">{m.name}{m.is_static ? '·s' : ''}</span>
                      {#if m.annotations.length > 0}
                        <span class="anno">@{m.annotations[0].name}{m.annotations.length > 1 ? `+${m.annotations.length - 1}` : ''}</span>
                      {/if}
                      <span class="line-no">{m.line_start}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
          {#if outline.fields.length > 0}
            <div class="outline-section">
              <h3>{$t('outline.fields')} <span class="count">{outline.fields.length}</span></h3>
              <ul>
                {#each outline.fields as f (f.name + ':' + f.line)}
                  <li>
                    <button
                      type="button"
                      class="outline-row"
                      on:click={() => jumpToLine(f.line)}
                      title={[
                        `${f.visibility}${f.is_static ? ' static' : ''} ${f.name}: ${f.type} · L${f.line}`,
                        ...f.annotations.map(formatAnnotation),
                      ].join('\n')}
                    >
                      <span class="vis">{visibilityGlyph(f.visibility)}</span>
                      <span class="name">{f.name}</span>
                      {#if f.type}
                        <span class="ftype">{f.type}</span>
                      {/if}
                      <span class="line-no">{f.line}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
        {/if}
      </aside>
    {/if}
  </div>
</div>

<style>
  .root {
    padding: 16px 20px;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  .header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 12px;
    flex-shrink: 0;
  }

  .title-block {
    min-width: 0;
  }

  h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }

  .fqn {
    margin: 4px 0 0;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-2);
  }

  /* ----- Inheritance crumb -------------------------------------------- */

  /* Sits above the class name, one short line, monospaced so a long parent
     list doesn't make the header dance. Resolved (linked) parents look like
     ordinary buttons with an underline on hover; unresolved names render
     as muted text the user can still hover for the full type. */
  .crumb {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0 0 4px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
  }

  .crumb-kind {
    display: inline-block;
    width: 14px;
    text-align: center;
    color: var(--fg-2);
  }
  .crumb-kind.extends {
    color: var(--accent-2);
  }
  .crumb-kind.implements {
    color: var(--component, var(--accent-2));
  }

  .crumb-name {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--fg-2);
    font: inherit;
    cursor: default;
  }
  .crumb-name.linked {
    color: var(--fg-1);
    cursor: pointer;
  }
  .crumb-name.linked:hover {
    color: var(--accent-2);
    text-decoration: underline;
  }
  .crumb-name:focus-visible {
    outline: 1px solid var(--accent-2);
    outline-offset: 2px;
  }

  .crumb-sep {
    color: var(--fg-2);
    user-select: none;
  }

  .crumb-arrow {
    margin-left: 2px;
    color: var(--fg-2);
    user-select: none;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .file {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
  }

  .header-toggle {
    margin-left: 4px;
    padding: 3px 8px;
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-1);
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
  }
  .header-toggle:hover {
    background: var(--bg-3);
  }
  .header-toggle.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }

  /* Source pane spans the full body until the outline is opened, at which
     point a fixed-width column is reserved on the right. */
  .body {
    display: grid;
    grid-template-columns: 1fr;
    gap: 12px;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .body.has-outline {
    grid-template-columns: minmax(0, 1fr) 240px;
  }

  .source {
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: var(--radius-md);
    padding: 12px 0;
    font-family: var(--mono);
    /* `em`, not `px`, so the .root `font-size: {zoom}em` actually scales
       the source code on shift+wheel. 0.78em ≈ 12.5px at the 16px base. */
    font-size: 0.78em;
    line-height: 1.55;
    margin: 0;
    overflow: auto;
    min-height: 0;
  }

  .line {
    display: block;
    padding: 0 12px;
    scroll-margin-top: 12px;
  }

  .line.flash {
    background: color-mix(in srgb, var(--accent-2) 35%, transparent);
    transition: background 1s ease;
  }

  .line.highlight {
    background: color-mix(in srgb, var(--accent-2) 18%, transparent);
    border-left: 3px solid var(--accent-2);
    padding-left: 9px;
  }

  .line.wt-highlight {
    background: color-mix(in srgb, var(--warn) 30%, transparent);
    border-left: 3px solid var(--warn);
    padding-left: 9px;
  }

  .lineno {
    display: inline-block;
    width: 36px;
    color: var(--fg-2);
    text-align: right;
    margin-right: 12px;
    user-select: none;
  }

  /* ----- Annotated gutter --------------------------------------------- */

  /* The gutter is an inline-block column that sits between `lineno` and
     `content`. We reserve a fixed width even on lines without an item so
     the source code doesn't reflow as the user scrolls past member
     declarations. Width chosen to fit the most common annotation chips
     (`@Service`, `@Override`, `@Autowired`) without truncating. */
  .gutter {
    display: inline-block;
    width: 138px;
    margin-right: 8px;
    vertical-align: baseline;
    user-select: none;
    font-family: var(--mono);
    color: var(--fg-2);
    /* We render the chip + glyph as plain inline content so the existing
       `<pre>` whitespace handling still works for `.content`. */
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .gutter .vis {
    display: inline-block;
    width: 16px;
    text-align: center;
    color: var(--fg-2);
  }

  .gutter .chip {
    display: inline-block;
    padding: 0 6px;
    border-radius: 8px;
    font-size: 0.85em;
    line-height: 1.3;
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-2));
    color: var(--fg-0);
    border: 1px solid color-mix(in srgb, var(--accent-2) 35%, transparent);
  }

  .gutter .chip.stereo {
    background: color-mix(in srgb, var(--component, var(--accent-2)) 22%, var(--bg-2));
    border-color: color-mix(in srgb, var(--component, var(--accent-2)) 40%, transparent);
  }

  /* When no item lives on a line we still reserve the column so the code
     stays aligned, but we render nothing — keeps the gutter quiet. */
  .gutter:not(.has-item) {
    visibility: hidden;
  }

  .content {
    white-space: pre;
  }

  /* ----- Outline pane -------------------------------------------------- */

  .outline {
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: var(--radius-md);
    overflow: auto;
    padding: 8px 6px;
    font-size: 0.78em;
    min-height: 0;
  }

  .outline-section + .outline-section {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--bg-3);
  }

  .outline h3 {
    margin: 0 6px 6px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .outline h3 .count {
    font-family: var(--mono);
    font-weight: 400;
    color: var(--fg-2);
  }

  .outline ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .outline-row {
    width: 100%;
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto auto;
    align-items: baseline;
    gap: 6px;
    padding: 4px 6px;
    background: transparent;
    border: 0;
    border-radius: 3px;
    color: inherit;
    text-align: left;
    font: inherit;
    font-family: var(--mono);
    cursor: pointer;
  }
  .outline-row:hover {
    background: var(--bg-2);
  }
  .outline-row:focus-visible {
    outline: 2px solid var(--accent-2);
    outline-offset: -2px;
  }

  .outline-row .vis {
    color: var(--fg-2);
    text-align: center;
  }
  .outline-row .name {
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .outline-row .anno {
    font-size: 10px;
    color: var(--accent-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
  }
  .outline-row .ftype {
    font-size: 10px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
  }
  .outline-row .line-no {
    color: var(--fg-2);
    font-size: 10px;
  }

  .outline-placeholder {
    padding: 12px;
    color: var(--fg-2);
    font-style: italic;
    text-align: center;
  }
</style>
