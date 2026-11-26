import { describe, expect, it } from 'vitest';
import { guessDeviceName } from '../src/lib/api';
import { uuidv4 } from '../src/lib/uuid';

describe('utilitaires', () => {
  it('uuid v4 valide et unique', () => {
    const a = uuidv4();
    expect(a).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(uuidv4()).not.toBe(a);
  });

  it('nom d’appareil', () => {
    expect(guessDeviceName('Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)')).toBe('iPhone');
    expect(guessDeviceName('Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP1A) Chrome')).toBe('Pixel 8');
    expect(guessDeviceName('Mozilla/5.0 (Windows NT 10.0)')).toBe('Navigateur');
  });
});
