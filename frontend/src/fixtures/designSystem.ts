/**
 * FIXTURE LAYER — design-system showcase only.
 *
 * Synthetic values used to exercise components with realistic density.
 * They are never imported by production routes (enforced by a unit test)
 * and every screen that renders them wraps them in <FixtureFrame>.
 * Identifiers are deliberately non-addresses so they cannot be mistaken
 * for real wallets.
 */

export interface FixtureWalletRow {
  id: string;
  label: string;
  score: number;
  /** Realized P&L in lamports (string: u64-safe). Sign kept separately. */
  realizedPnlLamports: string;
  pnlNegative: boolean;
  winRateBps: number;
  drawdownBps: number;
  trades: number;
  lastActiveSecondsAgo: number;
  state: 'TRACKED' | 'PAUSED' | 'REJECTED';
}

export const fixtureWallets: FixtureWalletRow[] = [
  {
    id: 'FX-W-0001',
    label: 'fixture-alpha',
    score: 87,
    realizedPnlLamports: '412500000000',
    pnlNegative: false,
    winRateBps: 6120,
    drawdownBps: 1840,
    trades: 214,
    lastActiveSecondsAgo: 42,
    state: 'TRACKED',
  },
  {
    id: 'FX-W-0002',
    label: 'fixture-bravo',
    score: 74,
    realizedPnlLamports: '98250000000',
    pnlNegative: false,
    winRateBps: 5530,
    drawdownBps: 2610,
    trades: 1093,
    lastActiveSecondsAgo: 380,
    state: 'TRACKED',
  },
  {
    id: 'FX-W-0003',
    label: 'fixture-charlie',
    score: 61,
    realizedPnlLamports: '15000000000',
    pnlNegative: true,
    winRateBps: 4410,
    drawdownBps: 3975,
    trades: 58,
    lastActiveSecondsAgo: 5_400,
    state: 'PAUSED',
  },
  {
    id: 'FX-W-0004',
    label: 'fixture-delta',
    score: 23,
    realizedPnlLamports: '1203000000000',
    pnlNegative: true,
    winRateBps: 2890,
    drawdownBps: 7120,
    trades: 7,
    lastActiveSecondsAgo: 172_800,
    state: 'REJECTED',
  },
];

export const fixtureTokenAmounts = [
  { raw: '18446744073709551615', decimals: 6, note: 'u64::MAX, 6 decimals' },
  { raw: '1000000001', decimals: 9, note: '1.000000001 SOL in lamports' },
  { raw: '5', decimals: 9, note: 'dust' },
];
