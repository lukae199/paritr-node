(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  let secret = sessionStorage.getItem('paritrAdminSecret') || '';
  const directSecret = new URLSearchParams(location.hash.slice(1)).get('secret');
  if (directSecret && /^[0-9a-f]{48,}$/i.test(directSecret)) {
    secret = directSecret;
    sessionStorage.setItem('paritrAdminSecret', secret);
    history.replaceState(null, '', location.pathname + location.search);
  }
  let config = null;
  let editing = false;
  let loading = false;
  let restartingUntil = 0;

  async function api(path, options = {}) {
    const response = await fetch(path, {
      ...options,
      cache: 'no-store',
      signal: AbortSignal.timeout(15000),
      headers: { 'Authorization': `Bearer ${secret}`, 'Content-Type': 'application/json', ...(options.headers || {}) },
    });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`);
    return data;
  }

  function toast(message) {
    $('toast').textContent = message;
    $('toast').classList.add('show');
    setTimeout(() => $('toast').classList.remove('show'), 3500);
  }

  function formatRate(value) {
    const units = ['H/s', 'kH/s', 'MH/s', 'GH/s']; let n = Number(value || 0); let i = 0;
    while (n >= 1000 && i < units.length - 1) { n /= 1000; i += 1; }
    return `${n.toLocaleString('de-DE', { maximumFractionDigits: 2 })} ${units[i]}`;
  }

  function formatDuration(seconds) {
    const value = Number(seconds || 0); const days = Math.floor(value / 86400); const hours = Math.floor(value % 86400 / 3600);
    return days ? `${days} T ${hours} Std` : `${hours} Std ${Math.floor(value % 3600 / 60)} Min`;
  }

  async function load() {
    if (Date.now() < restartingUntil) return;
    const [status, current] = await Promise.all([api('/admin/status'), api('/admin/config')]);
    config = current;
    $('online').classList.remove('busy');
    const running = status.node_enabled !== false;
    $('online').textContent = `${running ? 'Online' : 'Gestoppt'} · v${status.node_version}`;
    $('online').classList.toggle('ok', running);
    $('start').disabled = running;
    $('stop').disabled = !running;
    $('identity').textContent = `${current.device_name}.local · ${current.device_id.slice(0, 8)}`;
    $('height').textContent = Number(status.height).toLocaleString('de-DE');
    $('peers').textContent = Number(status.peer_count).toLocaleString('de-DE');
    $('hashrate').textContent = formatRate(status.hashrate);
    $('uptime').textContent = formatDuration(status.uptime_seconds);
    $('platform').textContent = status.platform || '—';
    $('cpuUse').textContent = `${Number(status.mining_processes || 0)} / ${Number(status.cpu_total || 0)} Threads`;
    $('miningNotice').hidden = Boolean(current.miner_address);
    $('pairState').textContent = current.portal_paired ? 'Mit dem Wallet-Portal gekoppelt' : 'Noch nicht gekoppelt';
    $('unpair').disabled = !current.portal_paired;
    // Background status refresh must not overwrite unsaved form edits.
    if (editing) return;
    $('miningEnabled').checked = Boolean(current.mining_enabled);
    $('minerAddress').value = current.miner_address || '';
    $('threads').max = Math.max(1, Number(status.cpu_total || 1)); $('threads').value = current.mining_threads || 0;
    $('threadsOut').textContent = Number($('threads').value) === 0 ? 'Automatisch' : $('threads').value;
    $('intensity').value = current.mining_intensity; $('intensityOut').textContent = `${current.mining_intensity} %`;
    const mode = document.querySelector(`input[name=mode][value="${current.randomx_mode}"]`); if (mode) mode.checked = true;
    const fast = document.querySelector('input[name=mode][value=fast]'); fast.disabled = !status.randomx_fast_available;
    $('fastChoice').classList.toggle('unavailable', !status.randomx_fast_available);
    if (!status.randomx_fast_available) $('fastChoice').querySelector('small').textContent = 'Auf diesem System ist nur Light verfügbar';
    $('deviceName').value = current.device_name; $('publicUrl').value = current.public_url || '';
    $('localUrl').textContent = `http://${current.device_name}.local:${location.port || 5051}`;
    $('portalUrl').value = current.portal_url || 'https://paritr.highactive.de';
    $('pairState').textContent = current.portal_paired ? 'Mit dem Wallet-Portal gekoppelt' : 'Noch nicht gekoppelt';
    $('unpair').disabled = !current.portal_paired;
  }

  async function connect() {
    try {
      await api('/admin/auth');
      $('loginError').textContent = '';
      $('login').close();
      await load(); await loadLogs();
    } catch (error) {
      sessionStorage.removeItem('paritrAdminSecret');
      $('loginError').textContent = 'Secret ungültig oder Node nicht erreichbar.';
      if (!$('login').open) $('login').showModal();
    }
  }

  async function action(path, body, message) {
    if (path === '/admin/mining' && body.mining_enabled && !body.miner_address) {
      toast('Bitte zuerst eine Reward-Adresse hinterlegen. Ohne Adresse startet das Mining nicht.');
      $('minerAddress').focus(); return;
    }
    const button = document.activeElement;
    if (button?.tagName === 'BUTTON') { button.disabled = true; button.classList.add('busy'); }
    try {
      const result = await api(path, { method: 'POST', body: JSON.stringify(body || {}) });
      if (['/admin/mining', '/admin/config', '/admin/pair', '/admin/unpair'].includes(path)) {
        editing = false;
        $('pairCode').value = '';
      }
      if (result.restart_scheduled || path === '/admin/restart') {
        restartingUntil = Date.now() + 4500;
        $('online').textContent = 'Neustart läuft …';
        $('online').classList.add('busy');
        toast(message);
      } else {
        toast(result.restart_scheduled === false ? 'Einstellungen übernommen.' : message);
        await load();
      }
    }
    catch (error) { toast(error.message); }
    finally { if (button?.tagName === 'BUTTON') { button.disabled = false; button.classList.remove('busy'); } }
  }

  async function loadLogs() {
    $('refreshLogs').classList.add('busy');
    try { const data = await api('/admin/logs?limit=250'); $('logs').textContent = data.lines.join('\n') || 'Noch keine Logdaten.'; $('logs').scrollTop = $('logs').scrollHeight; }
    catch (error) { $('logs').textContent = error.message; }
    finally { $('refreshLogs').classList.remove('busy'); }
  }

  async function checkUpdate() {
    try {
      const data = await api('/admin/update');
      $('updateCommand').textContent = data.install_command;
      $('updateState').textContent = data.update_available
        ? `Version ${data.latest} ist verfügbar (installiert: ${data.current}).`
        : `Version ${data.current} ist aktuell.`;
    } catch (error) { $('updateState').textContent = error.message; }
  }

  document.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('.grid input').forEach((input) => input.addEventListener('input', () => { editing = true; }));
    $('login').addEventListener('cancel', (event) => event.preventDefault());
    $('loginButton').addEventListener('click', (event) => { event.preventDefault(); secret = $('secret').value.trim(); sessionStorage.setItem('paritrAdminSecret', secret); connect(); });
    $('threads').addEventListener('input', () => { $('threadsOut').textContent = Number($('threads').value) === 0 ? 'Automatisch' : $('threads').value; });
    $('intensity').addEventListener('input', () => { $('intensityOut').textContent = `${$('intensity').value} %`; });
    $('saveMining').addEventListener('click', () => action('/admin/mining', { miner_address: $('minerAddress').value.trim(), mining_enabled: $('miningEnabled').checked, mining_processes: Number($('threads').value), mining_intensity: Number($('intensity').value), randomx_mode: document.querySelector('input[name=mode]:checked')?.value || 'light' }, 'Gespeichert. Die Node startet neu.'));
    $('saveDevice').addEventListener('click', () => action('/admin/config', { device_name: $('deviceName').value.trim(), public_url: $('publicUrl').value.trim() }, 'Gespeichert. Die Node startet neu.'));
    $('pair').addEventListener('click', () => action('/admin/pair', { portal_url: $('portalUrl').value.trim(), code: $('pairCode').value.trim() }, 'Kopplung erfolgreich. Die Node startet neu.'));
    $('unpair').addEventListener('click', () => action('/admin/unpair', {}, 'Kopplung entfernt. Die Node startet neu.'));
    $('start').addEventListener('click', () => action('/admin/start', {}, 'Die Node wird gestartet.'));
    $('stop').addEventListener('click', () => action('/admin/stop', {}, 'P2P und Mining werden gestoppt.'));
    $('sync').addEventListener('click', () => action('/admin/sync', {}, 'Synchronisierung angefordert.'));
    $('restart').addEventListener('click', () => action('/admin/restart', {}, 'Die Node startet neu.'));
    $('refreshLogs').addEventListener('click', loadLogs);
    $('checkUpdate').addEventListener('click', checkUpdate);
    if (secret) connect(); else $('login').showModal();
    setInterval(async () => {
      if (!secret || $('login').open || loading) return;
      loading = true;
      try { await load(); }
      catch (error) {
        $('online').textContent = 'Verbindung unterbrochen · Wiederverbinden …';
        $('online').classList.remove('ok');
        $('online').classList.add('busy');
        $('start').disabled = true; $('stop').disabled = true;
      } finally { loading = false; }
    }, 5000);
  });
})();
