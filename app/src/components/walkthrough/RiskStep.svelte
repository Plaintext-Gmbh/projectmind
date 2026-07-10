<script lang="ts">
  // Walkthrough 2.0 `risk` step (Cockpit 2.4, #160).
  //
  // Renders the class viewer topped by a risk-score header bar. The bar's
  // width + tier colour reflect the composite risk score; the badge row
  // surfaces the signals the step asked for (`show:[churn,cx,cov]`, or every
  // signal with data when `show` is empty). Kept narrow: no coupling to the
  // pattern/atlas renderers — just class source + one atlas lookup.
  import { onMount } from 'svelte';
  import ClassViewer from '../ClassViewer.svelte';
  import { showClass, riskScoreFor, type ClassEntry, type RiskScore } from '../../lib/api';
  import { riskBadges, riskTier, type RiskBadge } from '../../lib/riskBadges';

  export let fqn: string;
  export let focus: string | null = null;
  export let show: string[] = [];

  let klass: ClassEntry | null = null;
  let source = '';
  let meta: { file: string; line_start: number; line_end: number } | null = null;
  let score: RiskScore | null = null;
  let badges: RiskBadge[] = [];
  let loading = true;
  let error: string | null = null;

  async function load() {
    loading = true;
    error = null;
    try {
      const [cls, sc] = await Promise.all([showClass(fqn), riskScoreFor(fqn)]);
      klass = {
        fqn,
        name: fqn.split('.').pop() ?? fqn,
        file: cls.file,
        stereotypes: [],
        kind: '',
        module: sc?.module ?? '',
      };
      source = cls.source;
      meta = { file: cls.file, line_start: cls.line_start, line_end: cls.line_end };
      score = sc;
      badges = sc ? riskBadges(sc, show) : [];
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  $: tier = score ? riskTier(score.score) : 'cool';
  $: barWidth = score ? Math.max(2, Math.min(100, Math.round(score.score))) : 0;
</script>

<div class="risk-step">
  {#if loading}
    <div class="loading">…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if klass && meta}
    <header class="risk-head" class:hot={tier === 'hot'} class:warm={tier === 'warm'} class:cool={tier === 'cool'}>
      <div class="risk-title">
        <span class="risk-label">Risk</span>
        <span class="risk-fqn" title={fqn}>{klass.name}</span>
        {#if focus}<span class="risk-focus" title="focus">· {focus}</span>{/if}
      </div>
      <div class="risk-meter" role="img" aria-label={`risk score ${score ? Math.round(score.score) : 0} of 100`}>
        <div class="risk-fill" style={`width:${barWidth}%`}></div>
        <span class="risk-score">{score ? Math.round(score.score) : '—'}</span>
      </div>
      {#if badges.length > 0}
        <div class="risk-badges">
          {#each badges as b (b.id)}
            <span class="risk-badge">{b.label}&nbsp;{b.value}</span>
          {/each}
        </div>
      {:else if score}
        <div class="risk-badges muted">no signals with data</div>
      {/if}
    </header>
    <div class="risk-body">
      <ClassViewer {klass} {source} {meta} highlightRanges={[]} />
    </div>
  {/if}
</div>

<style>
  .risk-step {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .risk-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border, #2a2a2a);
    background: var(--panel, #1b1b1b);
  }
  .risk-head.hot {
    border-left: 4px solid #e05252;
  }
  .risk-head.warm {
    border-left: 4px solid #d8a13a;
  }
  .risk-head.cool {
    border-left: 4px solid #4a8f5a;
  }
  .risk-title {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    font-size: 0.95rem;
  }
  .risk-label {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    opacity: 0.7;
  }
  .risk-fqn {
    font-weight: 600;
  }
  .risk-focus {
    opacity: 0.7;
    font-family: var(--mono, monospace);
    font-size: 0.85rem;
  }
  .risk-meter {
    position: relative;
    flex: 1 1 120px;
    min-width: 120px;
    height: 1.1rem;
    border-radius: 0.55rem;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }
  .risk-fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: linear-gradient(90deg, #4a8f5a, #d8a13a, #e05252);
  }
  .risk-score {
    position: absolute;
    right: 0.4rem;
    top: 50%;
    transform: translateY(-50%);
    font-size: 0.72rem;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 0 3px rgba(0, 0, 0, 0.8);
  }
  .risk-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .risk-badges.muted {
    opacity: 0.6;
    font-size: 0.8rem;
  }
  .risk-badge {
    font-size: 0.75rem;
    padding: 0.1rem 0.4rem;
    border-radius: 0.3rem;
    background: rgba(255, 255, 255, 0.08);
    white-space: nowrap;
  }
  .risk-body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }
  .loading,
  .error {
    padding: 1rem;
  }
</style>
