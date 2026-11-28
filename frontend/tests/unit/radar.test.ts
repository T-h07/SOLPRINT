import { describe, expect, it } from 'vitest';

import {
  formatComponent,
  formatScore,
  lifecycleLabel,
  radarCoverageLabel,
  radarName,
} from '@/features/radar/labels.ts';
import {
  isRadarCycles,
  isRadarEvidence,
  isRadarHistory,
  isRadarJobs,
  isRadarMarkets,
  isRadarMonitoring,
  isRadarSources,
  isRadarStatus,
  isRadarWalletDetail,
  isRadarWallets,
} from '@/lib/api/guards.ts';

import {
  radarCycles,
  radarEvidence,
  radarHistory,
  radarJobs,
  radarMarkets,
  radarMonitoring,
  radarSources,
  radarStatus,
  radarWallet,
  radarWallets,
  TRADER,
} from '../support/radarFixtures.ts';

describe('radar formatting', () => {
  it('truncates priority scores toward zero and never invents one', () => {
    expect(formatScore('71.3824')).toBe('71.3');
    expect(formatScore('64.0999')).toBe('64.0');
    expect(formatScore('100')).toBe('100.0');
    expect(formatScore('0.0000')).toBe('0.0');
    expect(formatScore(null)).toBe('—');
    expect(formatScore('-1')).toBe('—');
    expect(formatScore('abc')).toBe('—');
  });

  it('shows 0..1 components on the 0–100 scale without rounding up', () => {
    expect(formatComponent('0.8129')).toBe('81.2');
    expect(formatComponent('0.05')).toBe('5.0');
    expect(formatComponent('1')).toBe('100.0');
    expect(formatComponent('0')).toBe('0.0');
    expect(formatComponent('1.5x')).toBe('—');
  });

  it('labels states as text and leaves unknown values neutral and verbatim', () => {
    expect(lifecycleLabel('MONITORED')).toEqual({ tone: 'positive', label: 'Monitored' });
    expect(radarCoverageLabel('PARTIAL_HISTORY').tone).toBe('warning');
    expect(lifecycleLabel('NEW_STATE')).toEqual({ tone: 'neutral', label: 'new state' });
    expect(radarName('JUPITER_TOP_TRADED')).toBe('Jupiter · top traded');
  });
});

describe('radar guards', () => {
  it('accept every fixture shape', () => {
    for (const v of ['active', 'empty', 'paused'] as const) {
      expect(isRadarStatus(radarStatus(v))).toBe(true);
      expect(isRadarWallets(radarWallets(v, null))).toBe(true);
      expect(isRadarMarkets(radarMarkets(v))).toBe(true);
      expect(isRadarSources(radarSources(v))).toBe(true);
      expect(isRadarJobs(radarJobs(v))).toBe(true);
      expect(isRadarMonitoring(radarMonitoring(v))).toBe(true);
      expect(isRadarCycles(radarCycles(v))).toBe(true);
    }
    expect(isRadarWalletDetail(radarWallet(TRADER))).toBe(true);
    expect(isRadarEvidence(radarEvidence(TRADER))).toBe(true);
    expect(isRadarHistory(radarHistory(TRADER))).toBe(true);
  });

  it('reject malformed bodies', () => {
    for (const g of [isRadarStatus, isRadarWallets, isRadarWalletDetail, isRadarJobs]) {
      expect(g(null)).toBe(false);
      expect(g({})).toBe(false);
      expect(g('ok')).toBe(false);
    }
  });
});
