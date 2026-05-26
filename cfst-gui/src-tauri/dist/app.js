// ---- Tauri v2 IPC Bridge ----
// v0: verify script loads at all
(function() {
  var l = document.getElementById('logOutput');
  if (l) l.textContent += '[APPJS] Script file loaded, executing...\n';
})();

// Tauri v2 injects __TAURI__ (with withGlobalTauri: true) or __TAURI_INTERNALS__
function getTauriCore() {
  const t = window.__TAURI__;
  if (t && t.core && t.core.invoke) return t.core;
  const ti = window.__TAURI_INTERNALS__;
  if (ti && ti.invoke) return { invoke: (cmd, args) => ti.invoke(cmd, args || {}) };
  return null;
}

function getTauriEvent() {
  const t = window.__TAURI__;
  if (t && t.event && t.event.listen) return t.event;
  return null;
}

// ---- State ----
let settings = null;
let lastResults = [];
let initialLoadDone = false;

// ---- DOM ----
const $ = (sel) => document.getElementById(sel);
const $$ = (sel) => document.querySelectorAll(sel);

// ---- Safe invoke ----
async function safeInvoke(cmd, args) {
  const core = getTauriCore();
  if (!core) throw new Error('Tauri IPC not available');
  const cleanArgs = {};
  if (args) {
    for (const [k, v] of Object.entries(args)) {
      cleanArgs[k] = v;
    }
  }
  try {
    const result = await core.invoke(cmd, cleanArgs);
    return result;
  } catch (e) {
    appendLog('[IPC] ' + cmd + ' -> ERROR: ' + e + '\n');
    throw e;
  }
}

// ---- Init ----
async function init() {
  appendLog('[INIT] Starting initialization...\n');

  const core = getTauriCore();
  if (!core) {
    appendLog('[ERROR] Tauri IPC not available!\n');
    appendLog('[ERROR] __TAURI__: ' + (typeof window.__TAURI__) + '\n');
    appendLog('[ERROR] __TAURI_INTERNALS__: ' + (typeof window.__TAURI_INTERNALS__) + '\n');
    setStatus('IPC 未就绪', 'error');

    // Also show error overlay
    const overlay = document.createElement('div');
    overlay.style.cssText = 'position:fixed;top:50%;left:50%;transform:translate(-50%,-50%);background:#fff;padding:24px 40px;border-radius:8px;box-shadow:0 4px 24px rgba(0,0,0,0.2);z-index:9999;text-align:center;';
    overlay.innerHTML = '<h2 style="color:#e53935;">Tauri IPC 未就绪</h2><p style="color:#666;">请尝试以下排查:</p><ol style="text-align:left;color:#333;font-size:12px;"><li>确认在 Tauri 环境中运行</li><li>检查 tauri.conf.json 中 withGlobalTauri: true</li><li>查看终端日志了解详情</li></ol>';
    document.body.appendChild(overlay);
    return;
  }

  appendLog('[INIT] IPC bridge found: ' + (window.__TAURI__ ? '__TAURI__' : '__TAURI_INTERNALS__') + '\n');

  try {
    settings = await safeInvoke('get_settings');
    appendLog('[INIT] 设置已加载\n');
    applySettingsToUI(settings);
    updatePresetUI();
    refreshCommandPreview();
    initialLoadDone = true;
  } catch (e) {
    appendLog('[ERROR] 加载设置失败: ' + e + '\n');
  }

  // Event listener
  const eventApi = getTauriEvent();
  if (eventApi) {
    try {
      const unlisten = eventApi.listen('cfst:event', (event) => {
        const payload = event.payload;
        if (!payload) return;
        if (payload.event_type === 'log' || payload.type === 'log') {
          appendLog(payload.message || '');
        } else if (payload.event_type === 'done' || payload.type === 'done') {
          appendLog(payload.message || '\nCFST completed.\n');
          setStatus('测速完成', '');
        } else if (payload.event_type === 'error' || payload.type === 'error') {
          appendLog('\n[ERROR] ' + payload.message + '\n');
          setStatus('测速出错', 'error');
        }
      });
      appendLog('[INIT] 事件监听已注册\n');
    } catch (e) {
      appendLog('[WARN] 事件监听失败: ' + e + '\n');
    }
  } else {
    appendLog('[WARN] Tauri event API not available\n');
  }

  bindEvents();
  appendLog('[INIT] 事件绑定完成\n');
  loadHistoryFromSettings();

  try {
    const ts = await safeInvoke('get_token_status');
    updateTokenUI(ts);
  } catch (e) {
    appendLog('[WARN] Token状态查询失败: ' + e + '\n');
  }

  setStatus('就绪', '');
}

// ---- Settings ----
function applySettingsToUI(s) {
  if (!s) return;
  $('baseUrl').value = s.base_url || '';
  $('cfstPath').value = s.cfst_path || '';
  $('outputDir').value = s.output_dir || '';
  $('extraArgs').value = (s.cfst && s.cfst.extra_args) || '';
  $('addressFamily').value = (s.cfst && s.cfst.address_family) || 'Auto';
  $('cfstPort').value = (s.cfst && s.cfst.port) || 443;
  $('cfstTop').value = (s.cfst && s.cfst.top) || 10;
  $('cfstThreads').value = (s.cfst && s.cfst.thread_count) || 100;
  $('cfstLatency').value = (s.cfst && s.cfst.latency_limit) || 150;
  $('cfstHttping').checked = !s.cfst || s.cfst.httping !== false;
  $('autoUpload').checked = s.auto_upload === true;

  if (s.presets && s.presets.length > 0) {
    const idx = Math.min((s.cfst && s.cfst.preset_index) || 0, s.presets.length - 1);
    if (s.cfst) s.cfst.preset_index = idx;
    settings.presets = s.presets;
  }
}

function collectSettingsFromUI() {
  if (!settings) settings = {};
  settings.base_url = $('baseUrl').value.trim();
  settings.cfst_path = $('cfstPath').value.trim();
  settings.output_dir = $('outputDir').value.trim();
  if (!settings.cfst) settings.cfst = {};
  settings.cfst.extra_args = $('extraArgs').value.trim();
  settings.cfst.address_family = $('addressFamily').value;
  settings.cfst.port = parseInt($('cfstPort').value) || 443;
  settings.cfst.top = parseInt($('cfstTop').value) || 10;
  settings.cfst.thread_count = parseInt($('cfstThreads').value) || 100;
  settings.cfst.latency_limit = parseInt($('cfstLatency').value) || 150;
  settings.cfst.httping = $('cfstHttping').checked;
  settings.auto_upload = $('autoUpload').checked;
  return settings;
}

// ---- Presets ----
function updatePresetUI() {
  const list = $('presetList');
  if (!settings || !settings.presets) return;
  const presets = settings.presets;
  const idx = (settings.cfst && settings.cfst.preset_index) || 0;
  list.innerHTML = presets.map((p, i) =>
    `<div class="preset-item${i === idx ? ' active' : ''}" data-index="${i}">
      <span>${escapeHtml(p.name)}</span>
      <span class="preset-desc">${escapeHtml(p.description)}</span>
    </div>`
  ).join('');

  const active = presets[idx];
  if (active && idx < presets.length - 1) {
    $('extraArgs').value = active.args || '';
  }

  list.querySelectorAll('.preset-item').forEach(el => {
    el.addEventListener('click', () => {
      const i = parseInt(el.dataset.index);
      settings.cfst.preset_index = i;
      const p = settings.presets[i];
      if (i < settings.presets.length - 1) {
        $('extraArgs').value = p.args || '';
      }
      updatePresetUI();
      refreshCommandPreview();
    });
  });
}

// ---- Command Preview ----
async function refreshCommandPreview() {
  const s = collectSettingsFromUI();
  if (!s.cfst_path) {
    $('cmdPreview').textContent = '请先选择 cfst 程序路径并选择测速模式';
    return;
  }
  try {
    const result = await safeInvoke('preview_command', {
      cfstPath: s.cfst_path,
      options: s.cfst || {},
      ipFilePath: s.ip_file_path || '',
      ipv6FilePath: s.ipv6_file_path || '',
    });
    $('cmdPreview').textContent = result.command_line;
  } catch (e) {
    $('cmdPreview').textContent = `[Error] ${e}`;
  }
}

// ---- Log ----
function appendLog(text) {
  var log = $('logOutput');
  if (!log) return;
  if (typeof text !== 'string') return;

  // Detect progress lines: "N / M [___] 可用: X" pattern
  // Replace the last log line instead of appending a new one
  if (/^\d+\s*\/\s*\d+\s*\[/.test(text)) {
    var content = log.textContent;
    var lastNL = content.lastIndexOf('\n');
    log.textContent = (lastNL >= 0 ? content.substring(0, lastNL + 1) : '') + text;
  } else {
    log.textContent += text;
  }
  log.scrollTop = log.scrollHeight;
}

function clearLog() {
  const log = $('logOutput');
  if (log) log.textContent = '';
}

// ---- Status ----
function setStatus(text, cls) {
  const s = $('statusBar');
  if (!s) return;
  s.textContent = text;
  s.className = 'status';
  if (cls) s.classList.add(cls);
}

// ---- Token UI ----
function updateTokenUI(ts) {
  const el = $('tokenStatus');
  if (!el) return;
  if (ts.is_unlocked) {
    el.textContent = 'Token 已解锁';
    el.className = 'hint success';
  } else if (ts.has_encrypted) {
    el.textContent = 'Token 已加密存储，需解锁';
    el.className = 'hint';
  } else {
    el.textContent = '未设置 Token';
    el.className = 'hint';
  }
}

// ---- Results ----
function renderResults(ips) {
  lastResults = ips || [];
  const tbody = $('resultsBody');
  const countEl = $('resultCount');
  if (!ips || ips.length === 0) {
    tbody.innerHTML = '<tr><td colspan="6" class="muted">暂无结果</td></tr>';
    countEl.textContent = '';
    return;
  }
  countEl.textContent = '(' + ips.length + ' 条)';
  tbody.innerHTML = ips.map((ip, i) =>
    '<tr>' +
      '<td><input type="checkbox" class="ip-check" data-index="' + i + '" checked></td>' +
      '<td>' + escapeHtml(ip.ip) + '</td>' +
      '<td>' + ip.port + '</td>' +
      '<td>' + (ip.latency_ms != null ? ip.latency_ms.toFixed(1) : '-') + '</td>' +
      '<td>' + escapeHtml(ip.download_speed || '-') + '</td>' +
      '<td>' + escapeHtml(ip.packet_loss || '-') + '</td>' +
    '</tr>'
  ).join('');
  updateSelectAllState();

  tbody.querySelectorAll('.ip-check').forEach(cb => {
    cb.addEventListener('change', updateSelectAllState);
  });
}

function getSelectedIps() {
  const checks = $$('.ip-check:checked');
  const ips = [];
  checks.forEach(cb => {
    const idx = parseInt(cb.dataset.index);
    if (lastResults[idx]) ips.push(lastResults[idx].ip);
  });
  return ips;
}

function updateSelectAllState() {
  const all = $$('.ip-check');
  const checked = $$('.ip-check:checked');
  const checkAll = $('checkAll');
  if (!checkAll) return;
  checkAll.indeterminate = checked.length > 0 && checked.length < all.length;
  checkAll.checked = checked.length === all.length && all.length > 0;
}

function selectAll() {
  const checkAll = $('checkAll');
  $$('.ip-check').forEach(cb => { cb.checked = checkAll.checked; });
}

function selectNone() {
  $$('.ip-check').forEach(cb => { cb.checked = false; });
  const checkAll = $('checkAll');
  if (checkAll) { checkAll.checked = false; checkAll.indeterminate = false; }
}

function selectInvert() {
  $$('.ip-check').forEach(cb => { cb.checked = !cb.checked; });
  updateSelectAllState();
}

// ---- History ----
function loadHistoryFromSettings() {
  const list = $('historyList');
  if (!list) return;
  if (!settings || !settings.upload_history || settings.upload_history.length === 0) {
    list.innerHTML = '<span class="muted">暂无记录</span>';
    return;
  }
  list.innerHTML = settings.upload_history.slice(0, 20).map(h =>
    '<div class="history-item">' +
      '<div>' +
        '<span class="time">' + escapeHtml(h.time) + '</span> ' +
        '<span>' + escapeHtml(h.group_name || h.group_id) + '</span> ' +
        '<span class="' + (h.success ? 'ok' : 'fail') + '">' + (h.success ? 'OK' : 'FAIL') + '</span>' +
      '</div>' +
      '<span class="muted">' + h.ip_count + ' IPs</span>' +
    '</div>'
  ).join('');
}

// ---- Event Bindings ----
function bindEvents() {
  // Save settings
  const btnSave = $('btnSaveSettings');
  if (btnSave) btnSave.addEventListener('click', async () => {
    try {
      const s = collectSettingsFromUI();
      await safeInvoke('save_settings', { settings: s });
      settings = s;
      $('tokenStatus').textContent = '设置已保存';
      $('tokenStatus').className = 'hint success';
      setStatus('设置已保存', '');
    } catch (e) {
      const el = $('tokenStatus');
      if (el) { el.textContent = '保存失败: ' + e; el.className = 'hint error'; }
    }
  });

  // Browse cfst
  const btnCfst = $('btnBrowseCfst');
  if (btnCfst) btnCfst.addEventListener('click', async () => {
    try {
      const path = await safeInvoke('select_cfst_path');
      if (path) { $('cfstPath').value = path; refreshCommandPreview(); }
    } catch (e) { appendLog('[WARN] 文件选择器不可用: ' + e + '\n'); }
  });

  // Browse output dir
  const btnOut = $('btnBrowseOutput');
  if (btnOut) btnOut.addEventListener('click', async () => {
    try {
      const path = await safeInvoke('select_output_dir');
      if (path) $('outputDir').value = path;
    } catch (e) { appendLog('[WARN] 目录选择器不可用: ' + e + '\n'); }
  });

  // Token save
  const btnSaveToken = $('btnSaveToken');
  if (btnSaveToken) btnSaveToken.addEventListener('click', async () => {
    try {
      const token = $('apiToken').value.trim();
      const pwd = $('masterPwd').value;
      if (!token || !pwd) {
        const el = $('tokenStatus');
        if (el) { el.textContent = '请输入 Token 和密码'; el.className = 'hint error'; }
        return;
      }
      const encrypted = await safeInvoke('encrypt_token', { token, password: pwd });
      const s = collectSettingsFromUI();
      s.encrypted_token = encrypted;
      await safeInvoke('save_settings', { settings: s });
      settings = s;
      const el = $('tokenStatus');
      if (el) { el.textContent = 'Token 已加密保存并解锁'; el.className = 'hint success'; }
      $('apiToken').value = '';
    } catch (e) {
      const el = $('tokenStatus');
      if (el) { el.textContent = '加密失败: ' + e; el.className = 'hint error'; }
    }
  });

  // Token unlock
  const btnUnlock = $('btnUnlockToken');
  if (btnUnlock) btnUnlock.addEventListener('click', async () => {
    try {
      if (!settings || !settings.encrypted_token) {
        const el = $('tokenStatus');
        if (el) { el.textContent = '没有已保存的 Token'; el.className = 'hint error'; }
        return;
      }
      const pwd = $('masterPwd').value;
      if (!pwd) {
        const el = $('tokenStatus');
        if (el) { el.textContent = '请输入主密码'; el.className = 'hint error'; }
        return;
      }
      const ok = await safeInvoke('unlock_token', { encrypted: settings.encrypted_token, password: pwd });
      const el = $('tokenStatus');
      if (el) {
        el.textContent = ok ? 'Token 已解锁' : '密码错误';
        el.className = ok ? 'hint success' : 'hint error';
      }
    } catch (e) {
      const el = $('tokenStatus');
      if (el) { el.textContent = '解锁失败: ' + e; el.className = 'hint error'; }
    }
  });

  // Run CFST
  const btnRun = $('btnRun');
  if (btnRun) btnRun.addEventListener('click', async () => {
    const s = collectSettingsFromUI();
    if (!s.cfst_path) {
      appendLog('[ERROR] 请先选择 cfst 程序路径\n');
      return;
    }
    btnRun.disabled = true;
    const btnStop = $('btnStop');
    if (btnStop) btnStop.disabled = false;
    setStatus('测速中...', 'running');
    clearLog();
    appendLog('正在启动 cfst...\n');
    try {
      const ips = await safeInvoke('run_cfst', {
        cfstPath: s.cfst_path,
        options: s.cfst || {},
        ipFilePath: s.ip_file_path || '',
        ipv6FilePath: s.ipv6_file_path || '',
      });
      lastResults = ips;
      renderResults(ips);
      appendLog('\n测速完成，共 ' + (ips ? ips.length : 0) + ' 条结果\n');

      if (s.auto_upload && ips && ips.length > 0) {
        const groupId = $('targetGroup').value;
        if (groupId) {
          appendLog('自动上传中...\n');
          await doUpload(groupId, ips.map(ip => ip.ip));
        }
      }

      try {
        settings = await safeInvoke('get_settings');
        loadHistoryFromSettings();
      } catch (e) { /* ignore */ }
    } catch (e) {
      appendLog('\n[ERROR] ' + e + '\n');
      setStatus('测速出错', 'error');
    } finally {
      btnRun.disabled = false;
      const btnStop = $('btnStop');
      if (btnStop) btnStop.disabled = true;
    }
  });

  // Stop CFST
  const btnStop = $('btnStop');
  if (btnStop) btnStop.addEventListener('click', async () => {
    try {
      await safeInvoke('stop_cfst');
      appendLog('\n[用户停止]\n');
      setStatus('已停止', '');
      const b = $('btnRun');
      if (b) b.disabled = false;
      btnStop.disabled = true;
    } catch (e) { /* ignore */ }
  });

  // Refresh command
  const btnPreview = $('btnPreview');
  if (btnPreview) btnPreview.addEventListener('click', refreshCommandPreview);

  // Clear log
  const btnClear = $('btnClearLog');
  if (btnClear) btnClear.addEventListener('click', clearLog);

  // Select all/none/invert
  const checkAll = $('checkAll');
  if (checkAll) checkAll.addEventListener('change', selectAll);
  const btnAll = $('btnSelectAll');
  if (btnAll) btnAll.addEventListener('click', () => { const ca = $('checkAll'); if (ca) ca.checked = true; selectAll(); });
  const btnNone = $('btnSelectNone');
  if (btnNone) btnNone.addEventListener('click', selectNone);
  const btnInv = $('btnSelectInvert');
  if (btnInv) btnInv.addEventListener('click', selectInvert);

  // Fetch groups
  const btnFetch = $('btnFetchGroups');
  if (btnFetch) btnFetch.addEventListener('click', async () => {
    try {
      const s = collectSettingsFromUI();
      if (!s.base_url) {
        alert('请先设置服务器地址');
        return;
      }
      const groups = await safeInvoke('fetch_groups', { baseUrl: s.base_url, apiToken: '' });
      const sel = $('targetGroup');
      sel.innerHTML = groups.map(g =>
        '<option value="' + g.id + '">' + escapeHtml(g.name) + ' (' + (g.ips ? g.ips.length : (g.count || 0)) + ' IPs)</option>'
      ).join('');
      if (s.selected_group_id) sel.value = s.selected_group_id;
      const st = $('uploadStatus');
      if (st) { st.textContent = '已加载 ' + groups.length + ' 个分组'; st.className = 'hint success'; }
    } catch (e) {
      const st = $('uploadStatus');
      if (st) { st.textContent = '获取失败: ' + e; st.className = 'hint error'; }
    }
  });

  // Upload
  const btnUpload = $('btnUpload');
  if (btnUpload) btnUpload.addEventListener('click', async () => {
    const groupId = $('targetGroup').value;
    const st = $('uploadStatus');
    if (!groupId) {
      if (st) { st.textContent = '请先选择目标分组'; st.className = 'hint error'; }
      return;
    }
    const ips = getSelectedIps();
    if (ips.length === 0) {
      if (st) { st.textContent = '请选择要上传的 IP'; st.className = 'hint error'; }
      return;
    }
    await doUpload(groupId, ips);
  });

  // Collapse panels
  document.querySelectorAll('.panel-header[data-toggle]').forEach(hdr => {
    hdr.addEventListener('click', () => {
      const targetId = hdr.dataset.toggle;
      const body = document.getElementById(targetId);
      if (body) {
        body.classList.toggle('collapsed');
        hdr.classList.toggle('collapsed');
      }
    });
  });

  // Auto-refresh command preview on input change
  const refreshInputs = ['cfstPath', 'extraArgs', 'addressFamily', 'cfstPort', 'cfstTop', 'cfstThreads', 'cfstLatency', 'cfstHttping'];
  refreshInputs.forEach(id => {
    const el = $(id);
    if (el) {
      el.addEventListener('change', refreshCommandPreview);
      el.addEventListener('input', refreshCommandPreview);
    }
  });
}

async function doUpload(groupId, ips) {
  try {
    const s = collectSettingsFromUI();
    const sel = $('targetGroup');
    const groupName = sel.options[sel.selectedIndex] ? sel.options[sel.selectedIndex].text : groupId;

    const result = await safeInvoke('upload_ips', {
      baseUrl: s.base_url,
      apiToken: '',
      groupId: groupId,
      groupName: groupName,
      ips: ips,
    });

    const st = $('uploadStatus');
    const cls = result.ok ? 'hint success' : 'hint error';
    const msg = result.ok ? ('上传成功: ' + result.count + ' IPs') : ('上传失败: ' + (result.error || 'Unknown'));
    if (st) { st.textContent = msg; st.className = cls; }
    appendLog(msg + '\n');

    try {
      settings = await safeInvoke('get_settings');
      loadHistoryFromSettings();
    } catch (e) { /* ignore */ }
  } catch (e) {
    const st = $('uploadStatus');
    if (st) { st.textContent = '上传出错: ' + e; st.className = 'hint error'; }
    appendLog('[ERROR] 上传: ' + e + '\n');
  }
}

// ---- Utility ----
function escapeHtml(str) {
  if (!str) return '';
  const div = document.createElement('div');
  div.textContent = String(str);
  return div.innerHTML;
}

// ---- Boot ----
function boot() {
  appendLog('[BOOT] DOM ready, checking Tauri IPC...\n');

  // Dump what globals exist for diagnostics
  const hasTAURI = typeof window.__TAURI__ !== 'undefined';
  const hasInternals = typeof window.__TAURI_INTERNALS__ !== 'undefined';
  appendLog('[BOOT] __TAURI__: ' + hasTAURI + ', __TAURI_INTERNALS__: ' + hasInternals + '\n');

  if (hasTAURI) {
    const t = window.__TAURI__;
    appendLog('[BOOT] __TAURI__.core: ' + (!!t.core) + ', __TAURI__.event: ' + (!!t.event) + '\n');
    if (t.core) {
      const keys = Object.keys(t.core);
      appendLog('[BOOT] __TAURI__.core keys: ' + keys.join(', ') + '\n');
    }
  }

  if (hasInternals) {
    const ti = window.__TAURI_INTERNALS__;
    const keys = Object.keys(ti);
    appendLog('[BOOT] __TAURI_INTERNALS__ keys: ' + keys.join(', ') + '\n');
  }

  init();
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', function() { setTimeout(boot, 50); });
} else {
  // DOM already loaded, boot immediately
  setTimeout(boot, 50);
}
