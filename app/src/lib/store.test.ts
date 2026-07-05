import { describe, expect, it } from 'vitest';
import { dedupeModuleFiles } from './store';
import type { ModuleFile } from './api';

function file(abs: string, rel: string): ModuleFile {
  return { abs, rel, kind: 'md', size: 1 };
}

describe('dedupeModuleFiles', () => {
  it('drops files an aggregator module reports on top of its children (#171)', () => {
    // A Maven parent module's directory physically contains the child
    // modules, so `list_module_files` reports e.g. INFO.md twice: once via
    // the child (rel = "INFO.md") and once via the parent
    // (rel = "plaintext-admin-cron/INFO.md"). Same absolute path.
    const byModule = {
      'plaintext-admin-cron': [file('/repo/plaintext-admin-cron/INFO.md', 'INFO.md')],
      'ch.plaintext:plaintext-root-parent': [
        file('/repo/plaintext-admin-cron/INFO.md', 'plaintext-admin-cron/INFO.md'),
        file('/repo/README.md', 'README.md'),
      ],
    };
    const out = dedupeModuleFiles(byModule);
    expect(out.map((f) => f.abs)).toEqual([
      '/repo/plaintext-admin-cron/INFO.md',
      '/repo/README.md',
    ]);
  });

  it('keeps the first occurrence of a duplicated path', () => {
    const byModule = {
      a: [file('/repo/x/DOC.md', 'DOC.md')],
      b: [file('/repo/x/DOC.md', 'x/DOC.md')],
    };
    const out = dedupeModuleFiles(byModule);
    expect(out).toHaveLength(1);
    expect(out[0].rel).toBe('DOC.md');
  });

  it('preserves distinct files and their order', () => {
    const byModule = {
      a: [file('/repo/a/1.md', '1.md'), file('/repo/a/2.md', '2.md')],
      b: [file('/repo/b/3.md', '3.md')],
    };
    expect(dedupeModuleFiles(byModule).map((f) => f.abs)).toEqual([
      '/repo/a/1.md',
      '/repo/a/2.md',
      '/repo/b/3.md',
    ]);
  });

  it('returns an empty list for an empty map', () => {
    expect(dedupeModuleFiles({})).toEqual([]);
  });
});
