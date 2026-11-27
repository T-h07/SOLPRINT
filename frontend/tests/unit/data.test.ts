import { describe, expect, it } from 'vitest';

import {
  attributionLabel,
  decodeLabel,
  finalityLabel,
  protocolName,
} from '@/features/data/labels.ts';
import {
  isDataDecoders,
  isDataStatus,
  isDataSwaps,
  isDataTransactionDetail,
} from '@/lib/api/guards.ts';
import { formatSignedBaseUnits, formatTokenAmount, MINUS, shortId } from '@/lib/format/number.ts';

import {
  dataDecoders,
  dataStatus,
  dataSwaps,
  dataTransaction,
  dataTransactions,
} from '../support/dataFixtures.ts';

describe('data engine formatting', () => {
  it('formats raw token amounts exactly with BigInt and never rounds up', () => {
    expect(formatTokenAmount('17000000000000', 6)).toBe('17,000,000.0000');
    expect(formatTokenAmount('18446744073709551615', 9)).toBe('18,446,744,073.7095');
    expect(formatTokenAmount('999999', 6)).toBe('0.9999');
    expect(formatTokenAmount('5', 0)).toBe('5');
  });

  it('never invents a scale or a zero for unknown values', () => {
    expect(formatTokenAmount(null, 6)).toBe('—');
    expect(formatTokenAmount('123456', null)).toBe('123,456 raw');
    expect(formatTokenAmount('1.5', 6)).toBe('—');
  });

  it('signs deltas explicitly so direction survives without colour', () => {
    expect(formatSignedBaseUnits('-502105000', 9, 9)).toBe(`${MINUS}0.502105000`);
    expect(formatSignedBaseUnits('17000000000000', 6)).toBe('+17,000,000.0000');
    expect(formatSignedBaseUnits('0', 9, 2)).toBe('0.00');
    expect(formatSignedBaseUnits('--1', 9)).toBe('—');
  });

  it('shortens identifiers symmetrically', () => {
    expect(shortId('So11111111111111111111111111111111111111112')).toBe('So11…1112');
    expect(shortId('abc')).toBe('abc');
  });
});

describe('data engine labels', () => {
  it('maps every known state to text, and unknown states verbatim', () => {
    expect(finalityLabel('FINALIZED')).toEqual({ tone: 'positive', label: 'Finalized' });
    expect(finalityLabel('POSSIBLY_ORPHANED').tone).toBe('warning');
    expect(decodeLabel('INSUFFICIENT_EVIDENCE').label).toBe('Insufficient evidence');
    expect(attributionLabel('PROGRAM_CONTROLLED').label).toBe('Program');
    // A future backend value is shown as-is, never coerced into a known state.
    expect(decodeLabel('SOMETHING_NEW')).toEqual({ tone: 'neutral', label: 'something new' });
    expect(protocolName('RAYDIUM_CLMM')).toBe('Raydium CLMM');
    expect(protocolName('NEW_DEX')).toBe('NEW_DEX');
  });
});

describe('data engine contract guards', () => {
  it('accept the synthetic fixtures', () => {
    expect(isDataStatus(dataStatus('active'))).toBe(true);
    expect(isDataStatus(dataStatus('partial'))).toBe(true);
    expect(isDataSwaps(dataSwaps('active', null))).toBe(true);
    const sig = dataTransactions('active').items[0]?.signature ?? '';
    expect(isDataTransactionDetail(dataTransaction(sig))).toBe(true);
  });

  it('reject numeric amounts (u64 must travel as strings)', () => {
    const body = dataSwaps('active', null);
    const first = body.items[0];
    if (!first) throw new Error('fixture has swaps');
    const broken = {
      ...body,
      items: [{ ...first, input: { ...first.input, amount: 500000000 } }],
    };
    expect(isDataSwaps(broken)).toBe(false);
  });

  it('accept the decoder registry and reject a malformed entry', () => {
    expect(isDataDecoders(dataDecoders())).toBe(true);
    const body = dataDecoders();
    expect(isDataDecoders({ ...body, decoders: [{ id: 'x', protocol: 'X' }] })).toBe(false);
  });

  it('reject a status that claims execution capability', () => {
    expect(isDataStatus({ ...dataStatus('active'), execution_capability: 'LIVE' })).toBe(false);
    expect(isDataStatus(null)).toBe(false);
  });
});
