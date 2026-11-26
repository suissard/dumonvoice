// Test de bout en bout du daemon DumonVoice : local + LAN HTTPS + WebSocket.
// Usage : NODE_EXTRA_CA_CERTS=<home>/certs/ca.crt node tests/e2e/e2e.mjs <DUMONVOICE_HOME> <parole.wav>
// (daemon lancé avec DUMONVOICE_HOME=<home>, moteur Whisper prêt ; voir README).
// Note : fetch interdit de forcer l'en-tête Host ; la protection anti DNS-rebinding est
// couverte par les tests unitaires (rest::tests::host_check).
import { readFileSync } from 'node:fs';
const [home, wavPath] = process.argv.slice(2);
const LOCAL = 'http://127.0.0.1:18450';
const tok = new URL(JSON.parse(readFileSync(`${home}/session.json`, 'utf8')).url).searchParams.get('auth');
let fails = 0;
const ok = (c, m) => { console.log(`${c ? '✔' : '✘'} ${m}`); if (!c) fails++; };
const uuid = () => crypto.randomUUID();

// 1. Coquille PWA publique, API protégée
ok((await fetch(LOCAL + '/')).status === 200, 'PWA servie sur le listener local');
ok((await fetch(LOCAL + '/api/status')).status === 401, 'API locale refusée sans jeton');
const r302 = await fetch(`${LOCAL}/?auth=${tok}&view=mobile`, { redirect: 'manual' });
ok(r302.status === 302 && /HttpOnly/.test(r302.headers.get('set-cookie')) && r302.headers.get('location') === '/#mobile', 'lien du tray → cookie HttpOnly + redirection');
const L = { Authorization: 'Bearer ' + tok, 'Content-Type': 'application/json' };
const st = await (await fetch(LOCAL + '/api/status', { headers: L })).json();
ok(st.state === 'IDLE' && st.vad === 'silero' && st.engine.whisper.available, `statut : ${st.state}, VAD ${st.vad}, Whisper prêt`);
ok(st.mobile.running && st.mobile.lan_url, `serveur mobile actif : ${st.mobile.lan_url}`);

// 2. Configuration : patch validé
let r = await fetch(LOCAL + '/api/config', { method: 'PATCH', headers: L, body: JSON.stringify({ triggers: { hotkey: { keys: ['ControlLeft', 'KeyC'] } } }) });
ok(r.status === 400 && (await r.json()).message.includes('Ctrl+C'), 'raccourci en conflit (Ctrl+C) refusé');
r = await fetch(LOCAL + '/api/config', { method: 'PATCH', headers: L, body: JSON.stringify({ ui: { keep_history_count: 10 } }) });
ok(r.status === 200 && !JSON.stringify(await r.json()).includes('token_hash'), 'patch accepté, aucune empreinte de jeton exposée');

// 3. Appairage QR via HTTPS (certificat validé avec la CA locale)
const pc = await (await fetch(LOCAL + '/api/pairing/create', { method: 'POST', headers: L })).json();
ok(pc.qr_svg.startsWith('<?xml') || pc.qr_svg.includes('<svg'), 'QR code SVG généré');
const LAN = new URL(pc.url).origin;
ok((await fetch(LAN + '/api/pairing/create', { method: 'POST' })).status === 403, 'création de code interdite depuis le LAN');
const ex = await (await fetch(LAN + '/api/pairing/exchange', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ pair_code: pc.pair_code, device_name: 'Pixel test' }) })).json();
ok(!!ex.device_token, `appareil appairé : ${ex.device_id}`);
const reuse = await fetch(LAN + '/api/pairing/exchange', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ pair_code: pc.pair_code, device_name: 'x' }) });
ok(reuse.status === 401, 'code d’appairage à usage unique');
const D = { Authorization: 'Bearer ' + ex.device_token, 'Content-Type': 'application/json' };
ok((await fetch(LAN + '/api/status')).status === 401, 'API LAN refusée sans jeton d’appareil');

// 4. Texte direct REST + idempotence
const mid = uuid();
const body = JSON.stringify({ message_id: mid, text: 'Salut 👋 depuis le téléphone\nligne 2', press_enter_after: true, refine_with_ollama: false });
const t0 = Date.now();
const a = await (await fetch(LAN + '/api/remote/inject-text', { method: 'POST', headers: D, body })).json();
const lat = Date.now() - t0;
const b = await (await fetch(LAN + '/api/remote/inject-text', { method: 'POST', headers: D, body })).json();
ok(a.type === 'injection_result' && a.injected && a.id === mid, `texte direct injecté (aller-retour HTTPS ${lat} ms, interne ${a.latency_ms} ms)`);
ok(b.id === mid, 'renvoi du même message_id : résultat en cache (idempotence)');
const h = await (await fetch(LAN + '/api/history', { headers: D })).json();
ok(h.filter((x) => x.id === mid).length === 1, 'injecté une seule fois (historique)');
r = await fetch(LAN + '/api/remote/inject-text', { method: 'POST', headers: D, body: JSON.stringify({ message_id: uuid(), text: 'x'.repeat(20001) }) });
ok(r.status === 413, 'texte trop long → 413 TEXT_TOO_LARGE');

// 5. WebSocket mobile : auth, ping, dictée audio streamée, BUSY
const ws = new WebSocket(LAN.replace('https', 'wss') + '/ws');
ws.binaryType = 'arraybuffer';
const events = [];
const waiters = [];
ws.onmessage = (m) => { const e = JSON.parse(m.data); events.push(e); waiters.forEach((w) => w(e)); };
const waitFor = (pred, ms = 15000) => new Promise((res, rej) => {
  const hit = events.find(pred); if (hit) return res(hit);
  const t = setTimeout(() => rej(new Error('timeout')), ms);
  waiters.push((e) => { if (pred(e)) { clearTimeout(t); res(e); } });
});
await new Promise((res) => (ws.onopen = res));
ws.send(JSON.stringify({ type: 'auth', token: ex.device_token, client_kind: 'mobile' }));
const auth = await waitFor((e) => e.type === 'auth_ok');
ok(auth.device_id === ex.device_id, `WS authentifié (PC : ${auth.pc_name})`);
ws.send(JSON.stringify({ type: 'ping', ts: 42 }));
ok((await waitFor((e) => e.type === 'pong')).ts === 42, 'heartbeat ping/pong');

// Lecture WAV → 16 kHz mono S16LE (rééchantillonnage linéaire simple)
const wav = readFileSync(wavPath);
const rate = wav.readUInt32LE(24);
let off = 12; while (wav.toString('ascii', off, off + 4) !== 'data') off += 8 + wav.readUInt32LE(off + 4);
const n = wav.readUInt32LE(off + 4) / 2, src = new Int16Array(n);
for (let i = 0; i < n; i++) src[i] = wav.readInt16LE(off + 8 + i * 2);
const outN = Math.floor((n * 16000) / rate), pcm = new Int16Array(outN);
for (let i = 0; i < outN; i++) { const t = (i * rate) / 16000, k = Math.floor(t), f = t - k; pcm[i] = src[k] + ((src[Math.min(k + 1, n - 1)] - src[k]) * f); }

const sid = uuid();
ws.send(JSON.stringify({ type: 'session_start', session_id: sid, audio_format: { encoding: 'pcm_s16le', sample_rate: 16000, channels: 1 }, trigger_mode: 'push_to_talk', auto_stop_vad: false }));
await waitFor((e) => e.type === 'state_changed' && e.state === 'LISTENING' && e.session_id === sid);
ok(true, 'session audio mobile → LISTENING');
// Conflit : texte direct pendant la dictée → BUSY
const busyId = uuid();
ws.send(JSON.stringify({ type: 'inject_text', message_id: busyId, text: 'conflit', press_enter_after: false, refine_with_ollama: false }));
ok((await waitFor((e) => e.type === 'error' && e.id === busyId)).code === 'BUSY', 'source concurrente rejetée avec BUSY');
for (let i = 0; i < pcm.length; i += 640) {
  ws.send(pcm.slice(i, i + 640).buffer);
  await new Promise((r) => setTimeout(r, 5));
}
const tStop = Date.now();
ws.send(JSON.stringify({ type: 'session_stop', session_id: sid }));
const res = await waitFor((e) => e.type === 'injection_result' && e.id === sid, 30000);
ok(res.injected && res.source === 'remote_audio' && /bonjour/i.test(res.text), `dictée mobile transcrite et injectée en ${Date.now() - tStop} ms : « ${res.text} »`);

// 6. Watchdog : flux interrompu sans session_stop
const sid2 = uuid();
ws.send(JSON.stringify({ type: 'session_start', session_id: sid2, auto_stop_vad: false }));
await waitFor((e) => e.type === 'state_changed' && e.state === 'LISTENING' && e.session_id === sid2);
const t2 = Date.now();
await waitFor((e) => e.type === 'state_changed' && e.state === 'IDLE' && Date.now() - t2 > 1500, 6000);
ok(true, `watchdog : retour à IDLE ${Date.now() - t2} ms après la fin du flux`);

// 7. Révocation : le WebSocket est fermé
const closed = new Promise((res) => (ws.onclose = res));
await fetch(LOCAL + `/api/pairing/devices/${ex.device_id}`, { method: 'DELETE', headers: L });
await Promise.race([closed, new Promise((_, rej) => setTimeout(() => rej(new Error('pas fermé')), 5000))]);
ok((await fetch(LAN + '/api/status', { headers: D })).status === 401, 'appareil révoqué : WebSocket fermé et jeton refusé');

console.log(fails ? `\n${fails} échec(s)` : '\nTous les contrôles de bout en bout sont passés');
process.exit(fails ? 1 : 0);
