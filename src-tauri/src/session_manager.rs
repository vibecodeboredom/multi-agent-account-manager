use crate::group_transport;
#[cfg(desktop)]
use crate::group_transport::ResolvedMemberTransport;
use crate::platform::{self, current_platform_name, ProxyTestResult};
use crate::providers::{
  all_provider_infos, classify_top_level_url, resolve_provider, NavigationDecision, ProviderId,
  ProviderInfo,
};
#[cfg(desktop)]
use crate::providers::{is_verified_callback_origin, ProviderSpec};
#[cfg(desktop)]
use crate::site_adapters::SiteAdapterConfig;
use crate::site_adapters::{self, SiteAdaptersSnapshot};
#[cfg(desktop)]
use crate::storage::GroupChatMessage;
#[cfg(any(desktop, test))]
use crate::storage::GroupThreadBinding;
use crate::storage::{
  detect_conflicts, load_or_migrate_store, normalize_uuid, now_ms, save_store_atomic, AccountKey,
  AccountRecord, AccountsStoreFile, AppSettings, AuthState, ConflictItem, ExternalAgentKind,
  ExternalAgentRecord, GroupChatRecord, GroupMemberRole, GroupReplyStatus, GroupSharedSkill,
  GroupWorkflowMode, QuotaDetail,
};
use serde::Serialize;
#[cfg(desktop)]
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

pub const APP_TITLE: &str = "叠号";
const MAX_GROUP_MEMBERS: usize = 30;
#[cfg(desktop)]
const REVIEW_INLINE_CHAR_LIMIT: usize = 1200;
#[cfg(desktop)]
const REVIEW_FALLBACK_CHAR_LIMIT: usize = 4500;
#[cfg_attr(not(desktop), allow(dead_code))]
const AUTO_ROUTE_STICKY_MAX_USED_PCT: u8 = 85;
#[cfg_attr(not(desktop), allow(dead_code))]
const AUTO_ROUTE_COOLDOWN_MS: u64 = 120_000;
#[cfg(desktop)]
const LEFT_PANE_WIDTH: f64 = 380.0;
#[cfg(desktop)]
const MAX_WARM_DESKTOP_VIEWS: usize = 10;
#[cfg(desktop)]
const GROUP_MAX_CONCURRENT_WORKERS: usize = 6;
#[cfg(desktop)]
const GROUP_IDLE_RECLAIM_MS: u64 = 10 * 60 * 1000;
#[cfg(desktop)]
const MAX_CONCURRENT_PREWARMS: usize = 2;

#[cfg(desktop)]
const BACKGROUND_RAF_INIT_JS: &str = r#"(() => {
  try {
    if (window.__maamRafInstalled) return;
    window.__maamRafInstalled = true;
    const origRaf = window.requestAnimationFrame ? window.requestAnimationFrame.bind(window) : null;
    const origCancel = window.cancelAnimationFrame ? window.cancelAnimationFrame.bind(window) : null;
    if (!origRaf) return;
    let nextId = 1;
    const timers = new Map();
    const natives = new Map();
    window.requestAnimationFrame = (cb) => {
      if (typeof cb !== 'function') return 0;
      const id = nextId++;
      if (document.visibilityState !== 'hidden') {
        const nid = origRaf((ts) => {
          natives.delete(id);
          cb(ts);
        });
        natives.set(id, nid);
        return id;
      }
      const tid = setTimeout(() => {
        timers.delete(id);
        cb(performance.now());
      }, 32);
      timers.set(id, tid);
      return id;
    };
    window.cancelAnimationFrame = (id) => {
      if (timers.has(id)) {
        clearTimeout(timers.get(id));
        timers.delete(id);
      } else if (natives.has(id)) {
        const nid = natives.get(id);
        natives.delete(id);
        if (origCancel) origCancel(nid);
      } else if (origCancel) {
        origCancel(id);
      }
    };
  } catch (_) {}
})()"#;

#[cfg(desktop)]
const LOGIN_CAPTURE_INIT_JS: &str = r#"(() => {
  try {
    if (location.protocol !== 'https:') return;
    if (window !== window.top) return;
    const initHost = (location.hostname || '').toLowerCase();
    const isPrimaryHost = initHost === 'muse.ai' || initHost.endsWith('.muse.ai') || initHost === 'app.hark.com' || initHost.endsWith('.hark.com');
    const origFetch = window.fetch;
    const authCheck = window.__maamAuthCheck || {
      status: 'unknown',
      reason: '',
      checkedAt: 0,
      inflight: false
    };
    window.__maamAuthCheck = authCheck;
    const markAuthInvalid = (reason) => {
      authCheck.status = 'invalid';
      authCheck.reason = String(reason || 'unauthorized');
      authCheck.checkedAt = Date.now();
      window.__maamQuota = null;
      window.__maamQuotaDetail = null;
      try {
        localStorage.removeItem('__maam_cached_quota__');
        localStorage.removeItem('__maam_cached_quota_detail_v2__');
      } catch (_) {}
    };
    const markAuthOk = () => {
      authCheck.status = 'ok';
      authCheck.reason = '';
      authCheck.checkedAt = Date.now();
    };
    const BAD_EMAIL_RE = /^(?:support|privacy|help|legal|press|noreply|no-reply|info|security|contact|abuse|feedback|sales|terms|hello|team|admin|postmaster|notifications)@/i;
    const EMAIL_STRICT_RE = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/;
    const PROVIDER_EMAIL_RE = /@(?:[a-z0-9-]+\.)?(?:muse\.ai|hark\.com|meta\.com|facebook\.com)$/i;
    const BAD_NAME_RE = /^(?:Meta|Muse|Hark|Button|Settings|Profile|Account|User|Avatar|Loading|Unknown|Hatch|Home|Chat|New Chat|Log in|Sign up|待登录账号|已登录账号|Account not signed in|Signed-in account|通用|设置)$/i;
    const isGoodEmail = (s) => {
      if (typeof s !== 'string') return false;
      const clean = s.trim().toLowerCase();
      return EMAIL_STRICT_RE.test(clean) && !BAD_EMAIL_RE.test(clean) && !clean.endsWith('@example.com') && !clean.endsWith('@sentry.io') && !clean.endsWith('@wixpress.com');
    };
    const recordApiEmail = (raw) => {
      if (!isGoodEmail(raw)) return;
      const clean = raw.trim().toLowerCase();
      if (!window.__maamApiEmail || (PROVIDER_EMAIL_RE.test(window.__maamApiEmail) && !PROVIDER_EMAIL_RE.test(clean))) {
        window.__maamApiEmail = clean;
      }
    };
    const recordApiName = (raw) => {
      if (typeof raw !== 'string') return;
      const clean = raw.replace(/\s+/g, ' ').trim();
      if (clean.length < 2 || clean.length > 60) return;
      if (/^https?:\/\//i.test(clean) || /^\d+$/.test(clean) || BAD_NAME_RE.test(clean)) return;
      if (!window.__maamApiName) {
        window.__maamApiName = clean;
      }
    };
    window.__maamRecordApiEmail = recordApiEmail;
    window.__maamRecordApiName = recordApiName;
    if (initHost === 'app.hark.com' || initHost.endsWith('.hark.com')) {
      try {
        if (document.documentElement && !document.documentElement.style.backgroundColor) {
          document.documentElement.style.backgroundColor = '#171d2f';
        }
      } catch (_) {}
    }
    const setQuota = (text, pct) => {
      if (!text) return;
      const cleanPct = typeof pct === 'number' && Number.isFinite(pct)
        ? Math.max(0, Math.min(100, Math.round(pct)))
        : null;
      const obj = { text: String(text).slice(0, 48), pct: cleanPct };
      window.__maamQuota = obj;
      try { localStorage.setItem('__maam_cached_quota__', JSON.stringify(obj)); } catch (_) {}
    };
    const saveDetail = (detail) => {
      if (!detail || !detail.planLabel) return;
      window.__maamQuotaDetail = detail;
      try { localStorage.setItem('__maam_cached_quota_detail_v2__', JSON.stringify(detail)); } catch (_) {}
      const rem = typeof detail.planUsedPercent === 'number'
        ? Math.max(0, Math.min(100, 100 - detail.planUsedPercent))
        : null;
      setQuota(detail.planValue || detail.planLabel, rem);
    };
    const formatHarkReset = (iso) => {
      if (!iso || typeof iso !== 'string') return null;
      const d = new Date(iso);
      if (Number.isNaN(d.getTime())) return null;
      const pad = (n) => String(n).padStart(2, '0');
      const hhmm = pad(d.getHours()) + ':' + pad(d.getMinutes());
      const now = new Date();
      const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
      const targetStart = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
      const diffDays = Math.round((targetStart - todayStart) / 86400000);
      if (diffDays === 0) return '今天 ' + hhmm + ' 重置';
      if (diffDays === 1) return '明天 ' + hhmm + ' 重置';
      if (d.getFullYear() === now.getFullYear()) {
        return (d.getMonth() + 1) + '月' + d.getDate() + '日 ' + hhmm + ' 重置';
      }
      return d.getFullYear() + '年' + (d.getMonth() + 1) + '月' + d.getDate() + '日 ' + hhmm + ' 重置';
    };
    const mapHarkSub = (node) => {
      if (!(location.hostname || '').toLowerCase().endsWith('hark.com')) return null;
      if (!node || typeof node !== 'object') return null;
      const meters = (node.meters && typeof node.meters === 'object' ? (node.meters.harkTokens || node.meters) : null)
        || (node.harkTokens && typeof node.harkTokens === 'object' ? node.harkTokens : null)
        || (('dailyLimit' in node || 'poolLimit' in node) ? node : null);
      if (meters && typeof meters === 'object' && (typeof meters.dailyLimit === 'number' || typeof meters.poolLimit === 'number')) {
        const rawPlan = (node.plan && typeof node.plan === 'object' && typeof node.plan.name === 'string' && node.plan.name.trim())
          || (typeof node.planName === 'string' && node.planName.trim())
          || (typeof node.plan === 'string' && node.plan.trim())
          || '';
        const shortPlan = rawPlan.replace(/^hark\s+/i, '').trim();
        const hasDaily = typeof meters.dailyLimit === 'number' && meters.dailyLimit > 0 && typeof meters.dailyUsed === 'number';
        const hasMonthly = typeof meters.poolLimit === 'number' && meters.poolLimit > 0 && typeof meters.poolUsed === 'number';
        if (hasDaily || hasMonthly) {
          const dailyPct = hasDaily ? Math.max(0, Math.min(100, Math.round((meters.dailyUsed / meters.dailyLimit) * 100))) : null;
          const monthlyPct = hasMonthly ? Math.max(0, Math.min(100, Math.round((meters.poolUsed / meters.poolLimit) * 100))) : null;
          if (hasDaily) {
            return {
              planLabel: shortPlan ? (shortPlan + ' · 每日') : '每日上限',
              planValue: 'Used ' + dailyPct + '%',
              planSubtext: formatHarkReset(meters.dailyResetsAt),
              planUsedPercent: dailyPct,
              extraLabel: hasMonthly ? '每月上限' : null,
              extraValue: hasMonthly ? ('Used ' + monthlyPct + '%') : null,
              extraSubtext: hasMonthly ? formatHarkReset(meters.poolResetsAt) : null,
              extraUsedPercent: monthlyPct
            };
          }
          return {
            planLabel: shortPlan ? (shortPlan + ' · 每月') : '每月上限',
            planValue: 'Used ' + monthlyPct + '%',
            planSubtext: formatHarkReset(meters.poolResetsAt),
            planUsedPercent: monthlyPct,
            extraLabel: null,
            extraValue: null,
            extraSubtext: null,
            extraUsedPercent: null
          };
        }
      }
      const u = node.usage || node.billing || node.credits || node.subscription || node;
      if (!u || typeof u !== 'object' || 'page' in u || 'items' in u || 'messages' in u) return null;
      const rem = u.remaining ?? u.remaining_credits ?? u.remainingCredits ?? u.credits_remaining ?? u.creditsRemaining ?? u.balance ?? u.creditBalance ?? u.left;
      const tot = u.total ?? u.limit ?? u.total_credits ?? u.totalCredits ?? u.max ?? u.allowance;
      const used = u.used ?? u.used_credits ?? u.usedCredits ?? u.consumed;
      const planName = u.plan_name || u.planName || u.plan || node.plan_name || node.planName || node.plan || (typeof node.tier === 'string' ? node.tier : null) || (typeof u.name === 'string' ? u.name : null) || 'Hark 额度';
      if (typeof rem === 'number' || typeof used === 'number') {
        let usedPct = null;
        let planVal = '';
        if (typeof tot === 'number' && tot > 0) {
          const actualUsed = typeof used === 'number' ? used : (tot - (typeof rem === 'number' ? rem : 0));
          usedPct = Math.max(0, Math.min(100, Math.round((actualUsed / tot) * 100)));
          planVal = 'Used ' + usedPct + '%';
        } else if (typeof rem === 'number') {
          planVal = '剩余 ' + rem;
        }
        const resetsAt = u.resets_at ?? u.resetsAt;
        return {
          planLabel: String(planName),
          planValue: planVal,
          planSubtext: resetsAt ? ('将在 ' + new Date(resetsAt).toLocaleDateString() + ' 重置') : (u.period ? ('周期: ' + u.period) : null),
          planUsedPercent: usedPct,
          extraLabel: null,
          extraValue: null,
          extraSubtext: null,
          extraUsedPercent: null
        };
      }
      return null;
    };
    const mapSub = (node) => {
      if (!node || typeof node !== 'object') return null;
      const harkRes = mapHarkSub(node);
      if (harkRes) return harkRes;
      const s = (node.subscription && typeof node.subscription === 'object' && node.subscription.usage)
        ? node.subscription
        : node;
      const u = s.usage;
      if (!u || typeof u !== 'object') return null;
      if (!('state' in u) && !('percentUsed' in u) && !('percent_used' in u) && !('quotaStatus' in u) && !('quota_status' in u)) {
        return null;
      }
      const subType = s.subscriptionType || s.subscription_type || '';
      const tierName = (s.tier && typeof s.tier.name === 'string' && s.tier.name.trim()) || '';
      const planLabel = (typeof s.usageRowLabel === 'string' && s.usageRowLabel.trim())
        || (typeof s.usage_row_label === 'string' && s.usage_row_label.trim())
        || tierName
        || (subType === 'PAID' ? '订阅版' : subType === 'COMPLIMENTARY' ? '赠送版' : subType === 'FREE' ? '免费版' : '基础额度');
      const rawPct = typeof u.percentUsed === 'number' ? u.percentUsed : u.percent_used;
      const qStatus = u.quotaStatus || u.quota_status || '';
      let planUsedPercent = null;
      if (u.state !== 'UNLIMITED' && typeof rawPct === 'number' && Number.isFinite(rawPct)) {
        planUsedPercent = Math.max(0, Math.min(100, Math.round(rawPct)));
      } else if (qStatus === 'DEPLETED') {
        planUsedPercent = 100;
      }
      const rawVal = (typeof s.usageRowValueLabel === 'string' && s.usageRowValueLabel.trim())
        || (typeof s.usage_row_value_label === 'string' && s.usage_row_value_label.trim())
        || '';
      const planValue = rawVal
        || (u.state === 'UNLIMITED' ? 'Unlimited quota'
          : planUsedPercent !== null ? ('Used ' + planUsedPercent + '%')
          : qStatus === 'DEPLETED' ? 'Depleted'
          : qStatus === 'LOW' ? 'Low quota'
          : 'Quota available');
      let planSubtext = (typeof s.statusSubtitle === 'string' && s.statusSubtitle.trim())
        || (typeof s.status_subtitle === 'string' && s.status_subtitle.trim())
        || (typeof s.usageRowFooter === 'string' && s.usageRowFooter.trim())
        || (typeof s.usage_row_footer === 'string' && s.usage_row_footer.trim())
        || null;
      if (!planSubtext) {
        const rawReset = u.resetsAt ?? u.resets_at;
        if (typeof rawReset === 'number' && rawReset > 1000000000) {
          const ms = rawReset > 100000000000 ? rawReset : rawReset * 1000;
          const d = new Date(ms);
          if (!Number.isNaN(d.getTime())) {
            planSubtext = '将在 ' + (d.getMonth() + 1) + '月' + d.getDate() + '日重置';
          }
        }
      }
      const topupVal = (typeof s.topupRowValueLabel === 'string' && s.topupRowValueLabel.trim())
        || (typeof s.topup_row_value_label === 'string' && s.topup_row_value_label.trim())
        || '';
      const topupBal = typeof s.topupBalance === 'number' ? s.topupBalance : s.topup_balance;
      const topupTot = typeof s.topupTotal === 'number' ? s.topupTotal : s.topup_total;
      let extraLabel = null;
      let extraValue = null;
      let extraSubtext = null;
      let extraUsedPercent = null;
      if (topupVal || (typeof topupTot === 'number' && topupTot > 0)) {
        extraLabel = (typeof s.topupRowLabel === 'string' && s.topupRowLabel.trim())
          || (typeof s.topup_row_label === 'string' && s.topup_row_label.trim())
          || '额外的使用额度';
        if (typeof topupTot === 'number' && topupTot > 0 && typeof topupBal === 'number') {
          extraUsedPercent = Math.max(0, Math.min(100, Math.round(((topupTot - topupBal) / topupTot) * 100)));
        }
        extraValue = topupVal || (extraUsedPercent !== null ? ('Used ' + extraUsedPercent + '%') : null);
        extraSubtext = '从不过期';
      }
      return {
        planLabel,
        planValue,
        planSubtext,
        planUsedPercent,
        extraLabel,
        extraValue,
        extraSubtext,
        extraUsedPercent
      };
    };
    window.__maamMapSub = mapSub;
    window.__maamSaveDetail = saveDetail;

    const scanTpChunk = (chunk) => {
      if (!Array.isArray(chunk)) return;
      for (let i = 0; i < chunk.length; i++) {
        const item = chunk[i];
        if (Array.isArray(item)) {
          scanTpChunk(item);
          continue;
        }
        if (typeof item === 'function') {
          let src = '';
          try { src = Function.prototype.toString.call(item); } catch (_) { continue; }
          if (src.includes('fetchSubscriptionAction')) {
            const mAct = src.match(/createServerReference\)\("([0-9a-f]{32,64})"[^)]*"fetchSubscriptionAction"\)/);
            if (mAct) window.__maamSubActionId = mAct[1];
            if (i > 0 && typeof chunk[i - 1] === 'number') {
              window.__maamSubModId = chunk[i - 1];
            }
          }
          if (src.includes('preloadHatchSettingsData')) {
            const mPre = src.match(/\.A\((\d+)\)\.then\(\(\{preloadHatchSettingsData\b/);
            if (mPre) window.__maamPreloadModId = parseInt(mPre[1], 10);
          }
          if (src.includes('createServerReference')) {
            const reProf = /createServerReference\)\("([0-9a-f]{32,64})"[^)]*"((?:fetch|get)(?:User|Account|Profile|Viewer|Settings|Email|Identity|Me|CurrentUser|HatchUser|HatchProfile|HatchSettings|HatchEmail)[A-Za-z0-9_]*Action)"\)/g;
            let mp;
            while ((mp = reProf.exec(src)) !== null) {
              if (!Array.isArray(window.__maamProfileActions)) window.__maamProfileActions = [];
              if (!window.__maamProfileActions.includes(mp[1]) && window.__maamProfileActions.length < 5) {
                window.__maamProfileActions.push(mp[1]);
              }
            }
          }
        }
      }
    };

    if ((location.hostname || '').endsWith('muse.ai')) {
      try {
        const tpArr = Array.isArray(globalThis.TURBOPACK) ? globalThis.TURBOPACK : [];
        tpArr.forEach(scanTpChunk);
        const origArrPush = Array.prototype.push;
        tpArr.push = function(...items) {
          items.forEach(scanTpChunk);
          return origArrPush.apply(this, items);
        };
        let tpRuntime = (!Array.isArray(globalThis.TURBOPACK) && globalThis.TURBOPACK) ? globalThis.TURBOPACK : null;
        if (tpRuntime && typeof tpRuntime.push === 'function') {
          const origObjPush = tpRuntime.push.bind(tpRuntime);
          tpRuntime.push = function(...items) {
            items.forEach(scanTpChunk);
            return origObjPush(...items);
          };
        } else {
          Object.defineProperty(globalThis, 'TURBOPACK', {
            configurable: true,
            enumerable: true,
            get() {
              return tpRuntime || tpArr;
            },
            set(val) {
              if (val && typeof val === 'object' && !Array.isArray(val) && typeof val.push === 'function') {
                const origPush = val.push.bind(val);
                val.push = function(...items) {
                  items.forEach(scanTpChunk);
                  return origPush(...items);
                };
                tpRuntime = val;
              } else {
                tpRuntime = val;
              }
            }
          });
        }
      } catch (_) {}
    }

    window.__maamVerifySession = async (force) => {
      if (authCheck.inflight) return;
      const now = Date.now();
      if (!force && authCheck.checkedAt && now - authCheck.checkedAt < 12000) return;
      const curHost = (location.hostname || '').toLowerCase();
      const fetchFn = typeof origFetch === 'function' ? origFetch : window.fetch;
      if (typeof fetchFn !== 'function') return;
      authCheck.inflight = true;
      try {
        if (curHost === 'muse.ai' || curHost.endsWith('.muse.ai')) {
          const r1 = await fetchFn.call(window, '/api/client-version', {
            credentials: 'same-origin',
            cache: 'no-store',
            headers: { Accept: 'application/json' }
          });
          if (r1 && r1.status === 401) {
            markAuthInvalid('client_version_401');
            return;
          }
          if (r1 && r1.ok) {
            try { r1.clone().text().then(inspectText).catch(() => {}); } catch (_) {}
            const r2 = await fetchFn.call(window, '/api/auth/check', {
              method: 'POST',
              credentials: 'include',
              cache: 'no-store',
              headers: { Accept: 'application/json' }
            });
            if (r2 && (r2.status === 401 || r2.status === 403)) {
              markAuthInvalid('auth_check_' + r2.status);
              return;
            }
            if (r2 && r2.ok) {
              try { r2.clone().text().then(inspectText).catch(() => {}); } catch (_) {}
              markAuthOk();
              return;
            }
          }
          authCheck.checkedAt = Date.now();
          return;
        }
        if (curHost === 'app.hark.com' || curHost.endsWith('.hark.com')) {
          const r = await fetchFn.call(window, '/api/profile', {
            credentials: 'include',
            cache: 'no-store',
            headers: { Accept: 'application/json' }
          });
          if (r && r.status === 401) {
            markAuthInvalid('hark_profile_401');
            return;
          }
          if (r && r.ok) {
            try { r.clone().text().then(inspectText).catch(() => {}); } catch (_) {}
            markAuthOk();
            return;
          }
          authCheck.checkedAt = Date.now();
        }
      } catch (_) {
        authCheck.checkedAt = Date.now();
      } finally {
        authCheck.inflight = false;
      }
    };

    window.__maamRefreshBackendQuota = async (force) => {
      if (window.__maamQuotaInflight) return;
      const now = Date.now();
      if (!force && window.__maamQuotaFetchedAt && now - window.__maamQuotaFetchedAt < 90000) return;
      const curHost = (location.hostname || '').toLowerCase();
      if (curHost === 'app.hark.com' || curHost.endsWith('.hark.com')) {
        window.__maamQuotaInflight = true;
        try {
          const fetchFn = typeof origFetch === 'function' ? origFetch : window.fetch;
          const res = await fetchFn.call(window, '/api/billing/summary', {
            credentials: 'include',
            headers: { Accept: 'application/json' }
          });
          if (res && res.status === 401) {
            markAuthInvalid('hark_401');
          } else if (res && res.ok) {
            markAuthOk();
            const data = await res.json();
            const mapped = mapHarkSub(data);
            if (mapped) {
              window.__maamQuotaFetchedAt = Date.now();
              saveDetail(mapped);
            }
          }
          if (!window.__maamApiEmail && authCheck.status !== 'invalid') {
            try {
              const profRes = await fetchFn.call(window, '/api/profile', {
                credentials: 'include',
                headers: { Accept: 'application/json' }
              });
              if (profRes && profRes.status === 401) {
                markAuthInvalid('hark_401');
              } else if (profRes && profRes.ok) {
                markAuthOk();
                const prof = await profRes.json();
                if (prof && typeof prof.email === 'string' && prof.email.includes('@')) {
                  window.__maamApiEmail = prof.email.trim().toLowerCase();
                }
              }
            } catch (_) {}
          }
        } catch (_) {
        } finally {
          window.__maamQuotaInflight = false;
        }
        return;
      }
      const tpGlobal = globalThis.TURBOPACK;
      if (!tpGlobal || Array.isArray(tpGlobal) || typeof tpGlobal.push !== 'function') return;
      window.__maamQuotaInflight = true;
      try {
        if (!window.__maamTp) {
          tpGlobal.push(['__maam_init__', 999999991, (ctx) => { window.__maamTp = ctx; }]);
          tpGlobal.push(['__maam_run__', { otherChunks: [], runtimeModuleIds: [999999991] }]);
        }
        const tp = window.__maamTp;
        if (!tp) return;
        if (!window.__maamSubModId && !window.__maamPreloadModId && !window.__maamScriptsScanned) {
          window.__maamScriptsScanned = true;
          const scripts = Array.from(document.querySelectorAll('script[src*="/_next/static/"]'));
          for (const sc of scripts) {
            if (!sc.src) continue;
            try {
              const txt = await origFetch.call(window, sc.src, { cache: 'force-cache' }).then((r) => r.text());
              const mPre = txt.match(/\.A\((\d+)\)\.then\(\(\{preloadHatchSettingsData\b/);
              if (mPre) window.__maamPreloadModId = parseInt(mPre[1], 10);
              const mAct = txt.match(/createServerReference\)\("([0-9a-f]{32,64})"[^)]*"fetchSubscriptionAction"\)/);
              if (mAct) window.__maamSubActionId = mAct[1];
              if (window.__maamPreloadModId && window.__maamSubActionId) break;
            } catch (_) {}
          }
        }
        if (!window.__maamSubModId && window.__maamPreloadModId && typeof tp.A === 'function') {
          await tp.A(window.__maamPreloadModId).catch(() => {});
        }
        if (window.__maamSubModId && typeof tp.i === 'function') {
          const mod = tp.i(window.__maamSubModId);
          const fn = mod && (mod.f || mod.default || Object.values(mod).find((x) => typeof x === 'function'));
          if (typeof fn === 'function') {
            const res = await fn({ includeAgreement: true });
            inspectNode(res, 0, true);
            const mapped = mapSub(res);
            if (mapped) {
              markAuthOk();
              window.__maamQuotaFetchedAt = Date.now();
              saveDetail(mapped);
              return;
            }
          }
        }
        let callServerFn = null;
        if (tp.c && typeof tp.c.values === 'function') {
          for (const m of tp.c.values()) {
            if (m && m.exports && typeof m.exports.callServer === 'function') {
              callServerFn = m.exports.callServer;
              break;
            }
          }
        }
        const aid = window.__maamSubActionId || '40e0249eeae158685b6a8d0c3b203f53dd4f2c7d63';
        if (callServerFn && aid) {
          const res = await callServerFn(aid, [{ includeAgreement: true }]);
          inspectNode(res, 0, true);
          const mapped = mapSub(res);
          if (mapped) {
            markAuthOk();
            window.__maamQuotaFetchedAt = Date.now();
            saveDetail(mapped);
          }
        }
      } catch (err) {
        const msg = String(err && (err.message || err));
        if (/401|403|unauthenticated|unauthorized|authentication required|checkpoint/i.test(msg)) {
          markAuthInvalid('muse_sub_auth');
        }
      } finally {
        window.__maamQuotaInflight = false;
      }
    };

    window.__maamRefreshIdentity = async () => {
      if (!isPrimaryHost || window.__maamApiEmail || window.__maamIdFetchDone || window.__maamIdFetchInflight) return;
      window.__maamIdFetchInflight = true;
      try {
        const fetchFn = typeof origFetch === 'function' ? origFetch : window.fetch;
        const curHost = (location.hostname || '').toLowerCase();
        if (curHost === 'app.hark.com' || curHost.endsWith('.hark.com')) {
          if (typeof fetchFn === 'function') {
            try {
              const res = await fetchFn.call(window, '/api/profile', {
                credentials: 'include',
                cache: 'no-store',
                headers: { Accept: 'application/json' }
              });
              if (res && res.ok) {
                const txt = await res.text();
                inspectText(txt);
              }
            } catch (_) {}
          }
          return;
        }
        if (curHost === 'muse.ai' || curHost.endsWith('.muse.ai')) {
          if (typeof fetchFn === 'function') {
            try {
              const chk = await fetchFn.call(window, '/api/auth/check', {
                method: 'POST',
                credentials: 'include',
                cache: 'no-store',
                headers: { Accept: 'application/json' }
              });
              if (chk && chk.ok) {
                const txt = await chk.text();
                inspectText(txt);
              }
            } catch (_) {}
          }
          if (!window.__maamApiEmail) {
            const tpGlobal = globalThis.TURBOPACK;
            if (tpGlobal && !Array.isArray(tpGlobal) && typeof tpGlobal.push === 'function') {
              if (!window.__maamTp) {
                tpGlobal.push(['__maam_init__', 999999991, (ctx) => { window.__maamTp = ctx; }]);
                tpGlobal.push(['__maam_run__', { otherChunks: [], runtimeModuleIds: [999999991] }]);
              }
              const tp = window.__maamTp;
              if (tp && window.__maamPreloadModId && typeof tp.A === 'function') {
                try {
                  const preMod = await tp.A(window.__maamPreloadModId).catch(() => null);
                  if (preMod && typeof preMod.preloadHatchSettingsData === 'function') {
                    const preRes = await Promise.resolve(preMod.preloadHatchSettingsData()).catch(() => null);
                    if (preRes) inspectNode(preRes, 0, true);
                  }
                } catch (_) {}
              }
              if (!window.__maamApiEmail && tp && tp.c && typeof tp.c.values === 'function' && Array.isArray(window.__maamProfileActions)) {
                let callServerFn = null;
                for (const m of tp.c.values()) {
                  if (m && m.exports && typeof m.exports.callServer === 'function') {
                    callServerFn = m.exports.callServer;
                    break;
                  }
                }
                if (callServerFn) {
                  for (const aid of window.__maamProfileActions) {
                    try {
                      const actRes = await callServerFn(aid, []);
                      inspectNode(actRes, 0, true);
                      if (window.__maamApiEmail) break;
                    } catch (_) {}
                  }
                }
              }
            }
          }
        }
      } catch (_) {
      } finally {
        window.__maamIdFetchInflight = false;
        window.__maamIdFetchDone = true;
      }
    };

    const inspectNode = (node, depth, skipSub) => {
      if (!node || typeof node !== 'object' || depth > 10) return;
      if (Array.isArray(node)) {
        for (let i = 0; i < node.length && i < 30; i++) inspectNode(node[i], depth + 1, skipSub);
        return;
      }
      const EMAIL_KEYS = [
        'email', 'user_email', 'userEmail', 'contact_email', 'contactEmail',
        'primary_email', 'primaryEmail', 'email_address', 'emailAddress',
        'billing_email', 'billingEmail', 'customer_email', 'customerEmail',
        'login_email', 'loginEmail', 'account_email', 'accountEmail',
        'verified_email', 'verifiedEmail', 'display_label', 'displayLabel',
        'contact_point', 'contactPoint'
      ];
      for (const ek of EMAIL_KEYS) {
        if (typeof node[ek] === 'string' && node[ek].includes('@')) {
          recordApiEmail(node[ek]);
        }
      }
      const hasUserHint =
        'user_id' in node || 'userId' in node || 'viewer_id' in node || 'viewerId' in node ||
        'profile_pic_url' in node || 'profilePictureUrl' in node || 'avatar_url' in node || 'avatarUrl' in node ||
        'linked_accounts' in node || 'linkedAccounts' in node || 'account_type' in node || 'accountType' in node ||
        'subscriptionType' in node || 'subscription_type' in node || 'is_verified' in node ||
        'username' in node || 'user_name' in node || 'email' in node;
      if (hasUserHint) {
        const NAME_KEYS = ['display_name', 'displayName', 'full_name', 'fullName', 'username', 'user_name', 'userName', 'preferred_username', 'name'];
        for (const nk of NAME_KEYS) {
          if (typeof node[nk] === 'string') {
            recordApiName(node[nk]);
          }
        }
      }
      if (typeof node.avatar_url === 'string' && node.avatar_url.startsWith('http')) {
        window.__maamApiAvatar = node.avatar_url;
      }
      if (typeof node.picture === 'string' && node.picture.startsWith('http')) {
        window.__maamApiAvatar = node.picture;
      }
      let nextSkipSub = Boolean(skipSub);
      if (!nextSkipSub) {
        const mapped = mapSub(node);
        if (mapped) {
          saveDetail(mapped);
          nextSkipSub = true;
        }
      }
      const remCredits = node.remaining_credits ?? node.credits_remaining ?? node.credit_balance ?? node.quota_remaining;
      if (typeof remCredits === 'number' && Number.isFinite(remCredits)) {
        const total = node.total_credits ?? node.monthly_limit ?? node.quota_limit;
        if (typeof total === 'number' && total > 0) {
          const pct = Math.max(0, Math.min(100, Math.round((remCredits / total) * 100)));
          setQuota('剩余 ' + remCredits + '/' + total, pct);
        } else {
          setQuota('剩余额度 ' + remCredits, null);
        }
      }
      if (node.code === 'out_of_credits' || node.error === 'rate_limit_exceeded') {
        setQuota('Quota depleted', 0);
      }
      const keys = Object.keys(node);
      for (let i = 0; i < keys.length && i < 40; i++) {
        const v = node[keys[i]];
        if (v && typeof v === 'object') inspectNode(v, depth + 1, nextSkipSub);
      }
    };
    const inspectText = (txt) => {
      if (typeof txt !== 'string' || txt.length < 2 || txt.length > 600000) return;
      if (txt.includes('@')) {
        const keyedEmailRe = /\\?"(?:email|user_email|userEmail|contact_email|contactEmail|primary_email|primaryEmail|email_address|emailAddress|billing_email|billingEmail|customer_email|customerEmail|login_email|loginEmail|account_email|accountEmail|verified_email|verifiedEmail|display_label|displayLabel|contact_point|contactPoint)\\?"\s*:\s*\\?"([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})\\?"/g;
        let km;
        while ((km = keyedEmailRe.exec(txt)) !== null) {
          recordApiEmail(km[1]);
        }
      }
      if (/email|user_id|userId|viewer|profile|account/i.test(txt)) {
        const keyedNameRe = /\\?"(?:display_name|displayName|full_name|fullName|preferred_username)\\?"\s*:\s*\\?"([^"\\]{2,60})\\?"/g;
        let nm;
        while ((nm = keyedNameRe.exec(txt)) !== null) {
          recordApiName(nm[1]);
        }
      }
      if (!/percent_used|percentUsed|quota_status|quotaStatus|usage_row_value_label|usageRowValueLabel|topup_balance|topupBalance|remaining_credits|credit_balance|out_of_credits|UNLIMITED|email|credits|allowance|balance|usage|tokens|billing|subscription|harkTokens|dailyLimit|dailyUsed|poolLimit|poolUsed|dailyResetsAt|poolResetsAt|meters|displayName|display_name/i.test(txt)) return;
      try {
        inspectNode(JSON.parse(txt), 0, false);
        return;
      } catch (_) {}
      const lines = txt.split('\n');
      for (let i = 0; i < lines.length && i < 400; i++) {
        const idx = lines[i].indexOf(':');
        if (idx > 0 && idx < 16) {
          const rest = lines[i].slice(idx + 1).trim();
          if (rest.startsWith('{') || rest.startsWith('[')) {
            try { inspectNode(JSON.parse(rest), 0, false); } catch (_) {}
          }
        }
      }
    };
    if (isPrimaryHost) {
      if (typeof origFetch === 'function') {
        window.fetch = function(...args) {
          const reqUrl = typeof args[0] === 'string' ? args[0] : (args[0] && args[0].url) || '';
          const p = origFetch.apply(this, args);
          p.then((res) => {
            try {
              let reqPath = null;
              try {
                const parsedUrl = new URL(reqUrl || '', location.origin);
                if (parsedUrl.origin === location.origin && parsedUrl.pathname.startsWith('/api/')) {
                  reqPath = parsedUrl.pathname;
                }
              } catch (_) {}
              const ct = (res.headers && res.headers.get('content-type')) || '';
              if (
                reqPath &&
                reqPath !== '/api/logout' &&
                reqPath !== '/api/auth/checkpoint/terminate' &&
                !reqPath.startsWith('/api/falco') &&
                reqPath !== '/api/metaconfig/exposure_sessionless' &&
                !reqPath.startsWith('/api/marketing/')
              ) {
                if (res.status === 401) {
                  markAuthInvalid('http_401');
                } else if (res.status === 403 && initHost.endsWith('muse.ai')) {
                  if (reqPath === '/api/auth/check') {
                    markAuthInvalid('auth_check_403');
                  } else if (ct.includes('json')) {
                    res.clone().text().then((body) => {
                      if (/"type"\s*:\s*"checkpointRequired"|"error"\s*:\s*"Authentication required"|"invalidation"/i.test(body)) {
                        markAuthInvalid('auth_403');
                      }
                    }).catch(() => {});
                  }
                } else if (
                  res.ok &&
                  (reqPath === '/api/client-version' ||
                    reqPath === '/api/auth/check' ||
                    reqPath === '/api/profile' ||
                    reqPath === '/api/billing/summary' ||
                    reqPath.startsWith('/api/messages') ||
                    reqPath.startsWith('/api/chat'))
                ) {
                  markAuthOk();
                }
              }
              if (!ct.includes('event-stream') && !ct.includes('ndjson') && (ct.includes('json') || ct.includes('text'))) {
                res.clone().text().then(inspectText).catch(() => {});
              }
              if (reqUrl.includes('/api/chat') || reqUrl.includes('/api/messages')) {
                clearTimeout(window.__maamPostChatTimer);
                window.__maamPostChatTimer = setTimeout(() => {
                  if (typeof window.__maamRefreshBackendQuota === 'function') {
                    window.__maamRefreshBackendQuota(true);
                  }
                }, 8000);
              }
            } catch (_) {}
          }).catch(() => {});
          return p;
        };
      }
      const OrigWS = window.WebSocket;
      if (typeof OrigWS === 'function') {
        window.WebSocket = new Proxy(OrigWS, {
          construct(target, args, newTarget) {
            const ws = Reflect.construct(target, args, newTarget);
            ws.addEventListener('message', (ev) => {
              if (typeof ev.data === 'string') inspectText(ev.data);
            });
            return ws;
          }
        });
      }

      if (!window.__maamAutoApproveWatcher) {
        window.__maamAutoApproveWatcher = true;
        const DANGER_RE = /删除|移除|注销|清空|退出登录|退出账户|确认退出|支付|付款|购买|订阅|充值|转账|密码|更改邮箱|停用|封禁|delete|remove|logout|log out|sign out|payment|purchase|billing|checkout|subscribe|charge|transfer|password|deactivate|reset account/i;
        const PROMPT_RE = /授权|审批|权限|批准|请求允许|请求执行|需要你的允许|需要您的允许|需要确认|是否(?:允许|批准|授权|同意)|要允许|你的助手想要|助手想要|想(?:要|使用|访问|运行|执行|打开|上传|下载|读取|写入|查看|获取|调用|连接|发送)|允许.{0,24}(?:此操作|此次|使用|运行|执行|访问|操作|工具|命令|浏览|上传|下载|读取|写入|修改|打开|连接|发送|调用|查看|获取)|approve|approval|authorize|permission|wants to|would like to|asks? to|needs approval|allow\s+.{0,32}\bto\b|allow\s+.{0,24}(?:this|use|run|execute|access|tool|command|browser|upload|download|read|write|open|connect|send)/i;
        const SINGLE_APPROVE_BTN_RE = /^(?:允许(?!.*(?:始终|总是))[^，。！？]{0,10}|本次允许|仅允许一次|批准(?!.*(?:始终|总是))[^，。！？]{0,10}|同意(?:并继续)?|确认授权|确认允许|继续执行|allow(?!\s+always)(?:\s+(?:once|this\s+time|this\s+\w+|access|upload|download|run|execute|use))?|approve(?:\s+(?:once|this\s+\w+))?|authorize|grant)$/i;
        const APPROVE_BTN_RE = /^(?:(?:允许|本次允许|仅允许一次|始终允许|总是允许|批准|始终批准)[^，。！？]{0,18}|同意(?:并继续)?|确认授权|确认允许|继续执行|(?:allow|always\s+allow|approve|always\s+approve|authorize|grant)(?:\s+[\w\s'-]{1,28})?)$/i;
        const REJECT_BTN_RE = /^(?:(?:拒绝|不允许|不批准)[^，。！？]{0,12}|取消|暂不|以后再说|(?:deny|decline|reject|don't allow|do not allow)(?:\s+[\w\s'-]{1,20})?|cancel|block|never|not now)$/i;
        const MODAL_BOX_SEL = '[role="dialog"], [role="alertdialog"], [aria-modal="true"], dialog, [class*="permission" i], [class*="approval" i], [class*="approve" i], [class*="authorize" i], [class*="consent" i], [data-testid*="approv" i], [data-testid*="permiss" i]';
        const clickedAt = new WeakMap();
        const getBtnLabel = (b) => ((b.innerText || b.textContent || b.getAttribute('aria-label') || '') + '').replace(/\s+/g, ' ').trim();
        const isElVisible = (el) => {
          if (!el || !el.isConnected) return false;
          const r = el.getBoundingClientRect();
          if (r.width < 16 || r.height < 14) return false;
          let cur = el;
          for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
            try {
              const st = getComputedStyle(cur);
              if (st.display === 'none' || st.visibility === 'hidden') return false;
              const op = parseFloat(st.opacity);
              if (!Number.isNaN(op) && op < 0.05) return false;
            } catch (_) {}
            cur = cur.parentElement;
          }
          return true;
        };
        const isActionableBtn = (b) => {
          if (!isElVisible(b) || b.disabled || b.getAttribute('aria-disabled') === 'true') return false;
          if (b.closest && b.closest('nav, aside')) return false;
          return true;
        };
        const findApprovalTarget = () => {
          try {
            const allBtns = document.querySelectorAll('button, [role="button"]');
            for (const b of allBtns) {
              if (!isActionableBtn(b)) continue;
              const lbl = getBtnLabel(b);
              if (!APPROVE_BTN_RE.test(lbl) || REJECT_BTN_RE.test(lbl) || DANGER_RE.test(lbl)) continue;
              let cur = b.parentElement;
              for (let step = 0; step < 7 && cur && cur !== document.body && cur !== document.documentElement; step++) {
                if (cur.tagName === 'NAV' || cur.tagName === 'ASIDE') break;
                let isModal = false;
                try { isModal = typeof cur.matches === 'function' && cur.matches(MODAL_BOX_SEL); } catch (_) {}
                if (!isModal && cur.querySelector && cur.querySelector('textarea, [contenteditable="true"]')) break;
                const txt = (cur.innerText || cur.textContent || '').trim();
                if (txt.length > 2200) break;
                if (txt.length >= 6) {
                  if (DANGER_RE.test(txt)) break;
                  if (PROMPT_RE.test(txt)) {
                    const boxBtns = Array.from(cur.querySelectorAll('button, [role="button"]')).filter(isActionableBtn);
                    const hasReject = boxBtns.some((x) => REJECT_BTN_RE.test(getBtnLabel(x)));
                    if (hasReject || isModal) {
                      const approveBtns = boxBtns.filter((x) => {
                        const xLbl = getBtnLabel(x);
                        return APPROVE_BTN_RE.test(xLbl) && !REJECT_BTN_RE.test(xLbl) && !DANGER_RE.test(xLbl);
                      });
                      if (approveBtns.length > 0) {
                        const picked = approveBtns.find((x) => SINGLE_APPROVE_BTN_RE.test(getBtnLabel(x))) || approveBtns[0];
                        return { box: cur, button: picked, text: txt };
                      }
                    }
                  }
                }
                cur = cur.parentElement;
              }
            }
          } catch (_) {}
          return null;
        };
        window.__maamFindAuthPrompt = () => {
          const target = findApprovalTarget();
          return target ? target.text.slice(0, 140) : '';
        };
        window.__maamScanAutoApprove = () => {
          if (window.__maamAutoApprove !== true) return false;
          const target = findApprovalTarget();
          if (!target) return false;
          const now = Date.now();
          const last = clickedAt.get(target.button) || 0;
          if (now - last < 4000) return false;
          clickedAt.set(target.button, now);
          target.button.click();
          return true;
        };
        let approveTimer = null;
        const scheduleAutoApproveScan = () => {
          if (window.__maamAutoApprove !== true || approveTimer) return;
          approveTimer = setTimeout(() => {
            approveTimer = null;
            window.__maamScanAutoApprove();
          }, 250);
        };
        const startObserver = () => {
          try {
            if (!document.documentElement) return;
            const obs = new MutationObserver(() => scheduleAutoApproveScan());
            obs.observe(document.documentElement, { childList: true, subtree: true });
          } catch (_) {}
        };
        if (document.documentElement) {
          startObserver();
        } else {
          document.addEventListener('DOMContentLoaded', startObserver, { once: true });
        }
        setInterval(() => {
          if (window.__maamAutoApprove === true) {
            window.__maamScanAutoApprove();
          }
        }, 1200);
      }
    }
    const capture = (el) => {
      if (!el || typeof el.value !== 'string') return;
      const v = el.value.trim();
      if (/^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/.test(v)) {
        window.__maamTypedIdentity = v.toLowerCase();
        try { sessionStorage.setItem('__maam_typed_identity__', window.__maamTypedIdentity); } catch (_) {}
      } else if (/^\+?\d[\d\s-]{6,16}$/.test(v) && (el.type === 'tel' || /手机|phone|tel/i.test(el.placeholder || el.name || ''))) {
        window.__maamTypedPhone = v.replace(/\s+/g, '');
        try { sessionStorage.setItem('__maam_typed_phone__', window.__maamTypedPhone); } catch (_) {}
      }
    };
    document.addEventListener('input', (e) => capture(e.target), true);
    document.addEventListener('change', (e) => capture(e.target), true);
    document.addEventListener('blur', (e) => capture(e.target), true);
    document.addEventListener('submit', () => {
      document.querySelectorAll('input').forEach(capture);
    }, true);
    window.addEventListener('keydown', (e) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey) {
        const m = e.key && e.key.match(/^[1-9]$/);
        if (m) {
          window.__maamPendingHotkey = parseInt(m[0], 10);
        }
      }
    }, true);
  } catch (_) {}
})();"#;

#[cfg(desktop)]
const DOM_PAINT_READY_JS: &str = r#"(() => {
  try {
    if (location.href === 'about:blank') return false;
    if (document.readyState !== 'complete') return false;
    const body = document.body;
    if (!body) return false;
    const sheets = document.querySelectorAll('link[rel~="stylesheet"]');
    for (const l of sheets) {
      if (l.disabled) continue;
      if ((l.media || '').toLowerCase() === 'print') continue;
      if (l.sheet === null) return false;
    }
    const host = (location.hostname || '').toLowerCase();
    const isHark = host === 'app.hark.com' || host.endsWith('.hark.com');
    const isMuse =
      host === 'muse.ai' ||
      host.endsWith('.muse.ai') ||
      host === 'meta.com' ||
      host.endsWith('.meta.com');
    const vw = window.innerWidth || document.documentElement.clientWidth || 0;
    const vh = window.innerHeight || document.documentElement.clientHeight || 0;
    const isTrulyVisible = (el) => {
      if (!el) return false;
      const r = el.getBoundingClientRect();
      if (r.width < 16 || r.height < 16) return false;
      if (vw > 0 && vh > 0 && (r.bottom <= 0 || r.right <= 0 || r.top >= vh || r.left >= vw)) {
        return false;
      }
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
          const op = parseFloat(st.opacity);
          if (!Number.isNaN(op) && op < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };
    const isLoadingPlaceholderEl = (el) => {
      if (!el) return false;
      const role = (el.getAttribute && el.getAttribute('role')) || '';
      const busy = (el.getAttribute && el.getAttribute('aria-busy')) || '';
      const aria = (el.getAttribute && el.getAttribute('aria-label')) || '';
      const cls =
        typeof el.className === 'string'
          ? el.className
          : (el.getAttribute && el.getAttribute('class')) || '';
      if (role === 'progressbar' || busy === 'true') return true;
      return /spinner|skeleton|shimmer|loader|loading/i.test(`${cls} ${aria}`);
    };
    const isLoadingOnlyText = (t) => {
      const s = (t || '').trim();
      if (!s) return true;
      return /^(loading|加载中|正在加载|请稍候|please wait|\.{1,3}|meta|muse|hark)$/i.test(s);
    };

    let domReady = false;
    if (isHark) {
      const root = document.getElementById('root');
      const rootText = root ? (root.innerText || '').trim() : '';
      if (!(rootText.length < 4 && (body.innerText || '').trim() === 'Unauthorized')) {
        let ready = true;
        if (root) {
          let rootReady = rootText.length >= 2;
          if (!rootReady) {
            const uiEls = root.querySelectorAll('input, button, textarea, [contenteditable="true"], svg, img');
            for (let i = 0; i < uiEls.length && i < 30; i++) {
              if (isTrulyVisible(uiEls[i])) {
                rootReady = true;
                break;
              }
            }
          }
          if (!rootReady) ready = false;
        }
        if (ready && window.__harkSky && !window.__harkSky.painted) {
          const gl = document.getElementById('gl');
          if (gl) {
            try {
              if (getComputedStyle(gl).visibility === 'hidden') ready = false;
            } catch (_) {}
          }
        }
        domReady = ready;
      }
    } else {
      const bodyText = (body.innerText || '').trim();
      const interactiveSel = 'input:not([type="hidden"]), textarea, [contenteditable="true"], button, [role="button"], a[href]';
      const hasVisibleInteractive = (sel) => {
        const els = body.querySelectorAll(sel);
        for (let i = 0; i < els.length && i < 60; i++) {
          const el = els[i];
          if (isLoadingPlaceholderEl(el)) continue;
          if (el.tagName === 'IFRAME') {
            const r = el.getBoundingClientRect();
            if (r.width < 100 || r.height < 100) continue;
          }
          if (isTrulyVisible(el)) return true;
        }
        return false;
      };
      if (isMuse) {
        const hasControl = hasVisibleInteractive(interactiveSel);
        const hasInput = hasVisibleInteractive('input:not([type="hidden"]), textarea, [contenteditable="true"]');
        const hasText = bodyText.length >= 12 && !isLoadingOnlyText(bodyText);
        domReady = hasControl && (hasText || hasInput);
      } else {
        const hasContentText = bodyText.length >= 20 && !isLoadingOnlyText(bodyText);
        const hasLayoutOrContent = hasVisibleInteractive('main, article, section, form, nav, header, input:not([type="hidden"]), textarea, [contenteditable="true"], button, [role="button"], a[href], img, canvas, video, iframe');
        const hasDirectControl = hasVisibleInteractive('input:not([type="hidden"]), textarea, [contenteditable="true"], button, [role="button"], iframe');
        domReady = (hasContentText && hasLayoutOrContent) || hasDirectControl;
      }
    }

    if (!domReady) {
      window.__maamReadySince = 0;
      window.__maamReadyFrames = 0;
      return false;
    }

    if (!window.__maamReadySince) {
      window.__maamReadySince = Date.now();
      window.__maamReadyFrames = 0;
      if (typeof requestAnimationFrame === 'function') {
        const step = () => {
          window.__maamReadyFrames = (window.__maamReadyFrames || 0) + 1;
          if ((window.__maamReadyFrames || 0) < 3) {
            requestAnimationFrame(step);
          }
        };
        requestAnimationFrame(step);
      }
      return false;
    }

    if (document.visibilityState === 'hidden') {
      return (Date.now() - window.__maamReadySince) >= 350;
    }
    return (window.__maamReadyFrames || 0) >= 2 || (Date.now() - window.__maamReadySince) >= 600;
  } catch (_) {
    return false;
  }
})()"#;

#[cfg(desktop)]
const IDENTITY_PROBE_JS: &str = r#"(() => {
  try {
    const pendingHotkey = window.__maamPendingHotkey || null;
    window.__maamPendingHotkey = null;
    document.querySelectorAll('input').forEach((el) => {
      const v = (el.value || '').trim();
      if (/^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/.test(v)) {
        window.__maamTypedIdentity = v.toLowerCase();
        try { sessionStorage.setItem('__maam_typed_identity__', window.__maamTypedIdentity); } catch (_) {}
      }
    });
    let typed = window.__maamTypedIdentity || '';
    let phone = window.__maamTypedPhone || '';
    try {
      typed = typed || sessionStorage.getItem('__maam_typed_identity__') || '';
      phone = phone || sessionStorage.getItem('__maam_typed_phone__') || '';
    } catch (_) {}
    const host = (location.hostname || '').toLowerCase();
    const path = (location.pathname || '').toLowerCase();
    const isPrimary =
      host === 'muse.ai' || host.endsWith('.muse.ai') ||
      host === 'app.hark.com' || host.endsWith('.hark.com');
    const isMetaAccountCenter = host === 'accountscenter.meta.com';
    const isExternalLoginHost =
      host === 'meta.com' || host.endsWith('.meta.com') ||
      host === 'facebook.com' || host.endsWith('.facebook.com') ||
      host === 'appleid.apple.com';
    const makeLoggedOut = (staleSession = false) => {
      window.__maamQuota = null;
      window.__maamQuotaDetail = null;
      try {
        localStorage.removeItem('__maam_cached_quota__');
        localStorage.removeItem('__maam_cached_quota_detail_v2__');
      } catch (_) {}
      return JSON.stringify({ auth: false, loggedOut: true, staleSession: Boolean(staleSession), pageError: '', typed: typed || phone, email: '', displayName: '', avatar: '', quotaText: '', quotaPct: null, quotaDetail: null, pendingHotkey });
    };
    const makeLoading = () => JSON.stringify({ auth: false, loggedOut: false, staleSession: false, pageError: '', typed: typed || phone, email: '', displayName: '', avatar: '', quotaText: '', quotaPct: null, quotaDetail: null, pendingHotkey });
    const makePageError = (msg) => JSON.stringify({ auth: false, loggedOut: false, staleSession: false, pageError: String(msg || ''), typed: typed || phone, email: '', displayName: '', avatar: '', quotaText: '', quotaPct: null, quotaDetail: null, pendingHotkey });
    if (document.readyState === 'loading') {
      return makeLoading();
    }
    const loginInputs = document.querySelectorAll('input[type="email"], input[type="password"], input[autocomplete="one-time-code"], input[placeholder*="邮箱"], input[placeholder*="手机"], input[placeholder*="email" i], input[placeholder*="phone" i]');
    const hasVisibleLoginInput = () => {
      for (const inp of loginInputs) {
        if (inp.disabled || inp.readOnly) continue;
        const r = inp.getBoundingClientRect();
        if (r.width > 20 && r.height > 15) return true;
      }
      return false;
    };
    const hasChatComposer = Array.from(document.querySelectorAll('textarea, [contenteditable="true"]')).some((el) => {
      if (el.disabled || el.readOnly) return false;
      const r = el.getBoundingClientRect();
      return r.width > 120 && r.height > 20;
    });
    if (isExternalLoginHost) {
      if (/^\/(login|signin|signup|auth)\b/.test(path) || hasVisibleLoginInput()) {
        return makeLoggedOut(false);
      }
      if (isMetaAccountCenter && document.readyState !== 'loading') {
        const metaBodyText = (document.body && document.body.innerText) || '';
        const badMeta = /^(support|privacy|help|legal|press|noreply|no-reply|info|security|contact|abuse|feedback|sales|terms|hello|team|admin|postmaster|notifications)@/i;
        const providerMeta = /@(?:[a-z0-9-]+\.)?(?:muse\.ai|hark\.com|meta\.com|facebook\.com)$/i;
        const metaEmailRe = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g;
        const metaCands = [];
        const pushMetaRaw = (raw) => {
          if (typeof raw !== 'string' || !raw.includes('@')) return;
          const m = raw.match(metaEmailRe);
          if (m) metaCands.push(...m);
        };
        document.querySelectorAll('[data-email],[aria-label*="@"],[title*="@"],a[href^="mailto:"]').forEach((el) => {
          const raw = (el.getAttribute('data-email') || el.getAttribute('aria-label') || el.getAttribute('title') || el.getAttribute('href') || '').replace(/^mailto:/i, '');
          pushMetaRaw(raw);
        });
        pushMetaRaw(metaBodyText);
        if (metaCands.length === 0) {
          document.querySelectorAll('script[type="application/json"]').forEach((sc) => {
            const txt = sc.textContent || '';
            if (txt && txt.includes('@') && txt.length < 200000) pushMetaRaw(txt);
          });
        }
        const validMeta = [];
        for (const c of metaCands) {
          const clean = String(c || '').toLowerCase();
          if (clean && !badMeta.test(clean) && !clean.endsWith('@example.com') && !clean.endsWith('@sentry.io') && !clean.endsWith('@wixpress.com')) {
            if (!validMeta.includes(clean)) validMeta.push(clean);
          }
        }
        const metaEmail = validMeta.find((e) => !providerMeta.test(e)) || validMeta[0] || '';
        if (!metaEmail && /Log in to Meta|登录\s*Meta|Create a Meta account|创建\s*Meta\s*账户|Sign up for Meta/i.test(metaBodyText)) {
          return makeLoggedOut(false);
        }
        return JSON.stringify({ auth: false, loggedOut: false, staleSession: false, pageError: '', typed: typed || phone, email: metaEmail, displayName: '', avatar: '', quotaText: '', quotaPct: null, quotaDetail: null, pendingHotkey });
      }
      return makeLoading();
    }
    if (!isPrimary) {
      return makeLoading();
    }
    if (/^\/(login|signin|signup|auth)\b/.test(path)) {
      return makeLoggedOut(false);
    }
    if (!hasChatComposer && !/^\/(settings|profile|account)\b/.test(path) && hasVisibleLoginInput()) {
      return makeLoggedOut(false);
    }
    const bodyText = (document.body && document.body.innerText) || '';
    if (!hasChatComposer && /登录或创建账户|Log in or sign up|Sign in to|登录以继续|通过 Google 继续|Continue with Google|通过 Apple 继续|Continue with Apple/i.test(bodyText)) {
      return makeLoggedOut(false);
    }
    if (host.endsWith('hark.com') && bodyText.trim() === 'Unauthorized') {
      return makeLoggedOut(true);
    }
    if (document.readyState !== 'complete') {
      return makeLoading();
    }
    if (host.endsWith('muse.ai') && /^\/blocked\b/.test(path)) {
      return makePageError('账号需安全验证或受限，请在右侧处理');
    }
    if (hasChatComposer && window.__maamAuthCheck && window.__maamAuthCheck.status === 'invalid') {
      if (Date.now() - (window.__maamAuthCheck.checkedAt || 0) > 5000) {
        window.__maamAuthCheck.status = 'ok';
        window.__maamAuthCheck.reason = '';
        window.__maamAuthCheck.checkedAt = Date.now();
      }
    }
    if (!hasChatComposer) {
      const ac = window.__maamAuthCheck;
      if (ac && ac.status === 'invalid') {
        return makeLoggedOut(true);
      }
      const hasErrorActionBtn = Array.from(document.querySelectorAll('button, a, [role="button"]')).some((el) => {
        if (!el || !el.isConnected) return false;
        const r = el.getBoundingClientRect();
        if (r.width < 16 || r.height < 12) return false;
        try {
          const st = getComputedStyle(el);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
        } catch (_) {}
        const label = ((el.innerText || el.getAttribute('aria-label') || '') + '').trim();
        return /^(?:重试|Try again|退出|Log out)$/i.test(label);
      });
      const isErrorScreen =
        /(?:^|\n)\s*(?:出错了|Something went wrong|Application Error|应用出错)\s*(?:\n|$)/.test(bodyText) &&
        hasErrorActionBtn;
      const isDegradedMuseSettings =
        host.endsWith('muse.ai') &&
        /(?:Meta\s*账户|Meta\s*Account)/i.test(bodyText) &&
        /(?:帮助与支持|Help\s*&\s*support)/i.test(bodyText) &&
        /(?:^|\n)\s*(?:退出|Log out)\s*(?:\n|$)/i.test(bodyText) &&
        !/订阅|使用量|用量|Subscription|Usage|Billing/i.test(bodyText);
      if (typeof window.__maamVerifySession === 'function') {
        window.__maamVerifySession(false);
      }
      if (isErrorScreen || isDegradedMuseSettings) {
        if (!ac || ac.inflight || !ac.checkedAt) {
          return makeLoading();
        }
        if (ac.status === 'invalid') {
          return makeLoggedOut(true);
        }
        return makePageError(isDegradedMuseSettings ? '登录态异常，请退出或清理重登' : '页面出错，请检查代理或清理重登');
      }
      if (!/^\/(settings|profile|account)\b/.test(path) && (!ac || ac.status === 'unknown') && (!ac || ac.inflight || !ac.checkedAt)) {
        return makeLoading();
      }
    }

    const extractAvatar = () => {
      const selectors = [
        '[data-hatch-media-owner="avatar-layer"]',
        '[data-hatch-avatar-host]:not([data-hatch-avatar-host-hidden="true"]) img',
        '[data-hatch-avatar-host]:not([data-hatch-avatar-host-hidden="true"]) video',
        '[data-hatch-avatar-host] img',
        '[data-hatch-avatar-host] video',
        '[data-hatch-avatar-media-static] img',
        '[data-hatch-avatar-media-static] video',
        '[class*="hatch-avatar"] img',
        '[class*="hatch-avatar"] video',
        '[data-avatar-parity-side] img',
        '[data-testid*="avatar" i] img',
        '[data-testid*="user" i] img',
        'header [class*="avatar" i] img',
        'nav [class*="avatar" i] img',
        'button[aria-label*="Profile" i] img',
        'button[aria-label*="Account" i] img',
        'button[aria-label*="User" i] img',
        'button[aria-label*="Settings" i] img',
        'img[src^="blob:"]',
        '[class*="avatar" i] img',
        'img[alt*="avatar" i]',
        'img[src*="avatar" i]'
      ];
      let target = null;
      for (const sel of selectors) {
        const isHatchAvatar = sel.includes('hatch');
        const list = document.querySelectorAll(sel);
        for (const el of list) {
          if (el.closest && el.closest('.landing-rebus-avatar-slot')) continue;
          const r = el.getBoundingClientRect();
          if (!isHatchAvatar && (r.width < 16 || r.height < 16 || r.width > 520 || Math.abs(r.width - r.height) > 64)) continue;
          if (isHatchAvatar && r.width > 520) continue;
          if (el.tagName === 'IMG' && el.complete && el.naturalWidth > 0) {
            target = el;
            break;
          }
          if (el.tagName === 'VIDEO' && el.readyState >= 2 && el.videoWidth > 0) {
            target = el;
            break;
          }
        }
        if (target) break;
      }
      if (!target) return window.__maamApiAvatar || window.__maamAvatarData || '';
      const srcKey = target.currentSrc || target.src || target.poster || '';
      if (srcKey && window.__maamAvatarKey === srcKey && window.__maamAvatarData) {
        return window.__maamAvatarData;
      }
      try {
        const sw = target.tagName === 'VIDEO' ? target.videoWidth : target.naturalWidth;
        const sh = target.tagName === 'VIDEO' ? target.videoHeight : target.naturalHeight;
        if (!sw || !sh) return window.__maamApiAvatar || window.__maamAvatarData || '';
        const size = 72;
        const canvas = document.createElement('canvas');
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext('2d');
        if (!ctx) return '';
        const side = Math.min(sw, sh);
        const sx = (sw - side) / 2;
        const sy = (sh - side) / 2;
        ctx.drawImage(target, sx, sy, side, side, 0, 0, size, size);
        const px = ctx.getImageData(0, 0, size, size).data;
        let visible = 0;
        for (let i = 3; i < px.length; i += 64) {
          if (px[i] > 12) { visible++; if (visible >= 4) break; }
        }
        if (visible < 4) return window.__maamApiAvatar || window.__maamAvatarData || '';
        let dataUrl = canvas.toDataURL('image/webp', 0.86);
        if (!dataUrl || dataUrl.length < 32) {
          dataUrl = canvas.toDataURL('image/png');
        }
        if (dataUrl && dataUrl.startsWith('data:image/') && dataUrl.length < 60000) {
          window.__maamAvatarKey = srcKey;
          window.__maamAvatarData = dataUrl;
          return dataUrl;
        }
      } catch (_) {}
      return window.__maamApiAvatar || window.__maamAvatarData || '';
    };

    const extractQuota = () => {
      if (isPrimary) {
        if (typeof window.__maamRefreshBackendQuota === 'function') {
          window.__maamRefreshBackendQuota(false);
        }
      }

      if (window.__maamQuotaDetail && window.__maamQuotaDetail.planLabel) {
        const d = window.__maamQuotaDetail;
        const rem = typeof d.planUsedPercent === 'number' ? Math.max(0, 100 - d.planUsedPercent) : null;
        return { text: d.planValue || d.planLabel, pct: rem, detail: d };
      }

      try {
        const cachedV2 = localStorage.getItem('__maam_cached_quota_detail_v2__');
        if (cachedV2) {
          const d = JSON.parse(cachedV2);
          if (d && d.planLabel) {
            window.__maamQuotaDetail = d;
            const rem = typeof d.planUsedPercent === 'number' ? Math.max(0, 100 - d.planUsedPercent) : null;
            return { text: d.planValue || d.planLabel, pct: rem, detail: d };
          }
        }
      } catch (_) {}

      if (host.endsWith('hark.com')) {
        const mDaily = bodyText.match(/(?:已使用每日上限的\s*(\d+(?:\.\d+)?)\s*%|(\d+(?:\.\d+)?)\s*%\s*of\s*daily\s*limit\s*used)/i);
        const mMonthly = bodyText.match(/(?:已使用每月上限的\s*(\d+(?:\.\d+)?)\s*%|(\d+(?:\.\d+)?)\s*%\s*of\s*monthly\s*limit\s*used)/i);
        if (mDaily || mMonthly) {
          const dPct = mDaily ? Math.max(0, Math.min(100, Math.round(parseFloat(mDaily[1] || mDaily[2])))) : null;
          const mPct = mMonthly ? Math.max(0, Math.min(100, Math.round(parseFloat(mMonthly[1] || mMonthly[2])))) : null;
          const mDailyReset = bodyText.match(/(?:每日重置|Daily reset)[\s:：·]*([^\n\r]{2,28})/i);
          const mMonthlyReset = bodyText.match(/(?:每月重置|Monthly reset)[\s:：·]*([^\n\r]{2,28})/i);
          const fmtDomReset = (m) => {
            if (!m || !m[1]) return null;
            const s = m[1].trim();
            return s ? (s.endsWith('重置') ? s : (s + ' 重置')) : null;
          };
          const d = dPct !== null
            ? {
                planLabel: '每日上限',
                planValue: 'Used ' + dPct + '%',
                planSubtext: fmtDomReset(mDailyReset),
                planUsedPercent: dPct,
                extraLabel: mPct !== null ? '每月上限' : null,
                extraValue: mPct !== null ? ('Used ' + mPct + '%') : null,
                extraSubtext: mPct !== null ? fmtDomReset(mMonthlyReset) : null,
                extraUsedPercent: mPct
              }
            : {
                planLabel: '每月上限',
                planValue: 'Used ' + mPct + '%',
                planSubtext: fmtDomReset(mMonthlyReset),
                planUsedPercent: mPct,
                extraLabel: null,
                extraValue: null,
                extraSubtext: null,
                extraUsedPercent: null
              };
          if (typeof window.__maamSaveDetail === 'function') window.__maamSaveDetail(d);
          return { text: d.planValue, pct: 100 - d.planUsedPercent, detail: d };
        }
        const creditEls = document.querySelectorAll('[class*="credit" i], [class*="usage" i], [data-testid*="credit"], [data-testid*="usage"]');
        for (const el of creditEls) {
          const txt = (el.innerText || '').trim();
          const m = txt.match(/(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)/);
          if (m) {
            const cur = parseFloat(m[1]);
            const max = parseFloat(m[2]);
            if (max > 0) {
              const pct = Math.max(0, Math.min(100, Math.round((cur / max) * 100)));
              const d = {
                planLabel: 'Hark 用量',
                planValue: 'Used ' + pct + '%',
                planSubtext: '当前 ' + cur + ' / ' + max,
                planUsedPercent: pct,
                extraLabel: null,
                extraValue: null,
                extraSubtext: null,
                extraUsedPercent: null
              };
              if (typeof window.__maamSaveDetail === 'function') window.__maamSaveDetail(d);
              return { text: d.planValue, pct: 100 - pct, detail: d };
            }
          }
        }
      }

      if (window.__maamQuota && window.__maamQuota.text && window.__maamQuota.text !== '剩余额度 100%') {
        return { text: window.__maamQuota.text, pct: window.__maamQuota.pct, detail: null };
      }
      return { text: '正在同步额度…', pct: null, detail: null };
    };

    const avatar = extractAvatar();
    const quota = extractQuota();
    if (isPrimary) {
      if (!window.__maamApiEmail && typeof window.__maamRefreshIdentity === 'function') {
        window.__maamRefreshIdentity();
      }
      if (window.__maamAutoApprove === true && typeof window.__maamScanAutoApprove === 'function') {
        window.__maamScanAutoApprove();
      }
    }

    const bad = /^(support|privacy|help|legal|press|noreply|no-reply|info|security|contact|abuse|feedback|sales|terms|hello|team|admin|postmaster|notifications)@/i;
    const providerMail = /@(?:[a-z0-9-]+\.)?(?:muse\.ai|hark\.com|meta\.com|facebook\.com)$/i;
    const re = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g;
    const candidates = [];
    const pushRawEmails = (raw) => {
      if (typeof raw !== 'string' || !raw.includes('@')) return;
      const m = raw.match(re);
      if (m) candidates.push(...m);
    };
    if (window.__maamApiEmail) candidates.push(window.__maamApiEmail);

    try {
      if (Array.isArray(self.__next_f)) {
        const nfLen = self.__next_f.length;
        if (window.__maamNextFLen !== nfLen) {
          window.__maamNextFLen = nfLen;
          const nfEmails = [];
          let nfName = window.__maamNextFName || '';
          const badNameRe = /^(?:Meta|Muse|Hark|Button|Settings|Profile|Account|User|Avatar|Loading|Unknown|Hatch|Home|Chat|New Chat|Log in|Sign up|待登录账号|已登录账号|Account not signed in|Signed-in account|通用|设置)$/i;
          const keyedEmailRe = /\\?"(?:email|user_email|userEmail|contact_email|contactEmail|primary_email|primaryEmail|email_address|emailAddress|billing_email|billingEmail|customer_email|customerEmail|login_email|loginEmail|account_email|accountEmail|verified_email|verifiedEmail|display_label|displayLabel|contact_point|contactPoint)\\?"\s*:\s*\\?"([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})\\?"/g;
          const keyedNameRe = /\\?"(?:display_name|displayName|full_name|fullName|preferred_username)\\?"\s*:\s*\\?"([^"\\]{2,60})\\?"/g;
          for (let i = 0; i < nfLen; i++) {
            const entry = self.__next_f[i];
            const chunkStr = Array.isArray(entry) && typeof entry[1] === 'string'
              ? entry[1]
              : (typeof entry === 'string' ? entry : '');
            if (!chunkStr) continue;
            if (chunkStr.includes('@') && chunkStr.length < 180000) {
              let km;
              while ((km = keyedEmailRe.exec(chunkStr)) !== null) nfEmails.push(km[1]);
              if (nfEmails.length === 0) {
                const gm = chunkStr.match(re);
                if (gm) nfEmails.push(...gm);
              }
            }
            if (!nfName && chunkStr.length < 180000 && /user_id|userId|viewer|profile|account|email/i.test(chunkStr)) {
              const nm = keyedNameRe.exec(chunkStr);
              keyedNameRe.lastIndex = 0;
              if (nm && nm[1]) {
                const cn = nm[1].replace(/\s+/g, ' ').trim();
                if (cn.length >= 2 && cn.length <= 60 && !/^https?:\/\//i.test(cn) && !/^\d+$/.test(cn) && !badNameRe.test(cn)) {
                  nfName = cn;
                }
              }
            }
          }
          window.__maamNextFEmails = nfEmails;
          if (nfName) window.__maamNextFName = nfName;
        }
        if (Array.isArray(window.__maamNextFEmails)) {
          candidates.push(...window.__maamNextFEmails);
        }
      }
    } catch (_) {}

    try {
      if (window.__NEXT_DATA__ && typeof window.__NEXT_DATA__ === 'object') {
        const nd = JSON.stringify(window.__NEXT_DATA__);
        if (nd && nd.includes('@') && nd.length < 120000) pushRawEmails(nd);
      }
    } catch (_) {}

    document.querySelectorAll('[data-email],[aria-label*="@"],[title*="@"],a[href^="mailto:"]').forEach((el) => {
      const raw = (el.getAttribute('data-email') || el.getAttribute('aria-label') || el.getAttribute('title') || el.getAttribute('href') || '').replace(/^mailto:/i, '');
      pushRawEmails(raw);
    });
    const scanStorage = (store) => {
      try {
        for (let i = 0; i < store.length; i++) {
          const k = store.key(i);
          const val = k ? store.getItem(k) : '';
          if (!val || val.length >= 12000) continue;
          pushRawEmails(val);
          if (val.includes('%40')) {
            try { pushRawEmails(decodeURIComponent(val)); } catch (_) {}
          }
        }
      } catch (_) {}
    };
    scanStorage(localStorage);
    scanStorage(sessionStorage);
    try {
      const rawCookie = document.cookie || '';
      if (rawCookie.includes('@') || rawCookie.includes('%40')) {
        pushRawEmails(rawCookie);
        try { pushRawEmails(decodeURIComponent(rawCookie)); } catch (_) {}
      }
    } catch (_) {}
    pushRawEmails(bodyText);

    const validEmails = [];
    for (const c of candidates) {
      const clean = String(c || '').toLowerCase();
      if (clean && !bad.test(clean) && !clean.endsWith('@example.com') && !clean.endsWith('@sentry.io') && !clean.endsWith('@wixpress.com')) {
        if (!validEmails.includes(clean)) validEmails.push(clean);
      }
    }
    const foundEmail = validEmails.find((e) => !providerMail.test(e)) || validEmails[0] || '';
    const displayName = (window.__maamApiName || window.__maamNextFName || '').trim();
    return JSON.stringify({
      auth: true,
      loggedOut: false,
      staleSession: false,
      pageError: '',
      typed: typed || phone,
      email: foundEmail,
      displayName,
      avatar,
      quotaText: quota.text || '',
      quotaPct: quota.pct,
      quotaDetail: quota.detail || null,
      pendingHotkey
    });
  } catch (_) {
    return JSON.stringify({ auth: false, loggedOut: false, staleSession: false, pageError: '', typed: '', email: '', displayName: '', avatar: '', quotaText: '', quotaPct: null, quotaDetail: null, pendingHotkey: null });
  }
})()"#;

#[cfg(desktop)]
fn build_new_chat_click_js(adapter: &SiteAdapterConfig) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const sels = Array.isArray(adapter.newChatSelectors) ? adapter.newChatSelectors : [];
    for (const sel of sels) {
      if (!sel) continue;
      let nodes = [];
      try { nodes = Array.from(document.querySelectorAll(sel)); } catch (_) { continue; }
      for (const el of nodes) {
        if (!el || !el.isConnected || el.disabled || el.getAttribute('aria-disabled') === 'true') continue;
        const r = el.getBoundingClientRect();
        if (r.width < 8 || r.height < 8) continue;
        el.click();
        return JSON.stringify({ clicked: true, selector: sel });
      }
    }
    return JSON.stringify({ clicked: false });
  } catch (err) {
    return JSON.stringify({ clicked: false, error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
}

#[cfg(desktop)]
#[derive(Debug, Clone)]
struct GroupAttachment {
  filename: String,
  mime_type: String,
  content: String,
}

#[cfg(desktop)]
fn build_group_attach_file_js(adapter: &SiteAdapterConfig, attachment: &GroupAttachment) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  let name_json = serde_json::to_string(&attachment.filename).unwrap_or_else(|_| "\"draft.md\"".to_string());
  let mime_json = serde_json::to_string(&attachment.mime_type).unwrap_or_else(|_| "\"text/markdown\"".to_string());
  let content_json = serde_json::to_string(&attachment.content).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const fileName = __FILENAME__;
    const mimeType = __MIME__;
    const content = __CONTENT__;

    const isVisibleEl = (el, minW = 16, minH = 12) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < minW || r.height < minH) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    let composer = null;
    const compSels = Array.isArray(adapter.composerSelectors) ? adapter.composerSelectors : [];
    for (const sel of compSels) {
      if (!sel) continue;
      try {
        const list = Array.from(document.querySelectorAll(sel)).filter((el) => isVisibleEl(el, 80, 16));
        if (list.length > 0) {
          list.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
          composer = list[0];
          break;
        }
      } catch (_) {}
    }
    if (!composer) {
      const fb = Array.from(document.querySelectorAll('textarea, [contenteditable="true"], [role="textbox"]'))
        .filter((el) => isVisibleEl(el, 100, 16));
      fb.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
      composer = fb[0] || null;
    }

    const file = new File([content], fileName, { type: mimeType });
    const dt = new DataTransfer();
    dt.items.add(file);

    const fileInputs = Array.from(document.querySelectorAll('input[type="file"]')).filter((inp) => {
      if (inp.disabled) return false;
      const acc = (inp.getAttribute('accept') || '').toLowerCase();
      if (acc && /(?:image|audio|video)\//.test(acc) && !/(?:\.md|\.txt|text\/|application\/|\*)/.test(acc)) {
        return false;
      }
      return true;
    });

    if (fileInputs.length > 0) {
      let picked = fileInputs[fileInputs.length - 1];
      if (composer) {
        const bar = composer.closest('form') || composer.parentElement?.parentElement || composer.parentElement;
        if (bar) {
          const inBar = fileInputs.find((inp) => bar.contains(inp));
          if (inBar) picked = inBar;
        }
      }
      try {
        const desc = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'files');
        if (desc && typeof desc.set === 'function') {
          desc.set.call(picked, dt.files);
        } else {
          picked.files = dt.files;
        }
      } catch (_) {
        picked.files = dt.files;
      }
      picked.dispatchEvent(new Event('input', { bubbles: true }));
      picked.dispatchEvent(new Event('change', { bubbles: true }));
      return JSON.stringify({ dispatched: true, via: 'file_input' });
    }

    const dropTarget = composer || document.body;
    if (!dropTarget) return JSON.stringify({ dispatched: false });
    let pasted = false;
    try {
      dropTarget.focus();
      const pasteEvt = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt });
      dropTarget.dispatchEvent(pasteEvt);
      pasted = true;
    } catch (_) {}
    try {
      const dragOpts = { bubbles: true, cancelable: true, dataTransfer: dt };
      dropTarget.dispatchEvent(new DragEvent('dragenter', dragOpts));
      dropTarget.dispatchEvent(new DragEvent('dragover', dragOpts));
      dropTarget.dispatchEvent(new DragEvent('drop', dragOpts));
      return JSON.stringify({ dispatched: true, via: pasted ? 'paste_drop' : 'drop' });
    } catch (_) {
      return JSON.stringify({ dispatched: pasted, via: pasted ? 'paste' : 'none' });
    }
  } catch (err) {
    return JSON.stringify({ dispatched: false, error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
    .replace("__FILENAME__", &name_json)
    .replace("__MIME__", &mime_json)
    .replace("__CONTENT__", &content_json)
}

#[cfg(desktop)]
fn build_group_check_attached_js(adapter: &SiteAdapterConfig, filename: &str) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  let name_json = serde_json::to_string(filename).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const fileName = __FILENAME__;
    const isVisibleEl = (el) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      return r.width > 12 && r.height > 12;
    };
    let composer = null;
    const compSels = Array.isArray(adapter.composerSelectors) ? adapter.composerSelectors : [];
    for (const sel of compSels) {
      if (!sel) continue;
      try {
        const list = Array.from(document.querySelectorAll(sel)).filter(isVisibleEl);
        if (list.length > 0) {
          list.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
          composer = list[0];
          break;
        }
      } catch (_) {}
    }
    if (!composer) {
      const fb = Array.from(document.querySelectorAll('textarea, [contenteditable="true"], [role="textbox"]')).filter(isVisibleEl);
      fb.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
      composer = fb[0] || null;
    }
    const bar = composer ? (composer.closest('form') || composer.parentElement?.parentElement?.parentElement || composer.parentElement) : document.body;
    const barText = (bar && bar.innerText ? bar.innerText : '').toLowerCase();
    const stem = fileName.toLowerCase();
    const hasName = Boolean(stem && (barText.includes(stem) || (document.body?.innerText || '').toLowerCase().includes(stem)));
    const uploading = /上传中|uploading|parsing|解析中/i.test(barText);
    return JSON.stringify({ ready: hasName && !uploading, uploading, hasName });
  } catch (err) {
    return JSON.stringify({ ready: false, error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
    .replace("__FILENAME__", &name_json)
}

#[cfg(desktop)]
fn build_hark_click_preview_card_js(preview_text: &str) -> String {
  let prev_json = serde_json::to_string(preview_text).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const previewText = (__PREVIEW__ || '').trim();
    if (!previewText) return JSON.stringify({ clicked: false });
    const winH = window.innerHeight || document.documentElement.clientHeight || 800;
    const isVisibleEl = (el, minW = 140, minH = 56) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < minW || r.height < minH || r.height > winH * 0.72) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
          if (parseFloat(st.opacity) < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    const composer = Array.from(document.querySelectorAll('textarea, [contenteditable="true"]'))
      .find((el) => isVisibleEl(el, 80, 16));
    let col = document.querySelector('main, [role="main"]') || document.body;
    if (composer) {
      let cur = composer.parentElement;
      while (cur && cur !== document.body && cur !== document.documentElement) {
        const r = cur.getBoundingClientRect();
        if (r.height >= winH * 0.5 && r.width >= 260) { col = cur; break; }
        cur = cur.parentElement;
      }
    }
    const bar = composer ? (composer.closest('form') || composer.parentElement?.parentElement) : null;
    const previewLines = previewText.split(/\r?\n/).map((l) => l.trim()).filter((l) => l.length >= 2);
    const tailLine = previewLines.length ? previewLines[previewLines.length - 1] : '';
    const midLine = previewLines.length >= 2 ? previewLines[Math.floor(previewLines.length / 2)] : '';

    const nodes = Array.from(col.querySelectorAll('div, section, article, button, [role="button"]'));
    let bestCard = null;
    for (let i = nodes.length - 1; i >= 0; i--) {
      const el = nodes[i];
      if (!isVisibleEl(el, 180, 64)) continue;
      if ((bar && bar.contains(el)) || el.closest('nav, aside, header, footer, form')) continue;
      if (el.querySelector('textarea, input')) continue;
      const txt = (el.innerText || '').trim();
      if (!txt || txt.length < 24 || txt.length > 2600) continue;
      const hitsTail = tailLine && txt.includes(tailLine);
      const hitsMid = midLine && txt.includes(midLine);
      if (!hitsTail && !hitsMid) continue;
      if (!previewText.includes(txt.slice(0, Math.min(24, txt.length))) && txt.length > previewText.length + 120) continue;

      let looksLikeCard = el.tagName === 'BUTTON' || el.getAttribute('role') === 'button';
      try {
        const st = getComputedStyle(el);
        if (st.cursor === 'pointer' || st.overflowY === 'hidden' || st.overflow === 'hidden') looksLikeCard = true;
        if ((st.maskImage && st.maskImage !== 'none') || (st.webkitMaskImage && st.webkitMaskImage !== 'none')) looksLikeCard = true;
      } catch (_) {}
      if (el.scrollHeight > el.clientHeight + 8) looksLikeCard = true;
      if (!looksLikeCard) {
        const kids = el.querySelectorAll('[style*="gradient" i], [style*="mask" i], button, [role="button"]');
        if (kids.length > 0) looksLikeCard = true;
      }
      if (!looksLikeCard) continue;

      if (!bestCard || el.getBoundingClientRect().top > bestCard.getBoundingClientRect().top) {
        bestCard = el;
      } else if (bestCard && bestCard.contains(el)) {
        bestCard = el;
      }
    }

    if (!bestCard) return JSON.stringify({ clicked: false });
    const cardPreview = (bestCard.innerText || '').trim();
    const firstLine = cardPreview.split(/\r?\n/).map((l) => l.trim()).find(Boolean) || '';
    let introPrefix = '';
    if (firstLine) {
      const idx = previewText.indexOf(firstLine);
      if (idx > 0) introPrefix = previewText.slice(0, idx).trim();
    }
    bestCard.click();
    return JSON.stringify({ clicked: true, introPrefix, cardPreview });
  } catch (err) {
    return JSON.stringify({ clicked: false, error: String(err) });
  }
})()"#
    .replace("__PREVIEW__", &prev_json)
}

#[cfg(desktop)]
fn build_hark_read_expanded_js(card_preview: &str) -> String {
  let card_json = serde_json::to_string(card_preview).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const cardPreview = (__CARD_PREVIEW__ || '').trim();
    const anchor = cardPreview.split(/\r?\n/).map((l) => l.trim()).find((l) => l.length >= 2) || '';
    const winW = window.innerWidth || document.documentElement.clientWidth || 1000;
    const winH = window.innerHeight || document.documentElement.clientHeight || 800;

    const isVisibleEl = (el) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < winW * 0.3 || r.height < winH * 0.35) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden' || parseFloat(st.opacity) < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    const extractStructuredText = (rootNode) => {
      if (!rootNode) return '';
      const skipSel = 'nav, aside, header, footer, button, script, style, svg, noscript, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let lastBlk = null;
      let lastRow = null;
      let lastCell = null;
      const walker = document.createTreeWalker(rootNode, 4);
      let n;
      while ((n = walker.nextNode())) {
        const v = n.nodeValue;
        if (!v || !v.trim()) continue;
        const p = n.parentElement;
        if (!p || (p.closest && p.closest(skipSel))) continue;
        const row = p.closest ? p.closest('tr') : null;
        const cell = p.closest ? p.closest('td, th') : null;
        const blk = (p.closest && p.closest(blockSel)) || p;
        if (parts.length > 0) {
          if (row && row === lastRow && cell && cell !== lastCell) parts.push(' | ');
          else if (blk !== lastBlk) parts.push('\n');
        }
        parts.push(v.trim());
        lastBlk = blk;
        lastRow = row;
        lastCell = cell;
      }
      return parts.join('').replace(/\n{3,}/g, '\n\n').trim();
    };

    const candidates = Array.from(document.querySelectorAll('[role="dialog"], [aria-modal="true"], dialog, article, main, section, div'))
      .filter((el) => {
        if (!isVisibleEl(el) || el === document.body) return false;
        if (el.closest('nav, aside')) return false;
        if (el.querySelector('textarea')) return false;
        const txt = (el.innerText || '').trim();
        if (!txt || txt.length <= cardPreview.length + 20) return false;
        if (anchor && !txt.includes(anchor)) return false;
        return true;
      });

    if (candidates.length === 0) return JSON.stringify({ text: '' });
    candidates.sort((a, b) => {
      const aModal = a.matches('[role="dialog"], [aria-modal="true"], dialog') ? 1 : 0;
      const bModal = b.matches('[role="dialog"], [aria-modal="true"], dialog') ? 1 : 0;
      if (aModal !== bModal) return bModal - aModal;
      const ra = a.getBoundingClientRect();
      const rb = b.getBoundingClientRect();
      return (ra.width * ra.height) - (rb.width * rb.height);
    });

    let extracted = extractStructuredText(candidates[0]);
    extracted = extracted
      .replace(/^(?:摘要|Summary|Brief)\s*\r?\n+/i, '')
      .replace(/(?:\r?\n+\s*(?:摘要|Summary|Brief)\s*)+$/i, '')
      .trim();
    return JSON.stringify({ text: extracted });
  } catch (err) {
    return JSON.stringify({ text: '', error: String(err) });
  }
})()"#
    .replace("__CARD_PREVIEW__", &card_json)
}

#[cfg(desktop)]
fn build_hark_close_expanded_js() -> String {
  r#"(() => {
  try {
    const btns = Array.from(document.querySelectorAll('button, [role="button"]')).filter((b) => {
      if (!b || !b.isConnected || b.disabled) return false;
      if (b.closest('nav, aside')) return false;
      const r = b.getBoundingClientRect();
      return r.width >= 20 && r.height >= 20 && r.top >= 0 && r.top <= 110 && r.left >= 0 && r.left <= 140;
    });
    const labeled = btns.find((b) => /close|collapse|back|收起|关闭|返回/i.test((b.getAttribute('aria-label') || '') + ' ' + (b.getAttribute('title') || '')));
    const target = labeled || btns.find((b) => Boolean(b.querySelector('svg')));
    if (target) {
      target.click();
      return JSON.stringify({ closed: true, via: 'button' });
    }
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', keyCode: 27, bubbles: true }));
    return JSON.stringify({ closed: true, via: 'escape' });
  } catch (err) {
    return JSON.stringify({ closed: false, error: String(err) });
  }
})()"#
    .to_string()
}

#[cfg(desktop)]
fn build_group_send_js(adapter: &SiteAdapterConfig, job_id: &str, prompt_text: &str) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  let job_json = serde_json::to_string(job_id).unwrap_or_else(|_| "\"\"".to_string());
  let prompt_json = serde_json::to_string(prompt_text).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const jobId = __JOB_ID__;
    const promptText = __PROMPT__;
    const host = (location.hostname || '').toLowerCase();
    const path = (location.pathname || '').toLowerCase();
    const isVisibleEl = (el, minW = 16, minH = 12) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < minW || r.height < minH) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
          const op = parseFloat(st.opacity);
          if (!Number.isNaN(op) && op < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    const findAuthPrompt = () => {
      if (typeof window.__maamFindAuthPrompt === 'function') {
        const hit = window.__maamFindAuthPrompt();
        if (hit) return hit;
      }
      const containers = document.querySelectorAll('[role="dialog"], [role="alertdialog"], [aria-modal="true"], dialog, [class*="permission" i], [class*="approval" i], [class*="authorize" i], [class*="consent" i]');
      for (const box of containers) {
        if (!isVisibleEl(box, 120, 60)) continue;
        const txt = (box.innerText || '').trim();
        if (!/授权|允许|始终允许|批准|请求访问|权限|approve|authorize|permission/i.test(txt)) continue;
        const btns = box.querySelectorAll('button, [role="button"]');
        for (const b of btns) {
          if (!isVisibleEl(b, 20, 16) || b.disabled) continue;
          const label = ((b.innerText || b.getAttribute('aria-label') || '') + '').trim();
          if (/^(允许|始终允许|本次允许|批准|确认授权|同意|allow|allow once|always allow|approve|authorize)$/i.test(label)) {
            return txt.slice(0, 140);
          }
        }
      }
      return '';
    };

    if (window.__maamAutoApprove === true && typeof window.__maamScanAutoApprove === 'function') {
      if (window.__maamScanAutoApprove()) {
        return JSON.stringify({ ok: false, status: 'no_composer', error: '' });
      }
    }

    const authReason = findAuthPrompt();
    if (authReason) {
      return JSON.stringify({ ok: false, status: 'needs_auth', error: '检测到页面授权确认，请前往该账号页面点击允许' });
    }

    const loginInputs = document.querySelectorAll('input[type="password"], input[type="email"]');
    let hasLoginForm = false;
    for (const inp of loginInputs) {
      if (isVisibleEl(inp, 40, 18)) { hasLoginForm = true; break; }
    }

    const isUsableComposer = (el) => {
      if (!el || el.disabled || el.readOnly) return false;
      if (el.getAttribute('aria-disabled') === 'true') return false;
      if (el.getAttribute('contenteditable') === 'false') return false;
      const type = (el.getAttribute('type') || '').toLowerCase();
      if (type === 'hidden' || type === 'password' || type === 'email' || type === 'search') return false;
      const ph = ((el.getAttribute('placeholder') || el.getAttribute('aria-label') || '') + '').toLowerCase();
      if (/\b(search|filter)\b|搜索|筛选/.test(ph)) return false;
      return isVisibleEl(el, 80, 16);
    };

    let composer = null;
    const adapterComposers = Array.isArray(adapter.composerSelectors) ? adapter.composerSelectors : [];
    for (const sel of adapterComposers) {
      if (!sel) continue;
      let list = [];
      try { list = Array.from(document.querySelectorAll(sel)); } catch (_) { continue; }
      const matched = list.filter(isUsableComposer);
      if (matched.length > 0) {
        matched.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
        composer = matched[0];
        break;
      }
    }

    if (!composer) {
      const rawComposers = Array.from(document.querySelectorAll('textarea, [contenteditable="true"], [role="textbox"]'));
      const candidates = rawComposers.filter((el) => isUsableComposer(el) && isVisibleEl(el, 100, 16));
      candidates.sort((a, b) => {
        const score = (el) => {
          const r = el.getBoundingClientRect();
          let s = r.bottom;
          if (el.closest('form')) s += 400;
          if (el.tagName === 'TEXTAREA' || el.id === 'prompt-textarea') s += 120;
          return s;
        };
        return score(b) - score(a);
      });
      composer = candidates[0] || null;
    }

    if (!composer) {
      if ((window.__maamAuthCheck && window.__maamAuthCheck.status === 'invalid') || hasLoginForm || /^\/(login|signin|signup|auth)\b/.test(path)) {
        return JSON.stringify({ ok: false, status: 'logged_out', error: '该账号登录态已失效，请前往重新登录' });
      }
      const bodyTxt = (document.body && document.body.innerText) || '';
      const hasRetryBtn = Array.from(document.querySelectorAll('button, a, [role="button"]')).some((el) => {
        if (!isVisibleEl(el, 16, 12)) return false;
        const label = ((el.innerText || el.getAttribute('aria-label') || '') + '').trim();
        return /^(?:重试|Try again|退出|Log out)$/i.test(label);
      });
      if (/^\/blocked\b/.test(path) || (/(?:^|\n)\s*(?:出错了|Something went wrong|Application Error|应用出错)\s*(?:\n|$)/.test(bodyTxt) && hasRetryBtn)) {
        return JSON.stringify({ ok: false, status: 'page_error', error: '该账号页面出错，请检查代理或清理重登' });
      }
      if (host.includes('hark.com')) {
        const tabs = Array.from(document.querySelectorAll('button, a, [role="tab"]'));
        for (const tb of tabs) {
          if (!isVisibleEl(tb, 20, 14)) continue;
          const label = (tb.innerText || tb.getAttribute('aria-label') || '').trim();
          if (/^(聊天|Chat)$/i.test(label)) {
            try { tb.click(); } catch (_) {}
            break;
          }
        }
      }
      return JSON.stringify({ ok: false, status: 'no_composer', error: '当前页面未找到聊天输入框，请切换至该账号会话页检查' });
    }

    const resolveChatColumn = (comp) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (comp && comp.isConnected) {
        let cur = comp.parentElement;
        while (cur && cur !== document.body && cur !== document.documentElement) {
          const r = cur.getBoundingClientRect();
          if (r.height >= winH * 0.5 && r.width >= 220) {
            return cur;
          }
          cur = cur.parentElement;
        }
      }
      return document.querySelector('main, [role="main"]') || document.body;
    };
    const resolveComposerBar = (comp, col) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (!comp || !comp.isConnected) return null;
      let bar = comp;
      let cur = comp.parentElement;
      while (cur && cur !== col && cur !== document.body) {
        const r = cur.getBoundingClientRect();
        if (r.height >= winH * 0.36) break;
        bar = cur;
        cur = cur.parentElement;
      }
      return bar;
    };

    const chatCol = resolveChatColumn(composer);
    const composerBar = resolveComposerBar(composer, chatCol);
    const colRect = chatCol.getBoundingClientRect();

    const adapterAsst = Array.isArray(adapter.assistantMessageSelectors) ? adapter.assistantMessageSelectors.filter(Boolean) : [];
    const baseMsgSelector = '[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], [data-testid*="assistant" i], [data-testid*="response" i], [data-testid*="turn" i], article, [class*="markdown" i], [class*="prose" i]';
    const msgSelector = adapterAsst.length ? adapterAsst.join(', ') + ', ' + baseMsgSelector : baseMsgSelector;
    const isExcludedContainer = (el) => Boolean((composerBar && composerBar.contains(el)) || (el.closest && el.closest('nav, aside, header, footer, form')));
    const isMessageWrapper = (el) => {
      const tid = (el.getAttribute('data-testid') || '').toLowerCase();
      if (/list|container|scroll|wrapper|feed|history|timestamp|time|date|header|footer|avatar/.test(tid)) return true;
      const nested = el.querySelectorAll('[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], article').length;
      return nested > 1;
    };
    const extractCleanText = (rootNode, skipChrome) => {
      if (!rootNode) return '';
      const skipSel = skipChrome
        ? 'nav, aside, header, footer, form, button, script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]'
        : 'script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let lastBlk = null;
      try {
        const walker = document.createTreeWalker(rootNode, 4);
        let n;
        while ((n = walker.nextNode())) {
          const v = n.nodeValue;
          if (!v || !v.trim()) continue;
          const p = n.parentElement;
          if (!p || (skipChrome && composerBar && composerBar.contains(p)) || (p.closest && p.closest(skipSel))) continue;
          const blk = (p.closest && p.closest(blockSel)) || p;
          if (parts.length > 0 && blk !== lastBlk) parts.push('\n');
          parts.push(v.trim());
          lastBlk = blk;
        }
      } catch (_) {}
      return parts.join('').replace(/\n{3,}/g, '\n\n').trim();
    };
    const getNodeText = (el) => {
      if (!el) return '';
      const t = (el.innerText || '').trim();
      if (t) return t;
      return extractCleanText(el, false);
    };
    const getSearchRoot = () => {
      const listSels = Array.isArray(adapter.messageListSelectors) ? adapter.messageListSelectors : [];
      for (const sel of listSels) {
        if (!sel || sel === 'main') continue;
        try {
          const el = chatCol.querySelector(sel);
          if (el && el.getBoundingClientRect().height > 10) return el;
        } catch (_) {}
      }
      return chatCol.querySelector('[data-testid="message-list"], [data-testid*="chat-messages" i], [data-testid*="message-feed" i]') || chatCol;
    };
    const collectBaseline = () => {
      const sRoot = getSearchRoot();
      const texts = [];
      const els = [];
      let list = [];
      try { list = sRoot.querySelectorAll(msgSelector); } catch (_) { list = sRoot.querySelectorAll(baseMsgSelector); }
      for (const el of list) {
        if (isExcludedContainer(el) || isMessageWrapper(el)) continue;
        if (!isVisibleEl(el, 20, 12)) continue;
        els.push(el);
        const txt = getNodeText(el);
        if (txt) texts.push(txt);
      }
      return { texts, els };
    };

    const getCleanMainText = () => extractCleanText(getSearchRoot(), true);

    const baselineInfo = collectBaseline();
    const baselineBlocks = baselineInfo.texts;
    const baselineMainText = getCleanMainText();
    const normWs = (s) => (s || '').replace(/\s+/g, ' ').trim();
    const stripBaselineTail = (raw) => {
      let s = (raw || '').trim();
      for (let k = 0; k < 6; k++) {
        const next = s.replace(/(?:\r?\n|\s|^)(?:\d{1,2}:\d{2}|Muse|Hark|已阅读|已发送|已连接|空闲中|复制|Copy)$/i, '').trim();
        if (next === s) break;
        s = next;
      }
      return s;
    };
    const baselineCleanSet = new Set();
    for (const bt of baselineBlocks) {
      const c = stripBaselineTail(bt);
      if (c) baselineCleanSet.add(c);
    }
    for (const ln of baselineMainText.split(/\r?\n/)) {
      const c = stripBaselineTail(ln);
      if (c) baselineCleanSet.add(c);
    }
    const sRoot = getSearchRoot();
    const baselineDomEls = new WeakSet();
    try {
      for (const el of sRoot.querySelectorAll('*')) {
        baselineDomEls.add(el);
      }
    } catch (_) {}
    let lastBaselineEl = baselineInfo.els.length > 0 ? baselineInfo.els[baselineInfo.els.length - 1] : null;
    if (!lastBaselineEl && sRoot) {
      try {
        const allRootEls = Array.from(sRoot.querySelectorAll('*'));
        for (let i = allRootEls.length - 1; i >= 0; i--) {
          const el = allRootEls[i];
          if (!el || isExcludedContainer(el) || el.children.length > 0) continue;
          if (!isVisibleEl(el, 8, 8)) continue;
          if ((el.textContent || '').trim()) {
            lastBaselineEl = el;
            break;
          }
        }
      } catch (_) {}
    }
    const baselineMainNorm = normWs(baselineMainText);
    const baselineBlockNorms = baselineBlocks
      .map((bt) => normWs(stripBaselineTail(bt)))
      .filter((s) => s.length >= 8);
    const lines = promptText.split('\n').map((l) => l.trim()).filter(Boolean);
    const isFooterLine = (l) => l.includes('不要复述上方方括号标题') || l.includes('不要重复上方协议标签') || l.includes('不要重复以上协议标签') || l.includes('do not repeat the bracketed headings above');
    const promptNeedle = (lines.length >= 2 && isFooterLine(lines[lines.length - 1]))
      ? lines[lines.length - 2]
      : (lines.length > 0 ? lines[lines.length - 1] : promptText.trim());
    const headCandidates = lines.filter((l) => !/^\[[^\]]{1,24}\]$/.test(l) && !isFooterLine(l));
    const headPick = headCandidates.find((l) => {
      const n40 = normWs(l).slice(0, 40);
      return n40 && (!baselineMainNorm || !baselineMainNorm.includes(n40));
    }) || headCandidates[0] || '';
    const promptHead = headPick.slice(0, 48);
    const endMarker = '(Please answer the “Current question” directly, and do not repeat the bracketed headings above)';
    const endMarkerZh = '（请直接回答「当前问题」，不要复述上方方括号标题）';
    const baselineMarkerCount = baselineMainText ? ((baselineMainText.split(endMarker).length - 1) + (baselineMainText.split(endMarkerZh).length - 1)) : 0;
    const baselineNeedleCount = (baselineMainText && promptNeedle) ? (baselineMainText.split(promptNeedle).length - 1) : 0;
    const baselineHeadCount = (baselineMainText && promptHead) ? (baselineMainText.split(promptHead).length - 1) : 0;
    const busyRe = /Researching\s+\w+|Searching\s+for\b|正在搜索|正在联网搜索|正在检索|正在读取|正在解析|正在加载|正在生成|正在处理|努力思考中|努力工作中/gi;
    const baselineBodyBusyCount = ((document.body && document.body.innerText) ? (document.body.innerText.match(busyRe) || []).length : 0);

    if (window.__maamNet) {
      window.__maamNet.esText = '';
      window.__maamNet.wsText = '';
    }

    window.__maamGroupJobs = window.__maamGroupJobs || {};
    if (window.__maamGroupJobs[jobId] && window.__maamGroupJobs[jobId].observer) {
      try { window.__maamGroupJobs[jobId].observer.disconnect(); } catch (_) {}
    }
    const jobRec = {
      jobId,
      host,
      composerEl: composer,
      chatColRect: { left: colRect.left, right: colRect.right, width: colRect.width },
      promptText,
      promptNeedle,
      promptHead,
      baselineCount: baselineBlocks.length,
      baselineSet: new Set(baselineBlocks.map((t) => t.slice(0, 220))),
      baselineExact: new Set(baselineBlocks),
      baselineCleanSet,
      baselineEls: new Set(baselineInfo.els),
      baselineDomEls,
      lastBaselineEl,
      baselineMainText,
      baselineMainNorm,
      baselineBlockNorms,
      baselineLines: new Set(baselineMainText.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)),
      baselineMarkerCount,
      baselineNeedleCount,
      baselineHeadCount,
      baselineBodyBusyCount,
      netStartSeq: (window.__maamNet && window.__maamNet.seq) || 0,
      netStartAt: Date.now(),
      startedAt: Date.now(),
      lastDomMutAt: Date.now(),
      observer: null,
      lastText: '',
      lastChangedAt: Date.now(),
    };
    try {
      if (typeof MutationObserver === 'function') {
        const obs = new MutationObserver(() => {
          jobRec.lastDomMutAt = Date.now();
        });
        obs.observe(getSearchRoot(), { childList: true, subtree: true, characterData: true });
        jobRec.observer = obs;
      }
    } catch (_) {}
    window.__maamGroupJobs[jobId] = jobRec;

    composer.focus();
    if (composer.tagName === 'TEXTAREA' || composer.tagName === 'INPUT') {
      const tracker = composer._valueTracker;
      if (tracker && typeof tracker.setValue === 'function') {
        try { tracker.setValue(''); } catch (_) {}
      }
      const proto = composer.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      const desc = Object.getOwnPropertyDescriptor(proto, 'value');
      if (desc && typeof desc.set === 'function') {
        desc.set.call(composer, promptText);
      } else {
        composer.value = promptText;
      }
      try {
        composer.dispatchEvent(new InputEvent('input', { bubbles: true, cancelable: true, inputType: 'insertText', data: promptText }));
      } catch (_) {}
      composer.dispatchEvent(new Event('input', { bubbles: true }));
      composer.dispatchEvent(new Event('change', { bubbles: true }));
    } else {
      try {
        const sel = window.getSelection();
        if (sel) {
          sel.removeAllRanges();
          const range = document.createRange();
          range.selectNodeContents(composer);
          sel.addRange(range);
        }
        document.execCommand('insertText', false, promptText);
      } catch (_) {}
      if ((composer.innerText || '').trim() !== promptText.trim()) {
        composer.textContent = promptText;
        try {
          composer.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, cancelable: true, inputType: 'insertText', data: promptText }));
        } catch (_) {}
        try {
          composer.dispatchEvent(new InputEvent('input', { bubbles: true, cancelable: true, inputType: 'insertText', data: promptText }));
        } catch (_) {
          composer.dispatchEvent(new Event('input', { bubbles: true }));
        }
      }
    }

    const isUsableSendBtn = (b) => {
      if (!b || b.disabled || b.getAttribute('aria-disabled') === 'true') return false;
      if (!isVisibleEl(b, 16, 16)) return false;
      const label = ((b.getAttribute('aria-label') || '') + ' ' + (b.getAttribute('title') || '') + ' ' + (b.getAttribute('data-testid') || '') + ' ' + (b.innerText || '')).trim().toLowerCase();
      if (/stop|停止|attach|upload|voice|mic|record|menu|上传|附件|附加|语音|录音|听写|dictat|模型/.test(label)) return false;
      return true;
    };

    const findSendBtn = (allowSvgFallback) => {
      const scopes = [];
      const form = composer.closest('form');
      if (form) scopes.push(form);
      let cur = composer.parentElement;
      for (let d = 0; cur && d < 6; d++) {
        if (!scopes.includes(cur)) scopes.push(cur);
        cur = cur.parentElement;
      }
      const adapterSendSels = Array.isArray(adapter.sendButtonSelectors) ? adapter.sendButtonSelectors : [];
      for (const sel of adapterSendSels) {
        if (!sel) continue;
        for (const scope of scopes) {
          try {
            const btns = Array.from(scope.querySelectorAll(sel));
            for (const b of btns) {
              if (isUsableSendBtn(b)) return b;
            }
          } catch (_) {}
        }
        try {
          const globalBtns = Array.from(document.querySelectorAll(sel));
          for (const b of globalBtns) {
            if (isUsableSendBtn(b)) return b;
          }
        } catch (_) {}
      }
      for (const scope of scopes) {
        const btns = Array.from(scope.querySelectorAll('button, [role="button"]'));
        for (const b of btns) {
          if (!isUsableSendBtn(b)) continue;
          const label = ((b.getAttribute('aria-label') || '') + ' ' + (b.getAttribute('title') || '') + ' ' + (b.getAttribute('data-testid') || '') + ' ' + (b.innerText || '')).trim().toLowerCase();
          if (b.getAttribute('type') === 'submit' || /send|submit|发送|提交|提问/.test(label)) {
            return b;
          }
        }
      }
      if (allowSvgFallback) {
        const near = composer.closest('form') || composer.parentElement?.parentElement || composer.parentElement;
        if (near) {
          const btns = Array.from(near.querySelectorAll('button, [role="button"]')).filter((b) => {
            if (!isUsableSendBtn(b)) return false;
            const label = ((b.getAttribute('aria-label') || '') + ' ' + (b.getAttribute('title') || '') + ' ' + (b.getAttribute('data-testid') || '') + ' ' + (b.innerText || '')).trim().toLowerCase();
            if (/plus|add|新建|历史/.test(label)) return false;
            return Boolean(b.querySelector('svg'));
          });
          if (btns.length > 0) return btns[btns.length - 1];
        }
      }
      return null;
    };

    const dispatchEnter = () => {
      try {
        composer.focus();
        ['keydown', 'keypress', 'keyup'].forEach((evType) => {
          composer.dispatchEvent(new KeyboardEvent(evType, {
            key: 'Enter',
            code: 'Enter',
            keyCode: 13,
            which: 13,
            bubbles: true,
            cancelable: true,
          }));
        });
      } catch (_) {}
    };

    let submitClicked = false;
    const trySubmit = (attempt) => {
      try {
        if (submitClicked) return;
        if (adapter.preferEnterSubmit && attempt === 0) {
          submitClicked = true;
          dispatchEnter();
          setTimeout(() => {
            try {
              const curBlocks = collectBaseline().texts;
              const rem = (composer.value || composer.innerText || '').trim();
              if (curBlocks.length <= baselineBlocks.length && rem.length >= Math.min(20, promptText.trim().length)) {
                const sb = findSendBtn(true);
                if (sb) sb.click();
              }
            } catch (_) {}
          }, 450);
          return;
        }
        const sendBtn = findSendBtn(attempt >= 4);
        if (sendBtn && !sendBtn.disabled && sendBtn.getAttribute('aria-disabled') !== 'true') {
          submitClicked = true;
          composer.focus();
          sendBtn.click();
          setTimeout(() => {
            try {
              const curBlocks = collectBaseline().texts;
              const rem = (composer.value || composer.innerText || '').trim();
              if (curBlocks.length <= baselineBlocks.length && rem.length >= Math.min(20, promptText.trim().length)) {
                dispatchEnter();
              }
            } catch (_) {}
          }, 450);
          return;
        }
        if (attempt < 20) {
          setTimeout(() => trySubmit(attempt + 1), 80);
          return;
        }
        submitClicked = true;
        dispatchEnter();
      } catch (_) {}
    };

    Promise.resolve().then(() => {
      setTimeout(() => trySubmit(0), 60);
    });

    return JSON.stringify({ ok: true });
  } catch (err) {
    return JSON.stringify({ ok: false, status: 'error', error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
    .replace("__JOB_ID__", &job_json)
    .replace("__PROMPT__", &prompt_json)
}

#[cfg(desktop)]
fn build_group_poll_js(adapter: &SiteAdapterConfig, job_id: &str) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  let job_json = serde_json::to_string(job_id).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const jobId = __JOB_ID__;
    const job = window.__maamGroupJobs && window.__maamGroupJobs[jobId];
    if (!job) {
      return JSON.stringify({ state: 'error', text: '', error: '页面已刷新或任务上下文已丢失' });
    }

    const isVisibleEl = (el, minW = 16, minH = 12) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < minW || r.height < minH) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
          const op = parseFloat(st.opacity);
          if (!Number.isNaN(op) && op < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    const findAuthPrompt = () => {
      if (typeof window.__maamFindAuthPrompt === 'function') {
        const hit = window.__maamFindAuthPrompt();
        if (hit) return hit;
      }
      const containers = document.querySelectorAll('[role="dialog"], [role="alertdialog"], [aria-modal="true"], dialog, [class*="permission" i], [class*="approval" i], [class*="authorize" i], [class*="consent" i]');
      for (const box of containers) {
        if (box.closest && box.closest('nav, aside')) continue;
        if (!isVisibleEl(box, 120, 60)) continue;
        const txt = (box.innerText || '').trim();
        if (!/授权|允许|始终允许|批准|请求访问|权限|approve|authorize|permission/i.test(txt)) continue;
        const btns = box.querySelectorAll('button, [role="button"]');
        for (const b of btns) {
          if (!isVisibleEl(b, 20, 16) || b.disabled) continue;
          const label = ((b.innerText || b.getAttribute('aria-label') || '') + '').trim();
          if (/^(允许|始终允许|本次允许|批准|确认授权|同意|allow|allow once|always allow|approve|authorize)$/i.test(label)) {
            return txt.slice(0, 140);
          }
        }
      }
      return '';
    };

    if (window.__maamAutoApprove === true && typeof window.__maamScanAutoApprove === 'function') {
      if (window.__maamScanAutoApprove()) {
        return JSON.stringify({ state: 'streaming', text: job.lastText || '' });
      }
    }

    const authReason = findAuthPrompt();
    if (authReason) {
      return JSON.stringify({ state: 'needs_auth', text: job.lastText || '', error: '该账号触发授权确认，请前往原生页面点击允许后重试' });
    }

    const isInteractiveBtn = (b) => {
      if (!isVisibleEl(b, 12, 12) || b.disabled || b.getAttribute('aria-disabled') === 'true' || b.getAttribute('aria-hidden') === 'true') return false;
      try {
        if (getComputedStyle(b).pointerEvents === 'none') return false;
      } catch (_) {}
      return true;
    };
    const sendBtns = Array.from(document.querySelectorAll('button, [role="button"]')).filter((b) => {
      if (!isInteractiveBtn(b)) return false;
      const t = (b.innerText || b.getAttribute('aria-label') || b.getAttribute('title') || b.getAttribute('data-testid') || '').trim();
      return /^(发送|发送消息|Send|Send message)$/i.test(t);
    });
    const overlapsSendBtn = (el) => {
      if (!el || !sendBtns.length) return false;
      const r1 = el.getBoundingClientRect();
      return sendBtns.some((sb) => {
        if (sb === el) return false;
        const r2 = sb.getBoundingClientRect();
        return Math.abs(r1.top - r2.top) < 12 && Math.abs(r1.left - r2.left) < 12;
      });
    };

    const adapterStops = Array.isArray(adapter.stopButtonSelectors) ? adapter.stopButtonSelectors.filter(Boolean) : [];
    const baseStopSelector = 'button[aria-label*="Stop" i], button[aria-label*="停止"], button[data-testid*="stop" i]';
    const stopSelector = adapterStops.length ? adapterStops.join(', ') + ', ' + baseStopSelector : baseStopSelector;
    let domGenerating = false;
    let stopEls = [];
    try { stopEls = document.querySelectorAll(stopSelector); } catch (_) { stopEls = document.querySelectorAll(baseStopSelector); }
    for (const sEl of stopEls) {
      if (sEl.closest && sEl.closest('nav, aside, header')) continue;
      if (!isInteractiveBtn(sEl) || overlapsSendBtn(sEl)) continue;
      domGenerating = true;
      break;
    }
    if (!domGenerating) {
      const allBtns = document.querySelectorAll('button, [role="button"]');
      for (const b of allBtns) {
        if (!isInteractiveBtn(b) || overlapsSendBtn(b)) continue;
        const txt = (b.innerText || b.getAttribute('aria-label') || b.getAttribute('title') || '').trim();
        if (/^(停止|停止生成|Stop|Stop generating)$/i.test(txt)) {
          domGenerating = true;
          break;
        }
      }
    }

    const resolveActiveComposer = () => {
      if (job.composerEl && job.composerEl.isConnected) return job.composerEl;
      const compSels = Array.isArray(adapter.composerSelectors) ? adapter.composerSelectors : [];
      for (const sel of compSels) {
        if (!sel) continue;
        try {
          const list = Array.from(document.querySelectorAll(sel)).filter((el) => isVisibleEl(el, 80, 16));
          if (list.length > 0) {
            list.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
            return list[0];
          }
        } catch (_) {}
      }
      const fallback = Array.from(document.querySelectorAll('textarea, [contenteditable="true"], [role="textbox"]'))
        .filter((el) => isVisibleEl(el, 100, 16));
      fallback.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
      return fallback[0] || null;
    };
    const resolveChatColumn = (comp) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (comp && comp.isConnected) {
        let cur = comp.parentElement;
        while (cur && cur !== document.body && cur !== document.documentElement) {
          const r = cur.getBoundingClientRect();
          if (r.height >= winH * 0.5 && r.width >= 220) {
            return cur;
          }
          cur = cur.parentElement;
        }
      }
      return document.querySelector('main, [role="main"]') || document.body;
    };
    const resolveComposerBar = (comp, col) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (!comp || !comp.isConnected) return null;
      let bar = comp;
      let cur = comp.parentElement;
      while (cur && cur !== col && cur !== document.body) {
        const r = cur.getBoundingClientRect();
        if (r.height >= winH * 0.36) break;
        bar = cur;
        cur = cur.parentElement;
      }
      return bar;
    };

    const activeComposer = resolveActiveComposer();
    if (activeComposer) job.composerEl = activeComposer;
    const composerVal = activeComposer
      ? (activeComposer.value || activeComposer.innerText || '').trim()
      : '';
    const chatCol = resolveChatColumn(activeComposer);
    const composerBar = resolveComposerBar(activeComposer, chatCol);
    const colRect = (chatCol && chatCol !== document.body)
      ? chatCol.getBoundingClientRect()
      : (job.chatColRect || null);

    if (!domGenerating && composerBar && composerVal === '') {
      const barBtns = composerBar.querySelectorAll('button, [role="button"]');
      for (const b of barBtns) {
        if (!isInteractiveBtn(b) || overlapsSendBtn(b)) continue;
        const rectSvg = b.querySelector('svg rect');
        if (rectSvg && !b.querySelector('svg path, svg circle, svg line, svg polyline, svg polygon')) {
          const rRect = rectSvg.getBoundingClientRect();
          const rw = parseFloat(rectSvg.getAttribute('width') || '0') || rRect.width;
          const rh = parseFloat(rectSvg.getAttribute('height') || '0') || rRect.height;
          if (rw >= 6 && rw <= 20 && rh >= 6 && Math.abs(rw - rh) <= 2) {
            domGenerating = true;
            break;
          }
        }
      }
    }

    const busyRe = /Researching\s+\w+|Searching\s+for\b|正在搜索|正在联网搜索|正在检索|正在读取|正在解析|正在加载|正在生成|正在处理|努力思考中|努力工作中/gi;
    const curBodyBusyCount = ((document.body && document.body.innerText) ? (document.body.innerText.match(busyRe) || []).length : 0);
    if (curBodyBusyCount > (job.baselineBodyBusyCount || 0)) {
      domGenerating = true;
    }

    const extraWords = Array.isArray(adapter.transientStatusWords)
      ? adapter.transientStatusWords.map((w) => String(w || '').trim()).filter(Boolean)
      : [];
    const extraIdleRe = /^(?:空闲中|已连接|已阅读|消息|Idle|Connected|Hark|Muse)$/i;
    let statusBusy = false;
    let progressText = '';
    const isIdleOnlyStatus = (s) => /^(?:(?:Muse|Hark|空闲中|已连接|已阅读|消息|Idle|Connected)[….\s]*)+$/i.test((s || '').trim());
    const isTransientStatusText = (s) => {
      const t = (s || '').trim();
      if (!t) return false;
      if (extraWords.some((w) => t === w || t === ('Muse\n' + w) || t === ('Hark\n' + w))) {
        return true;
      }
      return /^(?:(?:Muse|Hark)[ \t\r\n]+(?:正在[\u4e00-\u9fa5]{1,10}|[\u4e00-\u9fa5]{1,8}中|已连接|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)[….\s]*|(?:Muse|Hark)[ \t\r\n]*|(?:正在预热|预热中|空闲中|已连接|连接中|正在连接|仍在发送中|发送中|努力工作中|工作中|努力思考中|正在思考|思考中|正在搜索|正在搜索网页|正在联网搜索|搜索中|正在检索|检索中|正在回复|回复中|正在分析|分析中|正在查询|查询中|正在处理|处理中|正在生成|生成中|正在准备|准备中|加载中|待命中|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)[….\s]*)+$/i.test(t);
    };
    const isTransientProgressOnly = (txt) => {
      const s = (txt || '').trim();
      if (!s) return false;
      if (isTransientStatusText(s)) return !isIdleOnlyStatus(s);
      const lines = s.split(/\r?\n/).map((l) => l.trim().replace(/[。….\s]+$/g, '')).filter(Boolean);
      if (lines.length === 0 || lines.length > 3 || s.length > 100) return false;
      return lines.every((l) =>
        /^(?:Muse|Hark|ChatGPT|Claude|Gemini|DeepSeek)$/i.test(l)
        || /^(?:正在|努力|准备|开始)[^\n。！？!?]{1,32}$/.test(l)
        || /^[\u4e00-\u9fa5A-Za-z0-9 _\-]{1,18}中$/.test(l)
        || /^(?:正在|努力|开始)?(?:搜索|联网搜索|检索|查询|思考|深度思考|分析|阅读|读取|读入|解析|打开|下载|提取|整合|合并|修订|完善|优化|确认|等待|接收|获取|拉取|同步|写入|保存|浏览|整理|生成|处理|准备|加载|连接|预热|调用|执行|工作|研究|检查|审阅|评审|核对|评估|起草|撰写|编写|规划|制定|综合|汇总|梳理|对比|查看|构思|草拟|校验|复核|总结)[^\n。！？!?]{0,30}$/.test(l)
        || /^(?:已(?:搜索|阅读|读取|检索|解析|接收|获取|加载|打开|提取|合并|整合|浏览|思考|检查|审阅|核对)[^\n。！？!?]{0,24})$/.test(l)
        || /^(?:Searching|Researching|Thinking|Analyzing|Reading|Browsing|Generating|Loading|Fetching|Parsing|Opening|Merging|Synthesizing|Processing|Preparing|Running|Calling|Executing|Working|Warming\s*up|Sending|Retrieving|Checking|Reviewing|Drafting|Writing|Planning|Summarizing|Verifying|Evaluating)\b[^\n.!?]{0,70}$/i.test(l)
      );
    };
    const escRe = (v) => v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const cleanChrome = (txt) => {
      let s = (txt || '')
        .replace(/(\r?\n\s*(复制|Copy|Copy code|Regenerate|重新生成|重试|Retry|Share|分享|Edit|编辑|Like|Dislike)\s*)+$/gi, '')
        .trim();
      if (isTransientProgressOnly(s)) {
        statusBusy = true;
        if (!progressText) progressText = s;
        return '';
      }
      if (isTransientStatusText(s)) {
        if (!isIdleOnlyStatus(s) && !extraIdleRe.test(s)) statusBusy = true;
        return '';
      }
      const leadStatusRe = /^(?:(?:Muse|Hark)\s*\r?\n+|(?:正在预热|预热中|空闲中|仍在发送中|发送中|正在搜索网页|正在联网搜索|正在搜索|搜索中|正在思考|思考中|读取中|解析中|处理中|生成中|正在[^\r\n。！？!?]{1,28}[。….]?|已(?:搜索|阅读|读取|检索|解析)[^\r\n]{0,20}[。….]?|Thinking[.\s]*|Searching[.\s]*|Reading[.\s]*|Thought for \d+s?)\s*\r?\n+)+/i;
      s = s.replace(leadStatusRe, '').trim();
      const leadSpeakerRe = /^(?:(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]?\s*\r?\n+|(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]\s*)+/i;
      s = s.replace(leadSpeakerRe, '').trim();
      if (/^(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]?$/i.test(s)) {
        return '';
      }
      const tailRe = /(?:(?:Muse|Hark)[ \t\r\n]+(?:正在[\u4e00-\u9fa5]{1,10}|[\u4e00-\u9fa5]{1,8}中|已连接|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)(?:[ \t\r\n]+消息)?)+$|(?:\r?\n|\s|^)(?:Muse|Hark|已连接|连接中|正在连接|正在预热|预热中|空闲中|仍在发送中|发送中|努力工作中|工作中|努力思考中|正在思考|思考中|正在搜索|正在搜索网页|搜索中|正在检索|检索中|正在回复|回复中|正在分析|分析中|正在查询|查询中|正在处理|处理中|正在生成|生成中|正在准备|准备中|加载中|待命中|消息|停止|发送|Sending|已阅读|给\s*Hark\s*发消息|附加文件|开始听写|添加附件|发送消息|留下心情|共享页面|更多选项|Go to Ideas|Customize your Feed|\d{1,2}:\d{2})$/i;
      for (let k = 0; k < 10; k++) {
        let next = s.replace(tailRe, '').trim();
        for (const ew of extraWords) {
          const ewRe = new RegExp('(?:\\r?\\n|\\s|^)(?:(?:Muse|Hark)\\s+)?' + escRe(ew) + '$', 'i');
          next = next.replace(ewRe, '').trim();
        }
        if (next === s) break;
        s = next;
      }
      if (isTransientProgressOnly(s)) {
        statusBusy = true;
        if (!progressText) progressText = s;
        return '';
      }
      if (isTransientStatusText(s)) {
        if (!isIdleOnlyStatus(s) && !extraIdleRe.test(s)) statusBusy = true;
        return '';
      }
      return s;
    };

    const normWs = (s) => (s || '').replace(/\s+/g, ' ').trim();
    const promptNorm = (job.promptText || '').trim();
    const promptNormWs = normWs(promptNorm);
    const isSameAsPrompt = (t) => {
      const s = (t || '').trim();
      if (!s) return true;
      if (s === promptNorm) return true;
      const nt = normWs(s);
      if (nt.length >= 12 && promptNormWs.includes(nt)) return true;
      if (job.promptHead && s.includes(job.promptHead) && s.length <= promptNorm.length + 120) return true;
      if ((promptNorm.startsWith('[阶段要求]') || promptNorm.startsWith('[Stage requirements]')) && (s.startsWith('[阶段要求]') || s.startsWith('[Stage requirements]')) && s.length <= promptNorm.length + 120) return true;
      if (job.promptNeedle && s === job.promptNeedle && s.length <= promptNorm.length + 12) return true;
      if ((s.includes('不要复述上方方括号标题') || s.includes('不要重复上方协议标签') || s.includes('不要重复以上协议标签') || s.includes('do not repeat the bracketed headings above')) && (s.includes('[当前问题]') || s.includes('[用户问题]') || s.includes('[当前群聊发言]') || s.includes('[Current question]') || s.includes('[User question]') || s.includes('[Current group chat message]'))) return true;
      return false;
    };

    let scopedList = null;
    const adapterListSels = Array.isArray(adapter.messageListSelectors) ? adapter.messageListSelectors : [];
    for (const sel of adapterListSels) {
      if (!sel || sel === 'main') continue;
      try {
        const el = chatCol.querySelector(sel);
        if (el && el.getBoundingClientRect().height > 10) { scopedList = el; break; }
      } catch (_) {}
    }
    if (!scopedList) {
      scopedList = chatCol.querySelector('[data-testid="message-list"], [data-testid*="chat-messages" i], [data-testid*="message-feed" i]');
    }
    const searchRoot = scopedList || chatCol;
    const isOutOfColBounds = (el) => {
      if (!colRect || colRect.width < 120 || !el || !el.getBoundingClientRect) return false;
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0) return false;
      return r.right < colRect.left - 16 || r.left > colRect.right + 16;
    };
    const isExcluded = (el) => Boolean(
      (composerBar && composerBar.contains(el))
      || (el.closest && el.closest('nav, aside, header, footer, form'))
      || isOutOfColBounds(el)
    );
    const isMessageWrapper = (el) => {
      const tid = (el.getAttribute('data-testid') || '').toLowerCase();
      if (/list|container|scroll|wrapper|feed|history|timestamp|time|date|header|footer|avatar/.test(tid)) return true;
      const nested = el.querySelectorAll('[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], article').length;
      return nested > 1;
    };
    const extractCleanText = (rootNode, skipChrome) => {
      if (!rootNode) return '';
      const skipSel = skipChrome
        ? 'nav, aside, header, footer, form, button, script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]'
        : 'script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let lastBlk = null;
      let lastRow = null;
      let lastCell = null;
      try {
        const walker = document.createTreeWalker(rootNode, 4);
        let n;
        while ((n = walker.nextNode())) {
          const v = n.nodeValue;
          if (!v || !v.trim()) continue;
          const p = n.parentElement;
          if (!p || (skipChrome && composerBar && composerBar.contains(p)) || (p.closest && p.closest(skipSel))) continue;
          if (skipChrome && isOutOfColBounds(p)) continue;
          const row = p.closest ? p.closest('tr') : null;
          const cell = p.closest ? p.closest('td, th') : null;
          const blk = (p.closest && p.closest(blockSel)) || p;
          if (parts.length > 0) {
            if (row && row === lastRow && cell && cell !== lastCell) parts.push(' | ');
            else if (blk !== lastBlk) parts.push('\n');
          }
          parts.push(v.trim());
          lastBlk = blk;
          lastRow = row;
          lastCell = cell;
        }
      } catch (_) {}
      return parts.join('').replace(/\n{3,}/g, '\n\n').trim();
    };
    const getNodeText = (el) => {
      if (!el) return '';
      if (el.querySelector && el.querySelector('table, tr')) {
        return extractCleanText(el, false);
      }
      const t = (el.innerText || '').trim();
      if (t) return t;
      return extractCleanText(el, false);
    };
    const stripTailLite = (raw) => {
      let s = (raw || '').trim();
      for (let k = 0; k < 6; k++) {
        const next = s.replace(/(?:\r?\n|\s|^)(?:\d{1,2}:\d{2}|Muse|Hark|已阅读|已发送|已连接|空闲中|复制|Copy)$/i, '').trim();
        if (next === s) break;
        s = next;
      }
      return s;
    };
    const isOldBaselineText = (t) => {
      const raw = (t || '').trim();
      if (!raw) return true;
      const stripped = stripTailLite(raw);
      const nt = normWs(stripped);
      if (!nt) return true;
      if (job.baselineExact && (job.baselineExact.has(raw) || job.baselineExact.has(stripped))) return true;
      if (job.baselineCleanSet && (job.baselineCleanSet.has(raw) || job.baselineCleanSet.has(stripped))) return true;
      if (job.baselineLines && (job.baselineLines.has(raw) || job.baselineLines.has(stripped))) return true;
      if (nt.length >= 4 && job.baselineMainNorm && job.baselineMainNorm.includes(nt)) return true;
      if (nt.length >= 60 && job.baselineMainNorm) {
        const head48 = nt.slice(0, 48);
        const tail48 = nt.slice(-48);
        if (job.baselineMainNorm.includes(head48) && job.baselineMainNorm.includes(tail48)) return true;
        const tLines = stripped.split(/\r?\n/).map((l) => normWs(l)).filter((l) => l.length >= 12);
        if (tLines.length >= 3) {
          let hits = 0;
          for (const ln of tLines) {
            if (job.baselineMainNorm.includes(ln)) hits++;
          }
          if (hits / tLines.length >= 0.7) return true;
        }
      }
      if (nt.length >= 24 && Array.isArray(job.baselineBlockNorms)) {
        for (const b of job.baselineBlockNorms) {
          if (b.includes(nt) || (nt.includes(b) && b.length >= nt.length * 0.75)) return true;
        }
      }
      return false;
    };
    const isBaselineNode = (el) => Boolean(el && job.baselineDomEls && job.baselineDomEls.has(el));
    const isAfterLastBaseline = (el) => Boolean(
      el
      && job.lastBaselineEl
      && job.lastBaselineEl.isConnected
      && el !== job.lastBaselineEl
      && !el.contains(job.lastBaselineEl)
      && !job.lastBaselineEl.contains(el)
      && (job.lastBaselineEl.compareDocumentPosition(el) & 4)
    );

    const adapterAsst = Array.isArray(adapter.assistantMessageSelectors) ? adapter.assistantMessageSelectors.filter(Boolean) : [];
    const baseMsgSelector = '[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], [data-testid*="assistant" i], [data-testid*="response" i], [data-testid*="turn" i], article, [class*="markdown" i], [class*="prose" i]';
    const allNodesSelector = adapterAsst.length ? adapterAsst.join(', ') + ', ' + baseMsgSelector : baseMsgSelector;
    let rawNodeList = [];
    try { rawNodeList = Array.from(searchRoot.querySelectorAll(allNodesSelector)); } catch (_) { rawNodeList = Array.from(searchRoot.querySelectorAll(baseMsgSelector)); }
    const allNodes = rawNodeList.filter((el) => !isExcluded(el) && !isMessageWrapper(el) && isVisibleEl(el, 20, 12));

    const fullMain = extractCleanText(searchRoot, true);
    const endMarker = '(Please answer the “Current question” directly, and do not repeat the bracketed headings above)';
    const endMarkerZh = '（请直接回答「当前问题」，不要复述上方方括号标题）';
    const markerIdxEn = fullMain.lastIndexOf(endMarker);
    const markerIdxZh = fullMain.lastIndexOf(endMarkerZh);
    const markerIdx = Math.max(markerIdxEn, markerIdxZh);
    const markerLen = markerIdxEn >= markerIdxZh ? endMarker.length : endMarkerZh.length;
    const needleIdx = job.promptNeedle ? fullMain.lastIndexOf(job.promptNeedle) : -1;
    const curMarkerCount = fullMain ? ((fullMain.split(endMarker).length - 1) + (fullMain.split(endMarkerZh).length - 1)) : 0;
    const curNeedleCount = (fullMain && job.promptNeedle) ? (fullMain.split(job.promptNeedle).length - 1) : 0;
    const curHeadCount = (fullMain && job.promptHead) ? (fullMain.split(job.promptHead).length - 1) : 0;
    const hasNewMarker = curMarkerCount > (job.baselineMarkerCount || 0);
    const hasNewNeedle = curNeedleCount > (job.baselineNeedleCount || 0);
    const hasNewHead = curHeadCount > (job.baselineHeadCount || 0);
    const hasNewUserTurn = hasNewMarker || hasNewNeedle || hasNewHead;

    let userIdx = -1;
    const needleFull = job.promptNeedle || '';
    const needlePrefix = needleFull.length > 24 ? needleFull.slice(0, 24) : needleFull;
    const headFull = job.promptHead || '';
    const headPrefix = headFull.length > 24 ? headFull.slice(0, 24) : headFull;
    const matchesPromptSig = (txt) => Boolean(
      txt && (
        (needleFull && txt.includes(needleFull))
        || (needlePrefix.length >= 6 && txt.includes(needlePrefix))
        || (headFull && txt.includes(headFull))
        || (headPrefix.length >= 8 && txt.includes(headPrefix))
      )
    );
    if (needlePrefix || headPrefix || hasNewMarker) {
      for (let i = allNodes.length - 1; i >= 0; i--) {
        const el = allNodes[i];
        const rawT = getNodeText(el);
        if (!rawT) continue;
        if (isBaselineNode(el) && (!hasNewUserTurn || isOldBaselineText(rawT))) continue;
        if (matchesPromptSig(rawT) || (hasNewMarker && !isBaselineNode(el) && (rawT.includes(endMarker) || rawT.includes(endMarkerZh)))) {
          userIdx = i;
          break;
        }
      }
    }
    let userAnchorEl = userIdx >= 0 ? allNodes[userIdx] : null;
    if (!userAnchorEl && (needlePrefix || headPrefix || hasNewMarker)) {
      try {
        const walker = document.createTreeWalker(searchRoot, 4);
        let tNode;
        while ((tNode = walker.nextNode())) {
          const p = tNode.parentElement;
          if (!p || isExcluded(p)) continue;
          const v = (tNode.nodeValue || '').trim();
          if (!v) continue;
          if (isBaselineNode(p) && (!hasNewUserTurn || isOldBaselineText(v))) continue;
          if (matchesPromptSig(v) || (hasNewMarker && !isBaselineNode(p) && (v.includes(endMarker) || v.includes(endMarkerZh)))) {
            userAnchorEl = p;
          }
        }
      } catch (_) {}
    }

    let userTurnBox = userAnchorEl;
    if (userTurnBox) {
      const trivialSiblingRe = /^(?:\d{1,2}:\d{2}|👍|❤️|😂|🎉|复制|编辑|重试|Copy|Edit|Retry|已发送|已读)$/i;
      let cur = userTurnBox.parentElement;
      while (cur && cur !== searchRoot && cur !== chatCol && cur !== document.body) {
        if (composerBar && cur.contains(composerBar)) break;
        if (job.lastBaselineEl && job.lastBaselineEl.isConnected && cur.contains(job.lastBaselineEl)) break;
        const ct = (cur.innerText || '').trim();
        if (ct.length > promptNorm.length + 120) break;
        let hasAssistantSibling = false;
        let seenBranch = false;
        for (const ch of cur.children) {
          if (ch === userTurnBox || ch.contains(userTurnBox)) {
            seenBranch = true;
            continue;
          }
          if (seenBranch) {
            const st = (ch.innerText || '').trim();
            if (st && !trivialSiblingRe.test(st)) {
              hasAssistantSibling = true;
              break;
            }
          }
        }
        if (hasAssistantSibling) break;
        userTurnBox = cur;
        cur = cur.parentElement;
      }
    }

    const extractAfterUserTurn = (rootNode, turnBox) => {
      if (!rootNode || !turnBox) return { text: '', hasFreshDom: false };
      const skipSel = 'nav, aside, header, footer, form, button, script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let hasFreshDom = false;
      let lastBlk = null;
      let lastRow = null;
      let lastCell = null;
      try {
        const walker = document.createTreeWalker(rootNode, 4);
        let n;
        while ((n = walker.nextNode())) {
          const v = n.nodeValue;
          if (!v || !v.trim()) continue;
          const p = n.parentElement;
          if (!p || (composerBar && composerBar.contains(p)) || (p.closest && p.closest(skipSel))) continue;
          if (turnBox === p || turnBox.contains(p) || (turnBox.compareDocumentPosition(p) & 4) === 0) continue;
          if (isBaselineNode(p) && job.lastBaselineEl && job.lastBaselineEl.isConnected && (job.lastBaselineEl.compareDocumentPosition(p) & 4) === 0) continue;
          if (isOutOfColBounds(p) || !isVisibleEl(p, 1, 1)) continue;
          if (!isBaselineNode(p)) hasFreshDom = true;
          const row = p.closest ? p.closest('tr') : null;
          const cell = p.closest ? p.closest('td, th') : null;
          const blk = (p.closest && p.closest(blockSel)) || p;
          if (parts.length > 0) {
            if (row && row === lastRow && cell && cell !== lastCell) parts.push(' | ');
            else if (blk !== lastBlk) parts.push('\n');
          }
          parts.push(v.trim());
          lastBlk = blk;
          lastRow = row;
          lastCell = cell;
        }
      } catch (_) {}
      return { text: parts.join('').replace(/\n{3,}/g, '\n\n').trim(), hasFreshDom };
    };

    const isBeforeUserAnchor = (el) => {
      const ref = userTurnBox || userAnchorEl;
      if (!ref || !el) return false;
      if (el === ref || el.contains(ref) || ref.contains(el)) return true;
      return (ref.compareDocumentPosition(el) & 4) === 0;
    };

    const sentIntoDom = Boolean(userTurnBox) || userIdx >= 0 || hasNewUserTurn || allNodes.length > (job.baselineCount || 0);
    const stillUnsent = !sentIntoDom && composerVal.length >= Math.min(20, promptNorm.length) && composerVal === promptNorm;

    const now = Date.now();
    const isAsyncAgent = Boolean(adapter.asyncAgent);
    const netState = window.__maamNet;
    const apiPats = Array.isArray(adapter.chatApiPatterns)
      ? adapter.chatApiPatterns.map((p) => String(p || '').toLowerCase()).filter(Boolean)
      : [];
    const netEntries = (netState && Array.isArray(netState.entries))
      ? netState.entries.filter((e) => e && e.id > (job.netStartSeq || 0))
      : [];
    const relevantNet = netEntries.filter((e) => {
      const u = (e.url || '').toLowerCase();
      const patMatch = apiPats.length ? apiPats.some((p) => u.includes(p)) : false;
      return e.method === 'POST' || e.isStream || patMatch;
    });
    const poolNet = relevantNet.filter((e) => e.method === 'POST' || e.isStream);
    const activePostStream = poolNet.some((e) => (e.method === 'POST' || (e.isStream && !isAsyncAgent)) && !e.done && (now - (e.updatedAt || e.startedAt || 0)) < 12000);
    const activePushStream = !isAsyncAgent && Boolean(netState && ((netState.wsLastAt > (job.netStartAt || 0) && now - netState.wsLastAt < 4000) || (netState.esLastAt > (job.netStartAt || 0) && now - netState.esLastAt < 4000)));
    const netGenerating = activePostStream || activePushStream;

    let candidateText = '';
    if (!stillUnsent) {
      const confirmedNewTurn = Boolean(userTurnBox) && (!isBaselineNode(userTurnBox) || hasNewUserTurn);
      if (confirmedNewTurn) {
        const afterRes = extractAfterUserTurn(searchRoot, userTurnBox);
        const afterDom = cleanChrome(afterRes.text);
        if (afterDom && !isSameAsPrompt(afterDom)) {
          if (!isOldBaselineText(afterDom) || (afterRes.hasFreshDom && afterDom.length <= 80)) {
            candidateText = afterDom;
          }
        }
      }

      if (!candidateText && (confirmedNewTurn || hasNewUserTurn || allNodes.length > (job.baselineCount || 0))) {
        const baseAsstSelector = '[data-message-author-role="assistant"], [data-role="assistant"], [data-testid*="assistant" i]';
        const assistantSelector = adapterAsst.length ? adapterAsst.join(', ') + ', ' + baseAsstSelector : baseAsstSelector;
        let rawAsstNodes = [];
        try { rawAsstNodes = Array.from(searchRoot.querySelectorAll(assistantSelector)); } catch (_) { rawAsstNodes = Array.from(searchRoot.querySelectorAll(baseAsstSelector)); }
        const assistantNodes = rawAsstNodes.filter((el) => !isExcluded(el) && !isMessageWrapper(el) && isVisibleEl(el, 20, 12));
        const collectedAsst = [];
        for (let i = 0; i < assistantNodes.length; i++) {
          const node = assistantNodes[i];
          if (isBeforeUserAnchor(node)) continue;
          if (job.baselineEls && job.baselineEls.has(node)) continue;
          if (isBaselineNode(node) && isOldBaselineText(cleanChrome(getNodeText(node)))) continue;
          if (!userTurnBox && job.lastBaselineEl && job.lastBaselineEl.isConnected && !isAfterLastBaseline(node)) continue;
          if (assistantNodes.some((other) => other !== node && other.contains(node))) continue;
          const t = cleanChrome(getNodeText(node));
          if (!t || isSameAsPrompt(t)) continue;
          const allowShortRepeat = confirmedNewTurn && !isBaselineNode(node) && t.length <= 80;
          if (!allowShortRepeat && isOldBaselineText(t)) continue;
          collectedAsst.push(t);
        }
        if (collectedAsst.length > 0) {
          candidateText = userTurnBox ? collectedAsst.join('\n\n') : collectedAsst[collectedAsst.length - 1];
        }
      }

      if (!candidateText && !userTurnBox && (hasNewUserTurn || allNodes.length > (job.baselineCount || 0))) {
        for (let i = allNodes.length - 1; i >= 0; i--) {
          if (i < (job.baselineCount || 0)) continue;
          const el = allNodes[i];
          if (job.baselineEls && job.baselineEls.has(el)) continue;
          if (isBaselineNode(el)) continue;
          if (job.lastBaselineEl && job.lastBaselineEl.isConnected && !isAfterLastBaseline(el)) continue;
          if (el.getAttribute('data-message-author-role') === 'user' || el.getAttribute('data-role') === 'user') continue;
          const rawElText = getNodeText(el);
          if (!rawElText || matchesPromptSig(rawElText)) continue;
          if (job.baselineSet && job.baselineSet.has(rawElText.slice(0, 220))) continue;
          let t = rawElText;
          if (t.startsWith(promptNorm)) t = t.slice(promptNorm.length).trim();
          t = cleanChrome(t);
          if (!t || isSameAsPrompt(t) || isOldBaselineText(t)) continue;
          candidateText = t;
          break;
        }
      }

      if (!candidateText) {
        let sliceStart = -1;
        if (hasNewMarker && markerIdx >= 0) {
          sliceStart = markerIdx + markerLen;
        } else if (hasNewNeedle && needleIdx >= 0) {
          sliceStart = needleIdx + job.promptNeedle.length;
        }
        if (sliceStart >= 0) {
          const after = cleanChrome(fullMain.slice(sliceStart));
          if (after && !isSameAsPrompt(after) && !isOldBaselineText(after)) candidateText = after;
        }
      }

      if (!candidateText) {
        for (let i = poolNet.length - 1; i >= 0; i--) {
          const nt = cleanChrome((poolNet[i].text || '').trim());
          if (nt && !isSameAsPrompt(nt) && !isOldBaselineText(nt)) {
            candidateText = nt;
            break;
          }
        }
        if (!candidateText && netState && netState.esLastAt > (job.netStartAt || 0) && netState.esText) {
          const est = cleanChrome(netState.esText);
          if (est && !isSameAsPrompt(est) && !isOldBaselineText(est)) candidateText = est;
        }
      }
    }

    const cleanupJob = () => {
      if (job && job.observer) {
        try { job.observer.disconnect(); } catch (_) {}
      }
      delete window.__maamGroupJobs[jobId];
    };

    if (statusBusy) {
      domGenerating = true;
    }
    const isGenerating = domGenerating || netGenerating;

    if (candidateText && candidateText !== job.lastText) {
      job.lastText = candidateText;
      job.lastChangedAt = now;
    }

    const stableMs = now - (job.lastChangedAt || job.startedAt);
    const elapsedMs = now - job.startedAt;
    const recentDomMut = Boolean(job.lastDomMutAt && (now - job.lastDomMutAt) < (isAsyncAgent ? 2500 : 1200));
    const effectiveGenerating = isGenerating || recentDomMut;

    if (job.lastText) {
      const trimmedLines = job.lastText.trim().split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
      const lastLine = trimmedLines.length ? trimmedLines[trimmedLines.length - 1] : '';
      const looksIncomplete =
        (job.lastText.length < 24 && !/[。！？.!?]$/.test(job.lastText))
        || /(?:[:：\-—*•]|^(?:#{1,6}\s+|\d+[.、]\s*)[^\n。！？.!?]{1,28})$/.test(lastLine);
      const neededStableMs = isAsyncAgent
        ? (looksIncomplete ? 9000 : (job.lastText.length < 40 ? 6000 : 4500))
        : (looksIncomplete ? 3600 : (job.lastText.length < 24 ? 1800 : 1400));
      const quietFallbackMs = Math.max(neededStableMs + 1500, isAsyncAgent ? 7500 : 4200);
      const noDomFallbackMs = Math.max(neededStableMs + 2000, isAsyncAgent ? 9000 : 4800);
      const hardStableCapMs = isAsyncAgent ? 30000 : 22000;
      if (
        (!effectiveGenerating && stableMs >= neededStableMs)
        || (!isGenerating && stableMs >= quietFallbackMs)
        || (!domGenerating && !recentDomMut && stableMs >= noDomFallbackMs)
        || stableMs >= hardStableCapMs
      ) {
        cleanupJob();
        return JSON.stringify({ state: 'done', text: job.lastText });
      }
      return JSON.stringify({ state: 'streaming', text: job.lastText, transient: false });
    }

    const okPost = [...relevantNet].reverse().find((e) => e.method === 'POST' && e.status >= 200 && e.status < 300);
    const fatalChatNet = [...relevantNet].reverse().find((e) => e.status === 401 || e.status === 403 || e.status === 429 || (e.method === 'POST' && e.status >= 400));
    if (!isGenerating && fatalChatNet && (now - (fatalChatNet.updatedAt || fatalChatNet.startedAt)) >= 2500) {
      cleanupJob();
      const msg = (fatalChatNet.status === 401 || fatalChatNet.status === 403)
        ? ('目标站点鉴权失效（HTTP ' + fatalChatNet.status + '），请前往该账号页面重新登录')
        : fatalChatNet.status === 429
          ? '触发目标站点限流（HTTP 429），请稍后点击重试'
          : ('目标接口报错（HTTP ' + fatalChatNet.status + (fatalChatNet.errorSnippet ? '：' + fatalChatNet.errorSnippet : '') + '）');
      return JSON.stringify({ state: 'error', text: '', error: msg });
    }

    const stuckStillSending = Boolean(progressText && progressText.includes('仍在发送中') && elapsedMs >= 15000);
    if (stuckStillSending) {
      cleanupJob();
      return JSON.stringify({ state: 'stalled_retry', text: '' });
    }

    if (isAsyncAgent && !job.lastText && !progressText && sentIntoDom && elapsedMs >= 7000) {
      progressText = okPost
        ? ('服务端已接收（HTTP ' + okPost.status + '），等待异步回复…')
        : '消息已发出，等待服务端响应…';
    }

    const timedOut = stillUnsent
      ? (elapsedMs >= 20000)
      : isAsyncAgent
        ? (elapsedMs >= 95000)
        : (!isGenerating && elapsedMs >= 65000);
    if (timedOut) {
      cleanupJob();
      const waitedSec = Math.round(elapsedMs / 1000);
      const msg = stillUnsent
        ? '消息已填入但未能自动触发发送，请前往该账号页面检查输入框'
        : okPost
          ? ('服务端已接收消息（HTTP ' + okPost.status + '，已等待 ' + waitedSec + ' 秒），但未返回回复（可能服务端静默跳过或长连接中断），可前往查看或手动重试')
          : (!sentIntoDom && !relevantNet.some((e) => e.method === 'POST'))
            ? '未检测到发送请求发出，请检查该账号登录或网络状态后重试'
            : ('发送后未检测到新回复（已等待 ' + waitedSec + ' 秒），可前往查看或手动重试');
      return JSON.stringify({ state: 'error', text: '', error: msg });
    }

    return JSON.stringify({ state: 'streaming', text: progressText, transient: Boolean(progressText) });
  } catch (err) {
    return JSON.stringify({ state: 'error', text: '', error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
    .replace("__JOB_ID__", &job_json)
}

#[cfg(desktop)]
fn build_group_sync_latest_js(adapter: &SiteAdapterConfig, raw_user_content: &str) -> String {
  let adapter_json = serde_json::to_string(adapter).unwrap_or_else(|_| "{}".to_string());
  let query_json = serde_json::to_string(raw_user_content).unwrap_or_else(|_| "\"\"".to_string());
  r#"(() => {
  try {
    const adapter = __ADAPTER__;
    const rawUserContent = __QUERY__;
    const endMarker = '(Please answer the “Current question” directly, and do not repeat the bracketed headings above)';
    const endMarkerZh = '（请直接回答「当前问题」，不要复述上方方括号标题）';

    const isVisibleEl = (el, minW = 16, minH = 12) => {
      if (!el || !el.isConnected) return false;
      const r = el.getBoundingClientRect();
      if (r.width < minW || r.height < minH) return false;
      let cur = el;
      for (let d = 0; cur && cur.nodeType === 1 && d < 8; d++) {
        try {
          const st = getComputedStyle(cur);
          if (st.display === 'none' || st.visibility === 'hidden') return false;
          const op = parseFloat(st.opacity);
          if (!Number.isNaN(op) && op < 0.05) return false;
        } catch (_) {}
        cur = cur.parentElement;
      }
      return true;
    };

    const resolveActiveComposer = () => {
      const compSels = Array.isArray(adapter.composerSelectors) ? adapter.composerSelectors : [];
      for (const sel of compSels) {
        if (!sel) continue;
        try {
          const list = Array.from(document.querySelectorAll(sel)).filter((el) => isVisibleEl(el, 80, 16));
          if (list.length > 0) {
            list.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
            return list[0];
          }
        } catch (_) {}
      }
      const fallback = Array.from(document.querySelectorAll('textarea, [contenteditable="true"], [role="textbox"]'))
        .filter((el) => isVisibleEl(el, 100, 16));
      fallback.sort((a, b) => b.getBoundingClientRect().bottom - a.getBoundingClientRect().bottom);
      return fallback[0] || null;
    };
    const resolveChatColumn = (comp) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (comp && comp.isConnected) {
        let cur = comp.parentElement;
        while (cur && cur !== document.body && cur !== document.documentElement) {
          const r = cur.getBoundingClientRect();
          if (r.height >= winH * 0.5 && r.width >= 220) {
            return cur;
          }
          cur = cur.parentElement;
        }
      }
      return document.querySelector('main, [role="main"]') || document.body;
    };
    const resolveComposerBar = (comp, col) => {
      const winH = window.innerHeight || document.documentElement.clientHeight || 800;
      if (!comp || !comp.isConnected) return null;
      let bar = comp;
      let cur = comp.parentElement;
      while (cur && cur !== col && cur !== document.body) {
        const r = cur.getBoundingClientRect();
        if (r.height >= winH * 0.36) break;
        bar = cur;
        cur = cur.parentElement;
      }
      return bar;
    };

    const activeComposer = resolveActiveComposer();
    const chatCol = resolveChatColumn(activeComposer);
    const composerBar = resolveComposerBar(activeComposer, chatCol);
    const colRect = (chatCol && chatCol !== document.body) ? chatCol.getBoundingClientRect() : null;

    let scopedList = null;
    const adapterListSels = Array.isArray(adapter.messageListSelectors) ? adapter.messageListSelectors : [];
    for (const sel of adapterListSels) {
      if (!sel || sel === 'main') continue;
      try {
        const el = chatCol.querySelector(sel);
        if (el && el.getBoundingClientRect().height > 10) { scopedList = el; break; }
      } catch (_) {}
    }
    if (!scopedList) {
      scopedList = chatCol.querySelector('[data-testid="message-list"], [data-testid*="chat-messages" i], [data-testid*="message-feed" i]');
    }
    const searchRoot = scopedList || chatCol;
    const isOutOfColBounds = (el) => {
      if (!colRect || colRect.width < 120 || !el || !el.getBoundingClientRect) return false;
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0) return false;
      return r.right < colRect.left - 16 || r.left > colRect.right + 16;
    };
    const isExcluded = (el) => Boolean(
      (composerBar && composerBar.contains(el))
      || (el.closest && el.closest('nav, aside, header, footer, form'))
      || isOutOfColBounds(el)
    );
    const isMessageWrapper = (el) => {
      const tid = (el.getAttribute('data-testid') || '').toLowerCase();
      if (/list|container|scroll|wrapper|feed|history|timestamp|time|date|header|footer|avatar/.test(tid)) return true;
      const nested = el.querySelectorAll('[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], article').length;
      return nested > 1;
    };
    const extractCleanText = (rootNode, skipChrome) => {
      if (!rootNode) return '';
      const skipSel = skipChrome
        ? 'nav, aside, header, footer, form, button, script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]'
        : 'script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let lastBlk = null;
      let lastRow = null;
      let lastCell = null;
      try {
        const walker = document.createTreeWalker(rootNode, 4);
        let n;
        while ((n = walker.nextNode())) {
          const v = n.nodeValue;
          if (!v || !v.trim()) continue;
          const p = n.parentElement;
          if (!p || (skipChrome && composerBar && composerBar.contains(p)) || (p.closest && p.closest(skipSel))) continue;
          if (skipChrome && isOutOfColBounds(p)) continue;
          const row = p.closest ? p.closest('tr') : null;
          const cell = p.closest ? p.closest('td, th') : null;
          const blk = (p.closest && p.closest(blockSel)) || p;
          if (parts.length > 0) {
            if (row && row === lastRow && cell && cell !== lastCell) parts.push(' | ');
            else if (blk !== lastBlk) parts.push('\n');
          }
          parts.push(v.trim());
          lastBlk = blk;
          lastRow = row;
          lastCell = cell;
        }
      } catch (_) {}
      return parts.join('').replace(/\n{3,}/g, '\n\n').trim();
    };
    const getNodeText = (el) => {
      if (!el) return '';
      if (el.querySelector && el.querySelector('table, tr')) {
        return extractCleanText(el, false);
      }
      const t = (el.innerText || '').trim();
      if (t) return t;
      return extractCleanText(el, false);
    };

    const extraWords = Array.isArray(adapter.transientStatusWords)
      ? adapter.transientStatusWords.map((w) => String(w || '').trim()).filter(Boolean)
      : [];
    const isIdleOnlyStatus = (s) => /^(?:(?:Muse|Hark|空闲中|已连接|已阅读|消息|Idle|Connected)[….\s]*)+$/i.test((s || '').trim());
    const isTransientStatusText = (s) => {
      const t = (s || '').trim();
      if (!t) return false;
      if (extraWords.some((w) => t === w || t === ('Muse\n' + w) || t === ('Hark\n' + w))) {
        return true;
      }
      return /^(?:(?:Muse|Hark)[ \t\r\n]+(?:正在[\u4e00-\u9fa5]{1,10}|[\u4e00-\u9fa5]{1,8}中|已连接|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)[….\s]*|(?:Muse|Hark)[ \t\r\n]*|(?:正在预热|预热中|空闲中|已连接|连接中|正在连接|仍在发送中|发送中|努力工作中|工作中|努力思考中|正在思考|思考中|正在搜索|正在搜索网页|正在联网搜索|搜索中|正在检索|检索中|正在回复|回复中|正在分析|分析中|正在查询|查询中|正在处理|处理中|正在生成|生成中|正在准备|准备中|加载中|待命中|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)[….\s]*)+$/i.test(t);
    };
    const isTransientProgressOnly = (txt) => {
      const s = (txt || '').trim();
      if (!s) return false;
      if (isTransientStatusText(s)) return !isIdleOnlyStatus(s);
      const lines = s.split(/\r?\n/).map((l) => l.trim().replace(/[。….\s]+$/g, '')).filter(Boolean);
      if (lines.length === 0 || lines.length > 3 || s.length > 100) return false;
      return lines.every((l) =>
        /^(?:Muse|Hark|ChatGPT|Claude|Gemini|DeepSeek)$/i.test(l)
        || /^(?:正在|努力|准备|开始)[^\n。！？!?]{1,32}$/.test(l)
        || /^[\u4e00-\u9fa5A-Za-z0-9 _\-]{1,18}中$/.test(l)
        || /^(?:正在|努力|开始)?(?:搜索|联网搜索|检索|查询|思考|深度思考|分析|阅读|读取|读入|解析|打开|下载|提取|整合|合并|修订|完善|优化|确认|等待|接收|获取|拉取|同步|写入|保存|浏览|整理|生成|处理|准备|加载|连接|预热|调用|执行|工作|研究|检查|审阅|评审|核对|评估|起草|撰写|编写|规划|制定|综合|汇总|梳理|对比|查看|构思|草拟|校验|复核|总结)[^\n。！？!?]{0,30}$/.test(l)
        || /^(?:已(?:搜索|阅读|读取|检索|解析|接收|获取|加载|打开|提取|合并|整合|浏览|思考|检查|审阅|核对)[^\n。！？!?]{0,24})$/.test(l)
        || /^(?:Searching|Researching|Thinking|Analyzing|Reading|Browsing|Generating|Loading|Fetching|Parsing|Opening|Merging|Synthesizing|Processing|Preparing|Running|Calling|Executing|Working|Warming\s*up|Sending|Retrieving|Checking|Reviewing|Drafting|Writing|Planning|Summarizing|Verifying|Evaluating)\b[^\n.!?]{0,70}$/i.test(l)
      );
    };
    const escRe = (v) => v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const cleanChrome = (txt) => {
      let s = (txt || '')
        .replace(/(\r?\n\s*(复制|Copy|Copy code|Regenerate|重新生成|重试|Retry|Share|分享|Edit|编辑|Like|Dislike)\s*)+$/gi, '')
        .trim();
      if (isTransientProgressOnly(s) || isTransientStatusText(s)) return '';
      const leadStatusRe = /^(?:(?:Muse|Hark)\s*\r?\n+|(?:正在预热|预热中|空闲中|仍在发送中|发送中|正在搜索网页|正在联网搜索|正在搜索|搜索中|正在思考|思考中|读取中|解析中|处理中|生成中|正在[^\r\n。！？!?]{1,28}[。….]?|已(?:搜索|阅读|读取|检索|解析)[^\r\n]{0,20}[。….]?|Thinking[.\s]*|Searching[.\s]*|Reading[.\s]*|Thought for \d+s?)\s*\r?\n+)+/i;
      s = s.replace(leadStatusRe, '').trim();
      const leadSpeakerRe = /^(?:(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]?\s*\r?\n+|(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]\s*)+/i;
      s = s.replace(leadSpeakerRe, '').trim();
      if (/^(?:Gemini|ChatGPT|Claude|DeepSeek|Grok|Kimi)\s*(?:说|said|says)\s*[:：]?$/i.test(s)) return '';
      const tailRe = /(?:(?:Muse|Hark)[ \t\r\n]+(?:正在[\u4e00-\u9fa5]{1,10}|[\u4e00-\u9fa5]{1,8}中|已连接|已阅读|消息|Thinking|Warming\s*up|Sending|Working|Searching|Researching|Replying|Retrieving|Loading|Idle|Connected)(?:[ \t\r\n]+消息)?)+$|(?:\r?\n|\s|^)(?:Muse|Hark|已连接|连接中|正在连接|正在预热|预热中|空闲中|仍在发送中|发送中|努力工作中|工作中|努力思考中|正在思考|思考中|正在搜索|正在搜索网页|搜索中|正在检索|检索中|正在回复|回复中|正在分析|分析中|正在查询|查询中|正在处理|处理中|正在生成|生成中|正在准备|准备中|加载中|待命中|消息|停止|发送|Sending|已阅读|给\s*Hark\s*发消息|附加文件|开始听写|添加附件|发送消息|留下心情|共享页面|更多选项|Go to Ideas|Customize your Feed|\d{1,2}:\d{2})$/i;
      for (let k = 0; k < 10; k++) {
        let next = s.replace(tailRe, '').trim();
        for (const ew of extraWords) {
          const ewRe = new RegExp('(?:\\r?\\n|\\s|^)(?:(?:Muse|Hark)\\s+)?' + escRe(ew) + '$', 'i');
          next = next.replace(ewRe, '').trim();
        }
        if (next === s) break;
        s = next;
      }
      if (isTransientProgressOnly(s) || isTransientStatusText(s)) return '';
      return s;
    };

    const normWs = (s) => (s || '').replace(/\s+/g, ' ').trim();
    const rawQ = (rawUserContent || '').trim();
    const qLines = rawQ.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
    qLines.sort((a, b) => b.length - a.length);
    const qLongest = qLines[0] || rawQ;
    const qNeedle = qLongest.slice(0, 48);
    const qPrefix = qNeedle.length > 24 ? qNeedle.slice(0, 24) : qNeedle;
    const rawQNorm = normWs(rawQ);
    const isPromptOverlap = (t) => {
      const s = (t || '').trim();
      if (!s) return true;
      if (rawQ && s === rawQ) return true;
      const ns = normWs(s);
      if (rawQNorm && ns.length >= 12 && rawQNorm.includes(ns)) return true;
      if ((s.includes('不要复述上方方括号标题') || s.includes('不要重复上方协议标签') || s.includes('不要重复以上协议标签') || s.includes('do not repeat the bracketed headings above')) && (s.includes('[当前问题]') || s.includes('[用户问题]') || s.includes('[当前群聊发言]') || s.includes('[Current question]') || s.includes('[User question]') || s.includes('[Current group chat message]'))) return true;
      if ((s.startsWith('[阶段要求]') || s.startsWith('[Stage requirements]')) && (s.includes(endMarker) || s.includes(endMarkerZh))) return true;
      return false;
    };

    const adapterAsst = Array.isArray(adapter.assistantMessageSelectors) ? adapter.assistantMessageSelectors.filter(Boolean) : [];
    const baseMsgSelector = '[data-message-author-role], [data-role], [data-testid="message-bubble"], [data-testid="message-text"], [data-testid*="assistant" i], [data-testid*="response" i], [data-testid*="turn" i], article, [class*="markdown" i], [class*="prose" i]';
    const allNodesSelector = adapterAsst.length ? adapterAsst.join(', ') + ', ' + baseMsgSelector : baseMsgSelector;
    let rawNodeList = [];
    try { rawNodeList = Array.from(searchRoot.querySelectorAll(allNodesSelector)); } catch (_) { rawNodeList = Array.from(searchRoot.querySelectorAll(baseMsgSelector)); }
    const allNodes = rawNodeList.filter((el) => !isExcluded(el) && !isMessageWrapper(el) && isVisibleEl(el, 20, 12));

    let userAnchorEl = null;
    if (qPrefix.length >= 4) {
      for (let i = allNodes.length - 1; i >= 0; i--) {
        const rawT = getNodeText(allNodes[i]);
        if (rawT && ((qNeedle && rawT.includes(qNeedle)) || (qPrefix.length >= 6 && rawT.includes(qPrefix)))) {
          userAnchorEl = allNodes[i];
          break;
        }
      }
      if (!userAnchorEl) {
        try {
          const walker = document.createTreeWalker(searchRoot, 4);
          let tNode;
          while ((tNode = walker.nextNode())) {
            const p = tNode.parentElement;
            if (!p || isExcluded(p)) continue;
            const v = (tNode.nodeValue || '').trim();
            if (v && ((qNeedle && v.includes(qNeedle)) || (qPrefix.length >= 6 && v.includes(qPrefix)))) {
              userAnchorEl = p;
            }
          }
        } catch (_) {}
      }
    }
    if (!userAnchorEl) {
      for (let i = allNodes.length - 1; i >= 0; i--) {
        const rawT = getNodeText(allNodes[i]);
        if (rawT && (rawT.includes(endMarker) || rawT.includes(endMarkerZh) || rawT.includes('[当前问题]') || rawT.includes('[Current question]') || rawT.includes('[阶段要求]') || rawT.includes('[Stage requirements]'))) {
          userAnchorEl = allNodes[i];
          break;
        }
      }
      if (!userAnchorEl) {
        try {
          const walker = document.createTreeWalker(searchRoot, 4);
          let tNode;
          while ((tNode = walker.nextNode())) {
            const p = tNode.parentElement;
            if (!p || isExcluded(p)) continue;
            const v = (tNode.nodeValue || '').trim();
            if (v && (v.includes(endMarker) || v.includes(endMarkerZh) || v.includes('[当前问题]') || v.includes('[Current question]') || v.includes('[阶段要求]') || v.includes('[Stage requirements]'))) {
              userAnchorEl = p;
            }
          }
        } catch (_) {}
      }
    }

    let userTurnBox = userAnchorEl;
    if (userTurnBox) {
      const trivialSiblingRe = /^(?:\d{1,2}:\d{2}|👍|❤️|😂|🎉|复制|编辑|重试|Copy|Edit|Retry|已发送|已读)$/i;
      let cur = userTurnBox.parentElement;
      while (cur && cur !== searchRoot && cur !== chatCol && cur !== document.body) {
        if (composerBar && cur.contains(composerBar)) break;
        let hasAssistantSibling = false;
        let seenBranch = false;
        for (const ch of cur.children) {
          if (ch === userTurnBox || ch.contains(userTurnBox)) {
            seenBranch = true;
            continue;
          }
          if (seenBranch) {
            const st = (ch.innerText || '').trim();
            if (st && !trivialSiblingRe.test(st)) {
              hasAssistantSibling = true;
              break;
            }
          }
        }
        if (hasAssistantSibling) break;
        userTurnBox = cur;
        cur = cur.parentElement;
      }
    }

    const extractAfterUserTurn = (rootNode, turnBox) => {
      if (!rootNode || !turnBox) return '';
      const skipSel = 'nav, aside, header, footer, form, button, script, style, time, svg, noscript, template, [hidden], [aria-hidden="true"]';
      const blockSel = 'p, div, li, tr, h1, h2, h3, h4, h5, h6, pre, blockquote, article, section';
      const parts = [];
      let lastBlk = null;
      let lastRow = null;
      let lastCell = null;
      try {
        const walker = document.createTreeWalker(rootNode, 4);
        let n;
        while ((n = walker.nextNode())) {
          const v = n.nodeValue;
          if (!v || !v.trim()) continue;
          const p = n.parentElement;
          if (!p || (composerBar && composerBar.contains(p)) || (p.closest && p.closest(skipSel))) continue;
          if (turnBox === p || turnBox.contains(p) || (turnBox.compareDocumentPosition(p) & 4) === 0) continue;
          if (isOutOfColBounds(p) || !isVisibleEl(p, 1, 1)) continue;
          const row = p.closest ? p.closest('tr') : null;
          const cell = p.closest ? p.closest('td, th') : null;
          const blk = (p.closest && p.closest(blockSel)) || p;
          if (parts.length > 0) {
            if (row && row === lastRow && cell && cell !== lastCell) parts.push(' | ');
            else if (blk !== lastBlk) parts.push('\n');
          }
          parts.push(v.trim());
          lastBlk = blk;
          lastRow = row;
          lastCell = cell;
        }
      } catch (_) {}
      return parts.join('').replace(/\n{3,}/g, '\n\n').trim();
    };

    const isBeforeUserAnchor = (el) => {
      const ref = userTurnBox || userAnchorEl;
      if (!ref || !el) return false;
      if (el === ref || el.contains(ref) || ref.contains(el)) return true;
      return (ref.compareDocumentPosition(el) & 4) === 0;
    };

    let text = '';
    if (userTurnBox) {
      const afterDom = cleanChrome(extractAfterUserTurn(searchRoot, userTurnBox));
      if (afterDom && !isPromptOverlap(afterDom)) {
        text = afterDom;
      }
    }

    if (!text) {
      const baseAsstSelector = '[data-message-author-role="assistant"], [data-role="assistant"], [data-testid*="assistant" i]';
      const assistantSelector = adapterAsst.length ? adapterAsst.join(', ') + ', ' + baseAsstSelector : baseAsstSelector;
      let rawAsstNodes = [];
      try { rawAsstNodes = Array.from(searchRoot.querySelectorAll(assistantSelector)); } catch (_) { rawAsstNodes = Array.from(searchRoot.querySelectorAll(baseAsstSelector)); }
      const assistantNodes = rawAsstNodes.filter((el) => !isExcluded(el) && !isMessageWrapper(el) && isVisibleEl(el, 20, 12));
      const collected = [];
      for (let i = 0; i < assistantNodes.length; i++) {
        const node = assistantNodes[i];
        if (userTurnBox && isBeforeUserAnchor(node)) continue;
        if (assistantNodes.some((other) => other !== node && other.contains(node))) continue;
        const t = cleanChrome(getNodeText(node));
        if (!t || isPromptOverlap(t)) continue;
        collected.push(t);
      }
      if (collected.length > 0) {
        text = userTurnBox ? collected.join('\n\n') : collected[collected.length - 1];
      }
    }

    if (!text) {
      for (let i = allNodes.length - 1; i >= 0; i--) {
        const el = allNodes[i];
        if (userTurnBox && isBeforeUserAnchor(el)) continue;
        if (el.getAttribute('data-message-author-role') === 'user' || el.getAttribute('data-role') === 'user') continue;
        const rawT = getNodeText(el);
        if (!rawT || rawT.includes('[阶段要求]') || rawT.includes('[Stage requirements]') || rawT.includes(endMarker) || rawT.includes(endMarkerZh) || (qNeedle && rawT.includes(qNeedle))) continue;
        const t = cleanChrome(rawT);
        if (!t || isPromptOverlap(t)) continue;
        text = t;
        break;
      }
    }

    return JSON.stringify({
      ok: Boolean(text),
      text: text || '',
      error: text ? '' : '未在当前页面检测到可同步的助手回复',
    });
  } catch (err) {
    return JSON.stringify({ ok: false, text: '', error: String(err) });
  }
})()"#
    .replace("__ADAPTER__", &adapter_json)
    .replace("__QUERY__", &query_json)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPageError {
  pub provider_id: ProviderId,
  pub uuid: String,
  pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSnapshot {
  pub platform: &'static str,
  pub providers: Vec<ProviderInfo>,
  pub accounts: Vec<AccountRecord>,
  pub selected_provider_id: ProviderId,
  pub active_account: Option<AccountKey>,
  pub switching_account: Option<AccountKey>,
  pub warm_accounts: Vec<AccountKey>,
  pub last_committed: Option<AccountKey>,
  pub pending_cleanup: Vec<AccountKey>,
  pub conflicts: Vec<ConflictItem>,
  pub notice: Option<String>,
  pub modal_active: bool,
  pub settings: AppSettings,
  pub global_shortcut_active: bool,
  pub global_shortcut_error: Option<String>,
  pub group_chats: Vec<GroupChatRecord>,
  pub active_group_id: Option<String>,
  pub external_agents: Vec<ExternalAgentRecord>,
  pub account_errors: Vec<AccountPageError>,
}

#[cfg(desktop)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadNoticeEvent {
  pub id: u64,
  pub filename: String,
  pub status: &'static str,
  pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PendingExternalAuth {
  account_key: AccountKey,
  switch_seq: u64,
  nonce: String,
}

#[cfg(desktop)]
struct WarmDesktopEntry {
  account_key: AccountKey,
  view_instance_id: u64,
  webview_label: String,
  popup_label: Option<String>,
  paint_ready: bool,
  staged_painted: bool,
  navigation_reached: bool,
  load_gen: Arc<AtomicU64>,
  last_url: Option<Url>,
  captured_identity_hint: Option<String>,
  watcher_running: bool,
  probe_notify: Arc<tokio::sync::Notify>,
  last_used_ms: u64,
  alive_flag: Arc<AtomicBool>,
}

struct ActiveSessionSlot {
  account_key: AccountKey,
  view_instance_id: u64,
  #[cfg_attr(desktop, allow(dead_code))]
  switch_seq: u64,
  committed: bool,
}

#[cfg_attr(not(desktop), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AccountQuotaEval {
  depleted: bool,
  sticky_ok: bool,
  tier: u8,
  effective_used_pct: u8,
}

#[cfg_attr(not(desktop), allow(dead_code))]
fn is_quota_value_depleted(val: &str) -> bool {
  val.contains("用尽")
    || val.contains("耗尽")
    || val.contains("DEPLETED")
    || val.contains("剩余 0")
    || val.contains("剩余0")
}

#[cfg_attr(not(desktop), allow(dead_code))]
fn is_quota_value_low(val: &str) -> bool {
  val.contains("偏低") || val.contains("LOW")
}

#[cfg_attr(not(desktop), allow(dead_code))]
fn evaluate_account_quota(acc: &AccountRecord) -> AccountQuotaEval {
  if let Some(d) = &acc.quota_detail {
    let plan_used = d.plan_used_percent.map(|p| p.min(100));
    let plan_val = d.plan_value.as_str();
    let plan_depleted = plan_used.is_some_and(|p| p >= 100) || is_quota_value_depleted(plan_val);
    let plan_low = !plan_depleted
      && (plan_used.is_some_and(|p| p >= AUTO_ROUTE_STICKY_MAX_USED_PCT)
        || is_quota_value_low(plan_val));

    let has_extra = d.extra_label.is_some()
      || d.extra_value.is_some()
      || d.extra_used_percent.is_some();
    let extra_used = d.extra_used_percent.map(|p| p.min(100));
    let extra_val = d.extra_value.as_deref().unwrap_or("");
    let extra_depleted = has_extra
      && (extra_used.is_some_and(|p| p >= 100) || is_quota_value_depleted(extra_val));
    let extra_low = has_extra
      && !extra_depleted
      && (extra_used.is_some_and(|p| p >= AUTO_ROUTE_STICKY_MAX_USED_PCT)
        || is_quota_value_low(extra_val));
    let extra_is_cap = d.extra_label.as_deref().is_some_and(|lbl| {
      let lower = lbl.to_ascii_lowercase();
      lbl.contains('月') || lower.contains("monthly")
    });

    if extra_is_cap {
      if plan_depleted || extra_depleted {
        return AccountQuotaEval {
          depleted: true,
          sticky_ok: false,
          tier: 4,
          effective_used_pct: 100,
        };
      }
      let effective_used_pct = plan_used.unwrap_or(0).max(extra_used.unwrap_or(0));
      let low = plan_low || extra_low;
      return AccountQuotaEval {
        depleted: false,
        sticky_ok: !low,
        tier: if low { 1 } else { 0 },
        effective_used_pct,
      };
    }

    if plan_depleted {
      if !has_extra || extra_depleted {
        return AccountQuotaEval {
          depleted: true,
          sticky_ok: false,
          tier: 4,
          effective_used_pct: 100,
        };
      }
      return AccountQuotaEval {
        depleted: false,
        sticky_ok: !extra_low,
        tier: if extra_low { 2 } else { 1 },
        effective_used_pct: extra_used.unwrap_or(if extra_low { 90 } else { 50 }),
      };
    }

    let sticky_ok = !plan_low;
    let effective_used_pct = plan_used.unwrap_or(if plan_low { 90 } else { 50 });
    return AccountQuotaEval {
      depleted: false,
      sticky_ok,
      tier: if plan_low { 1 } else { 0 },
      effective_used_pct,
    };
  }

  if let Some(rem) = acc.quota_percent {
    let used = 100u8.saturating_sub(rem.min(100));
    if used >= 100 || acc.quota_text.as_deref().is_some_and(is_quota_value_depleted) {
      return AccountQuotaEval {
        depleted: true,
        sticky_ok: false,
        tier: 4,
        effective_used_pct: 100,
      };
    }
    let low = used >= AUTO_ROUTE_STICKY_MAX_USED_PCT
      || acc.quota_text.as_deref().is_some_and(is_quota_value_low);
    return AccountQuotaEval {
      depleted: false,
      sticky_ok: !low,
      tier: if low { 1 } else { 0 },
      effective_used_pct: used,
    };
  }

  if let Some(txt) = acc.quota_text.as_deref() {
    if is_quota_value_depleted(txt) {
      return AccountQuotaEval {
        depleted: true,
        sticky_ok: false,
        tier: 4,
        effective_used_pct: 100,
      };
    }
    if is_quota_value_low(txt) {
      return AccountQuotaEval {
        depleted: false,
        sticky_ok: false,
        tier: 1,
        effective_used_pct: 90,
      };
    }
  }

  AccountQuotaEval {
    depleted: false,
    sticky_ok: true,
    tier: 3,
    effective_used_pct: 50,
  }
}

struct InnerState {
  store_path: PathBuf,
  app_data_dir: PathBuf,
  store: AccountsStoreFile,
  conflicts: Vec<ConflictItem>,
  selected_provider_id: ProviderId,
  active_slot: Option<ActiveSessionSlot>,
  last_active_per_provider: HashMap<ProviderId, AccountKey>,
  #[cfg(desktop)]
  warm_pool: HashMap<AccountKey, WarmDesktopEntry>,
  #[cfg(desktop)]
  prewarm_failed: HashSet<AccountKey>,
  #[cfg(desktop)]
  cookie_checked_at: HashMap<AccountKey, u64>,
  #[cfg(desktop)]
  in_flight_group_members: HashMap<AccountKey, usize>,
  #[cfg(desktop)]
  group_context_synced: HashMap<(String, AccountKey), (u64, u64)>,
  #[cfg(desktop)]
  group_member_locks: HashMap<AccountKey, Arc<tokio::sync::Mutex<()>>>,
  #[cfg_attr(not(desktop), allow(dead_code))]
  group_channel_last_account: HashMap<(String, ProviderId), AccountKey>,
  #[cfg_attr(not(desktop), allow(dead_code))]
  account_last_assigned_ms: HashMap<AccountKey, u64>,
  #[cfg_attr(not(desktop), allow(dead_code))]
  account_cooldown_until_ms: HashMap<AccountKey, u64>,
  account_page_errors: HashMap<AccountKey, String>,
  cookie_identity_hints: HashMap<AccountKey, String>,
  pending_external_auth: Option<PendingExternalAuth>,
  notice: Option<String>,
  modal_active: bool,
  global_shortcut_active: bool,
  global_shortcut_error: Option<String>,
  active_group_id: Option<String>,
}

pub struct SessionManager {
  current_seq: Arc<AtomicU64>,
  active_view_id: Arc<AtomicU64>,
  view_counter: AtomicU64,
  last_sync_timestamp: AtomicU64,
  minimize_to_tray: AtomicBool,
  auto_approve: AtomicBool,
  #[cfg(desktop)]
  download_counter: AtomicU64,
  #[cfg(desktop)]
  recent_downloads: std::sync::Mutex<HashMap<u64, PathBuf>>,
  #[cfg(desktop)]
  group_worker_semaphore: Arc<tokio::sync::Semaphore>,
  inner: Mutex<InnerState>,
}

impl SessionManager {
  pub fn new(app_data_dir: PathBuf) -> Self {
    let store_path = app_data_dir.join("accounts.json");
    let loaded = load_or_migrate_store(&store_path);
    let initial_minimize = loaded.store.settings.minimize_to_tray;
    let initial_auto_approve = loaded.store.settings.auto_approve;
    let initial_provider = loaded
      .store
      .last_committed
      .as_ref()
      .map(|k| k.provider_id.clone())
      .filter(|pid| {
        resolve_provider(pid).is_some_and(|spec| spec.info.enabled)
      })
      .unwrap_or_else(ProviderId::muse);

    Self {
      current_seq: Arc::new(AtomicU64::new(1)),
      active_view_id: Arc::new(AtomicU64::new(0)),
      view_counter: AtomicU64::new(1),
      last_sync_timestamp: AtomicU64::new(0),
      minimize_to_tray: AtomicBool::new(initial_minimize),
      auto_approve: AtomicBool::new(initial_auto_approve),
      #[cfg(desktop)]
      download_counter: AtomicU64::new(1),
      #[cfg(desktop)]
      recent_downloads: std::sync::Mutex::new(HashMap::new()),
      #[cfg(desktop)]
      group_worker_semaphore: Arc::new(tokio::sync::Semaphore::new(GROUP_MAX_CONCURRENT_WORKERS)),
      inner: Mutex::new(InnerState {
        store_path,
        app_data_dir,
        store: loaded.store,
        conflicts: loaded.conflicts,
        selected_provider_id: initial_provider,
        active_slot: None,
        last_active_per_provider: HashMap::new(),
        #[cfg(desktop)]
        warm_pool: HashMap::new(),
        #[cfg(desktop)]
        prewarm_failed: HashSet::new(),
        #[cfg(desktop)]
        cookie_checked_at: HashMap::new(),
        #[cfg(desktop)]
        in_flight_group_members: HashMap::new(),
        #[cfg(desktop)]
        group_context_synced: HashMap::new(),
        #[cfg(desktop)]
        group_member_locks: HashMap::new(),
        group_channel_last_account: HashMap::new(),
        account_last_assigned_ms: HashMap::new(),
        account_cooldown_until_ms: HashMap::new(),
        account_page_errors: HashMap::new(),
        cookie_identity_hints: HashMap::new(),
        pending_external_auth: None,
        notice: loaded.startup_notice,
        modal_active: false,
        global_shortcut_active: false,
        global_shortcut_error: None,
        active_group_id: None,
      }),
    }
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  pub fn minimize_to_tray_enabled(&self) -> bool {
    self.minimize_to_tray.load(Ordering::SeqCst)
  }

  #[cfg(desktop)]
  pub fn toggle_main_window(app: &AppHandle) {
    let Some(win) = app.get_window("main") else {
      return;
    };
    let visible = win.is_visible().unwrap_or(false);
    let focused = win.is_focused().unwrap_or(false);
    if visible && focused {
      let _ = win.hide();
    } else {
      let _ = win.unminimize();
      let _ = win.show();
      let _ = win.set_focus();
    }
  }

  #[cfg(desktop)]
  async fn apply_global_shortcut_on(app: &AppHandle, shortcut: &str) -> Result<bool, String> {
    let trimmed = shortcut.trim().to_string();
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
      use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

      let gs = app_handle.global_shortcut();
      let _ = gs.unregister_all();
      if trimmed.is_empty() {
        return Ok(false);
      }
      gs.on_shortcut(trimmed.as_str(), move |app_cb, _hotkey, event| {
        if event.state == ShortcutState::Pressed {
          Self::toggle_main_window(app_cb);
        }
      })
      .map_err(|e| format!("快捷键格式无效或已被其他应用占用: {e}"))?;
      Ok(true)
    })
    .await
    .map_err(|e| format!("应用全局快捷键中断: {e}"))?
  }

  fn snapshot_from_locked(locked: &InnerState) -> ManagerSnapshot {
    let (active_account, switching_account) = match &locked.active_slot {
      Some(slot) if slot.committed => (Some(slot.account_key.clone()), None),
      Some(slot) => (None, Some(slot.account_key.clone())),
      None => (None, None),
    };

    #[cfg(desktop)]
    let warm_accounts = locked
      .warm_pool
      .values()
      .filter(|e| e.paint_ready)
      .map(|e| e.account_key.clone())
      .collect::<Vec<_>>();

    #[cfg(not(desktop))]
    let warm_accounts = Vec::new();

    let providers = all_provider_infos();
    let account_errors = locked
      .account_page_errors
      .iter()
      .map(|(k, msg)| AccountPageError {
        provider_id: k.provider_id.clone(),
        uuid: k.uuid.clone(),
        message: msg.clone(),
      })
      .collect::<Vec<_>>();

    ManagerSnapshot {
      platform: current_platform_name(),
      providers,
      accounts: locked.store.accounts.clone(),
      selected_provider_id: locked.selected_provider_id.clone(),
      active_account,
      switching_account,
      warm_accounts,
      last_committed: locked.store.last_committed.clone(),
      pending_cleanup: locked.store.pending_cleanup.clone(),
      conflicts: locked.conflicts.clone(),
      notice: locked.notice.clone(),
      modal_active: locked.modal_active,
      settings: locked.store.settings.clone(),
      global_shortcut_active: locked.global_shortcut_active,
      global_shortcut_error: locked.global_shortcut_error.clone(),
      group_chats: locked.store.group_chats.clone(),
      active_group_id: locked.active_group_id.clone(),
      external_agents: locked.store.external_agents.clone(),
      account_errors,
    }
  }

  fn emit_snapshot(&self, app: &AppHandle, snap: &ManagerSnapshot) {
    let _ = app.emit("manager-state-changed", snap);
  }

  pub async fn get_snapshot(&self) -> ManagerSnapshot {
    let locked = self.inner.lock().await;
    Self::snapshot_from_locked(&locked)
  }

  pub async fn run_startup_tasks(self: &Arc<Self>, app: &AppHandle) {
    #[cfg(desktop)]
    {
      let initial_shortcut = {
        let locked = self.inner.lock().await;
        locked.store.settings.global_shortcut.clone()
      };
      let shortcut_res = Self::apply_global_shortcut_on(app, &initial_shortcut).await;
      let mut locked = self.inner.lock().await;
      match shortcut_res {
        Ok(active) => {
          locked.global_shortcut_active = active;
          locked.global_shortcut_error = None;
        }
        Err(err) => {
          locked.global_shortcut_active = false;
          locked.global_shortcut_error = Some(err);
        }
      }
    }

    #[cfg(mobile)]
    {
      let mgr = Arc::clone(self);
      let app_clone = app.clone();
      let channel = tauri::ipc::Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(raw_json) = body {
          if let Ok(evt) =
            serde_json::from_str::<crate::platform::mobile::MobileSessionEvent>(&raw_json)
          {
            let mgr_inner = Arc::clone(&mgr);
            let app_inner = app_clone.clone();
            tauri::async_runtime::spawn(async move {
              mgr_inner.handle_mobile_event(&app_inner, evt).await;
            });
          }
        }
        Ok(())
      });
      let _ = crate::platform::mobile::subscribe_events(app, channel).await;
    }

    self.retry_pending_cleanups(app).await;

    let mgr_periodic = Arc::clone(self);
    let app_periodic = app.clone();
    tauri::async_runtime::spawn(async move {
      let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5 * 60));
      interval.tick().await;
      loop {
        interval.tick().await;
        mgr_periodic.trigger_silent_sync(&app_periodic, false).await;
      }
    });

    #[cfg(desktop)]
    {
      let mgr_reclaim = Arc::clone(self);
      let app_reclaim = app.clone();
      tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
          interval.tick().await;
          mgr_reclaim.reclaim_idle_desktop_webviews(&app_reclaim).await;
        }
      });
    }

    let candidate = {
      let mut locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() || locked.notice.is_some() {
        None
      } else if let Some(key) = locked.store.last_committed.clone() {
        let in_cleanup = locked.store.pending_cleanup.contains(&key);
        let exists = locked
          .store
          .accounts
          .iter()
          .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid);
        let enabled = resolve_provider(&key.provider_id)
          .is_some_and(|spec| spec.info.enabled);
        if in_cleanup || !exists {
          locked.store.last_committed = None;
          let _ = save_store_atomic(&locked.store_path, &locked.store);
          locked.notice = Some("上次记录的账号键不存在或已清理，已回到管理页".to_string());
          None
        } else if !enabled {
          None
        } else {
          locked.selected_provider_id = key.provider_id.clone();
          Some(key)
        }
      } else {
        None
      }
    };

    let had_startup_candidate = candidate.is_some();
    if let Some(key) = candidate {
      let _ = self.switch_account_internal(app, key, true).await;
    } else {
      let snap = self.get_snapshot().await;
      self.emit_snapshot(app, &snap);
    }

    let mgr_prewarm = Arc::clone(self);
    let app_prewarm = app.clone();
    tauri::async_runtime::spawn(async move {
      tokio::time::sleep(std::time::Duration::from_millis(600)).await;
      mgr_prewarm.prewarm_next_cold_account(&app_prewarm).await;
      if !had_startup_candidate {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        mgr_prewarm.prewarm_next_cold_account(&app_prewarm).await;
      }
    });
  }

  async fn retry_pending_cleanups(&self, app: &AppHandle) {
    let keys = {
      let locked = self.inner.lock().await;
      locked.store.pending_cleanup.clone()
    };
    if keys.is_empty() {
      return;
    }

    for key in keys {
      let deleted = self.delete_profile_for_key(app, &key.uuid).await;
      if deleted.is_ok() {
        let mut locked = self.inner.lock().await;
        locked.store.pending_cleanup.retain(|k| k != &key);
        let _ = save_store_atomic(&locked.store_path, &locked.store);
      }
    }
  }

  async fn delete_profile_for_key(&self, app: &AppHandle, uuid_str: &str) -> Result<(), String> {
    #[cfg(desktop)]
    {
      let app_data_dir = {
        let locked = self.inner.lock().await;
        locked.app_data_dir.clone()
      };
      platform::delete_desktop_profile(app, &app_data_dir, uuid_str).await
    }
    #[cfg(mobile)]
    {
      crate::platform::mobile::delete_profile(app, uuid_str).await
    }
    #[cfg(all(not(desktop), not(mobile)))]
    {
      let _ = (app, uuid_str);
      Ok(())
    }
  }

  #[cfg(desktop)]
  fn close_warm_entry(app: &AppHandle, entry: WarmDesktopEntry) {
    entry.alive_flag.store(false, Ordering::SeqCst);
    entry.probe_notify.notify_one();
    if let Some(popup_label) = entry.popup_label {
      if let Some(popup_win) = app.get_webview_window(&popup_label) {
        let _ = popup_win.close();
      }
    }
    if let Some(wv) = app.get_webview(&entry.webview_label) {
      let _ = wv.hide();
      let _ = wv.close();
    }
  }

  #[cfg(desktop)]
  fn group_needs_auth_members(locked: &InnerState) -> HashSet<AccountKey> {
    locked
      .active_group_id
      .as_ref()
      .and_then(|gid| locked.store.group_chats.iter().find(|g| &g.id == gid))
      .map(|g| {
        g.messages
          .iter()
          .filter(|m| m.status == GroupReplyStatus::NeedsAuth)
          .filter_map(|m| m.sender.clone())
          .collect()
      })
      .unwrap_or_default()
  }

  #[cfg(desktop)]
  fn evict_warm_pool_if_needed(
    app: &AppHandle,
    locked: &mut InnerState,
    protect: Option<&AccountKey>,
    max_len: usize,
  ) {
    let needs_auth_members = Self::group_needs_auth_members(locked);

    while locked.warm_pool.len() > max_len {
      let victim_key = locked
        .warm_pool
        .iter()
        .filter(|(k, _)| {
          Some(*k) != protect
            && locked.active_slot.as_ref().map(|s| &s.account_key) != Some(*k)
            && locked.in_flight_group_members.get(*k).copied().unwrap_or(0) == 0
            && !needs_auth_members.contains(*k)
        })
        .min_by_key(|(_, entry)| (entry.last_used_ms, entry.view_instance_id))
        .map(|(k, _)| k.clone());
      let Some(victim) = victim_key else {
        break;
      };
      if let Some(entry) = locked.warm_pool.remove(&victim) {
        Self::close_warm_entry(app, entry);
      }
    }
  }

  #[cfg(desktop)]
  async fn reclaim_idle_desktop_webviews(&self, app: &AppHandle) {
    let now = now_ms();
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      let needs_auth_members = Self::group_needs_auth_members(&locked);
      let in_group = locked.active_group_id.is_some();

      let mut candidates: Vec<(AccountKey, u64)> = locked
        .warm_pool
        .iter()
        .filter(|(k, entry)| {
          entry.paint_ready
            && entry.last_used_ms > 0
            && now.saturating_sub(entry.last_used_ms) >= GROUP_IDLE_RECLAIM_MS
            && locked.active_slot.as_ref().map(|s| &s.account_key) != Some(*k)
            && locked.in_flight_group_members.get(*k).copied().unwrap_or(0) == 0
            && !needs_auth_members.contains(*k)
        })
        .map(|(k, entry)| (k.clone(), entry.last_used_ms))
        .collect();

      if candidates.is_empty() {
        return;
      }

      if !in_group && locked.warm_pool.len() <= 2 {
        return;
      }

      candidates.sort_by_key(|(_, last_used)| *last_used);
      let keep_recent = if in_group { 0 } else { 2.min(locked.warm_pool.len()) };
      let mut closed_any = false;
      for (victim, _) in candidates {
        if !in_group && locked.warm_pool.len() <= keep_recent {
          break;
        }
        if let Some(entry) = locked.warm_pool.remove(&victim) {
          Self::close_warm_entry(app, entry);
          closed_any = true;
        }
      }

      if closed_any {
        Some(Self::snapshot_from_locked(&locked))
      } else {
        None
      }
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  async fn hide_foreground_slot(&self, app: &AppHandle, next_seq: u64, locked: &mut InnerState) {
    locked.pending_external_auth = None;
    self.active_view_id.store(0, Ordering::SeqCst);
    if let Some(old_slot) = locked.active_slot.take() {
      #[cfg(desktop)]
      {
        let _ = next_seq;
        if let Some(entry) = locked.warm_pool.get_mut(&old_slot.account_key) {
          if let Some(popup_label) = entry.popup_label.take() {
            if let Some(popup_win) = app.get_webview_window(&popup_label) {
              let _ = popup_win.close();
            }
          }
          if let Some(wv) = app.get_webview(&entry.webview_label) {
            let _ = wv.hide();
          }
        }
      }
      #[cfg(mobile)]
      {
        let _ = old_slot;
        let _ = crate::platform::mobile::close_active_session(app, next_seq).await;
      }
    }
    #[cfg(desktop)]
    {
      if let Some(main_win) = app.get_window("main") {
        let _ = main_win.set_title(APP_TITLE);
      }
      Self::apply_desktop_layout_locked(app, locked);
    }
  }

  #[cfg(desktop)]
  fn set_right_webview_input_frozen(webview: &tauri::Webview, frozen: bool) {
    #[cfg(windows)]
    {
      let _ = webview.with_webview(move |pwv| unsafe {
        let mut hwnd = windows::Win32::Foundation::HWND::default();
        if pwv.controller().ParentWindow(&mut hwnd).is_ok() && !hwnd.0.is_null() {
          let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(hwnd, !frozen);
        }
      });
    }

    if frozen {
      let _ = webview.eval(
        r#"(() => {
          try {
            document.documentElement.inert = true;
            document.activeElement?.blur?.();
            if (!document.getElementById('__mgr_freeze_shield__')) {
              const s = document.createElement('div');
              s.id = '__mgr_freeze_shield__';
              s.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:transparent;cursor:not-allowed;';
              for (const ev of ['mousedown','mouseup','click','keydown','keyup','keypress','focusin','wheel','touchstart']) {
                s.addEventListener(ev, (e) => { e.stopImmediatePropagation(); e.preventDefault(); }, true);
              }
              document.documentElement.appendChild(s);
            }
          } catch (_) {}
        })()"#,
      );
    } else {
      let _ = webview.eval(
        r#"(() => {
          try {
            document.documentElement.inert = false;
            document.getElementById('__mgr_freeze_shield__')?.remove();
          } catch (_) {}
        })()"#,
      );
    }
  }

  #[cfg(desktop)]
  fn apply_desktop_layout_locked(app: &AppHandle, locked: &InnerState) {
    let Some(main_win) = app.get_window("main") else {
      return;
    };
    let Ok(phys_size) = main_win.inner_size() else {
      return;
    };
    let Ok(scale) = main_win.scale_factor() else {
      return;
    };
    let (win_w, win_h) = (
      (phys_size.width as f64) / scale,
      (phys_size.height as f64) / scale,
    );
    let _ = (win_w, win_h);

    let left_phys = ((LEFT_PANE_WIDTH * scale).round().max(0.0) as u32).min(phys_size.width);
    let right_phys = phys_size.width.saturating_sub(left_phys);

    let active_committed_label = locked.active_slot.as_ref().and_then(|slot| {
      if slot.committed {
        locked
          .warm_pool
          .get(&slot.account_key)
          .map(|e| e.webview_label.clone())
      } else {
        None
      }
    });

    let active_loading_label = locked.active_slot.as_ref().and_then(|slot| {
      if !slot.committed {
        locked
          .warm_pool
          .get(&slot.account_key)
          .map(|e| e.webview_label.clone())
      } else {
        None
      }
    });

    let stage_loading_behind = cfg!(target_os = "macos")
      && active_committed_label.is_none()
      && active_loading_label.is_some()
      && right_phys > 1;
    let group_work_behind = cfg!(target_os = "macos")
      && active_committed_label.is_none()
      && right_phys > 1;

    if group_work_behind {
      if let Some(mgr_wv) = app.get_webview("manager") {
        let _ = mgr_wv.set_bounds(tauri::Rect {
          position: tauri::PhysicalPosition::new(0, 0).into(),
          size: tauri::PhysicalSize::new(phys_size.width, phys_size.height).into(),
        });
        #[cfg(target_os = "macos")]
        platform::raise_macos_webview_to_front(&mgr_wv);
      }
    }

    for entry in locked.warm_pool.values() {
      let keep_visible = Some(&entry.webview_label) == active_committed_label.as_ref()
        || (stage_loading_behind && Some(&entry.webview_label) == active_loading_label.as_ref());
      let keep_group_behind =
        group_work_behind && locked.in_flight_group_members.contains_key(&entry.account_key);
      if keep_group_behind {
        if let Some(wv) = app.get_webview(&entry.webview_label) {
          let _ = wv.set_bounds(tauri::Rect {
            position: tauri::PhysicalPosition::new(left_phys as i32, 0).into(),
            size: tauri::PhysicalSize::new(right_phys, phys_size.height).into(),
          });
          let _ = wv.show();
        }
      } else if !keep_visible {
        if let Some(wv) = app.get_webview(&entry.webview_label) {
          let _ = wv.hide();
        }
      }
    }

    #[cfg(target_os = "macos")]
    if group_work_behind {
      if let Some(mgr_wv) = app.get_webview("manager") {
        platform::raise_macos_webview_to_front(&mgr_wv);
      }
    }

    if let Some(active_label) = active_committed_label {
      if let Some(mgr_wv) = app.get_webview("manager") {
        let _ = mgr_wv.set_bounds(tauri::Rect {
          position: tauri::PhysicalPosition::new(0, 0).into(),
          size: tauri::PhysicalSize::new(left_phys, phys_size.height).into(),
        });
      }

      if let Some(acc_wv) = app.get_webview(&active_label) {
        if right_phys <= 1 {
          let _ = acc_wv.hide();
        } else {
          let _ = acc_wv.set_bounds(tauri::Rect {
            position: tauri::PhysicalPosition::new(left_phys as i32, 0).into(),
            size: tauri::PhysicalSize::new(right_phys, phys_size.height).into(),
          });
          let _ = acc_wv.show();
        }
        Self::set_right_webview_input_frozen(&acc_wv, locked.modal_active);
        if locked.modal_active {
          if let Some(mgr_wv) = app.get_webview("manager") {
            let _ = mgr_wv.set_focus();
          }
        }
      }
    } else if let Some(mgr_wv) = app.get_webview("manager") {
      let _ = mgr_wv.set_bounds(tauri::Rect {
        position: tauri::PhysicalPosition::new(0, 0).into(),
        size: tauri::PhysicalSize::new(phys_size.width, phys_size.height).into(),
      });

      #[cfg(target_os = "macos")]
      {
        platform::raise_macos_webview_to_front(&mgr_wv);
        if let Some(loading_label) = active_loading_label.filter(|_| right_phys > 1) {
          if let Some(loading_wv) = app.get_webview(&loading_label) {
            let _ = loading_wv.set_bounds(tauri::Rect {
              position: tauri::PhysicalPosition::new(left_phys as i32, 0).into(),
              size: tauri::PhysicalSize::new(right_phys, phys_size.height).into(),
            });
            let _ = loading_wv.show();
            platform::raise_macos_webview_to_front(&mgr_wv);
          }
        }
      }
    }
  }

  #[cfg(desktop)]
  pub async fn recompute_desktop_bounds(&self, app: &AppHandle) {
    let locked = self.inner.lock().await;
    Self::apply_desktop_layout_locked(app, &locked);
  }

  pub async fn select_provider(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;

    #[cfg(desktop)]
    let warm_restore = {
      let mut locked = self.inner.lock().await;
      let spec = resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      locked.active_group_id = None;
      locked.selected_provider_id = provider_id.clone();
      self.hide_foreground_slot(app, next_seq, &mut locked).await;
      if spec.info.enabled && locked.conflicts.is_empty() {
        locked
          .last_active_per_provider
          .get(&provider_id)
          .cloned()
          .filter(|k| {
            locked
              .warm_pool
              .get(k)
              .map(|e| e.paint_ready)
              .unwrap_or(false)
              && locked
                .store
                .accounts
                .iter()
                .any(|a| a.provider_id == k.provider_id && a.uuid == k.uuid)
          })
      } else {
        None
      }
    };

    #[cfg(desktop)]
    if let Some(restore_key) = warm_restore {
      return self.switch_account_internal(app, restore_key, false).await;
    }

    #[cfg(not(desktop))]
    {
      let mut locked = self.inner.lock().await;
      let _ = resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      locked.active_group_id = None;
      locked.selected_provider_id = provider_id;
      self.hide_foreground_slot(app, next_seq, &mut locked).await;
      return Ok(Self::snapshot_from_locked(&locked));
    }

    #[cfg(desktop)]
    Ok(self.get_snapshot().await)
  }

  pub async fn add_account(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_alias: &str,
    raw_proxy_url: Option<&str>,
    raw_timezone: Option<&str>,
    raw_cookies: Option<&str>,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let trimmed = raw_alias.trim();
    let (alias, auto_alias) = if trimmed.is_empty() {
      ("Account not signed in".to_string(), true)
    } else {
      (trimmed.to_string(), false)
    };
    let proxy_url = platform::parse_account_proxy(raw_proxy_url.unwrap_or_default())?
      .map(|url| platform::format_account_proxy(&url));
    let timezone = platform::parse_account_timezone(raw_timezone.unwrap_or_default())?;
    let proxy_raw = proxy_url.clone().unwrap_or_default();

    let cookie_raw_opt = raw_cookies
      .map(|s| s.trim())
      .filter(|s| !s.is_empty());

    let (parsed_cookie_specs, cookie_hint, url_opt, spec_enabled_check) = {
      let spec = resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      if !spec.info.enabled {
        return Err(format!("{} 当前待接入，暂不支持添加内嵌账号", spec.info.name));
      }
      if let Some(raw_ck) = cookie_raw_opt {
        let url = Url::parse(&spec.info.entry_url).map_err(|_| "服务入口地址无效".to_string())?;
        let default_host = url
          .host_str()
          .ok_or_else(|| "服务入口域名无效".to_string())?
          .to_string();
        let specs = platform::parse_imported_cookies(
          raw_ck,
          &default_host,
          &spec.allowed_hosts,
          (now_ms() / 1000) as i64,
        )?;
        let hint = platform::extract_identity_hint_from_cookies(&specs);
        (Some(specs), hint, Some(url), spec.info.name)
      } else {
        (None, None, None, spec.info.name)
      }
    };
    let _ = spec_enabled_check;

    let (created_key, profile_dir) = {
      let mut locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() {
        return Err("请先处理 UUID 归属冲突后再新增账号".to_string());
      }

      let uuid = loop {
        let candidate = Uuid::new_v4().hyphenated().to_string();
        let used_in_accounts = locked.store.accounts.iter().any(|a| a.uuid == candidate);
        let used_in_cleanup = locked.store.pending_cleanup.iter().any(|k| k.uuid == candidate);
        if !used_in_accounts && !used_in_cleanup {
          break candidate;
        }
      };

      let initial_auth = if parsed_cookie_specs.is_some() {
        AuthState::Unknown
      } else {
        AuthState::LoggedOut
      };

      locked.store.accounts.push(AccountRecord {
        provider_id: provider_id.clone(),
        uuid: uuid.clone(),
        alias,
        created_at_ms: now_ms(),
        auto_alias,
        proxy_url,
        timezone,
        avatar_url: None,
        quota_text: None,
        quota_percent: None,
        quota_detail: None,
        auth_state: initial_auth,
        cookie_expires_at_ms: None,
        cookie_session_only: false,
      });
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.accounts.pop();
        return Err(err);
      }
      locked.selected_provider_id = provider_id.clone();
      let dir = locked.app_data_dir.join("profiles").join(&uuid);
      (AccountKey { provider_id, uuid }, dir)
    };

    if let (Some(specs), Some(url)) = (parsed_cookie_specs.as_ref(), url_opt.as_ref()) {
      let write_res = self
        .write_imported_cookies_to_store(app, &created_key, url, &proxy_raw, &profile_dir, specs)
        .await;
      let win_cold_summary = match write_res {
        Ok(summary) => summary,
        Err(err) => {
          let mut locked = self.inner.lock().await;
          locked.cookie_identity_hints.remove(&created_key);
          locked
            .store
            .accounts
            .retain(|a| !(a.provider_id == created_key.provider_id && a.uuid == created_key.uuid));
          let _ = save_store_atomic(&locked.store_path, &locked.store);
          return Err(err);
        }
      };

      if let Some(hint) = cookie_hint {
        let mut locked = self.inner.lock().await;
        locked.cookie_identity_hints.insert(created_key.clone(), hint);
      }

      #[cfg(desktop)]
      if let Some(summary) = win_cold_summary {
        self.apply_cookie_expiry_summary(app, &created_key, summary).await;
      }
      #[cfg(not(desktop))]
      let _ = win_cold_summary;

      let _ = self.switch_account_internal(app, created_key, false).await?;
      let snap = {
        let mut locked = self.inner.lock().await;
        locked.notice = Some("已通过 Cookie 添加一个账号".to_string());
        Self::snapshot_from_locked(&locked)
      };
      self.emit_snapshot(app, &snap);
      return Ok(snap);
    }

    self.switch_account_internal(app, created_key, false).await
  }

  pub async fn test_account_proxy(
    &self,
    raw_provider: &str,
    raw_proxy_url: &str,
  ) -> Result<ProxyTestResult, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let entry_url = {
      let locked = self.inner.lock().await;
      let _ = locked;
      resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?
        .info
        .entry_url
    };
    platform::test_account_proxy(&entry_url, raw_proxy_url).await
  }

  pub async fn update_account_settings(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
    raw_alias: &str,
    raw_proxy_url: Option<&str>,
    raw_timezone: Option<&str>,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let alias = raw_alias.trim();
    if alias.is_empty()
      && raw_timezone.is_none()
      && (raw_proxy_url.is_none() || !platform::SUPPORTS_ACCOUNT_PROXY)
    {
      return Err("账号别名不能为空".to_string());
    }
    let proxy_update = raw_proxy_url.map(platform::parse_account_proxy).transpose()?;
    let timezone_update = raw_timezone.map(platform::parse_account_timezone).transpose()?;

    let mut locked = self.inner.lock().await;
    let spec = resolve_provider(&provider_id)
      .ok_or_else(|| "未找到对应的服务配置".to_string())?;
    let index = locked
      .store
      .accounts
      .iter()
      .position(|a| a.provider_id == provider_id && a.uuid == uuid)
      .ok_or_else(|| "未找到匹配的账号记录".to_string())?;
    let mut updated = locked.store.accounts[index].clone();
    if !alias.is_empty() {
      updated.alias = alias.to_string();
      updated.auto_alias = false;
    }
    if let Some(proxy) = proxy_update {
      updated.proxy_url = proxy.map(|url| platform::format_account_proxy(&url));
    }
    if let Some(tz) = timezone_update {
      updated.timezone = tz;
    }
    let proxy_changed = updated.proxy_url != locked.store.accounts[index].proxy_url;
    let timezone_changed = updated.timezone != locked.store.accounts[index].timezone;
    if (proxy_changed || timezone_changed) && !locked.conflicts.is_empty() {
      return Err("请先处理 UUID 归属冲突后再修改代理或时区".to_string());
    }
    if updated == locked.store.accounts[index] {
      return Ok(Self::snapshot_from_locked(&locked));
    }
    let previous = std::mem::replace(&mut locked.store.accounts[index], updated);
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.accounts[index] = previous;
      return Err(err);
    }

    #[cfg(desktop)]
    if proxy_changed || timezone_changed {
      let key = AccountKey {
        provider_id: provider_id.clone(),
        uuid: uuid.clone(),
      };
      let was_active = locked.active_slot.as_ref().is_some_and(|slot| slot.account_key == key);
      locked.prewarm_failed.remove(&key);
      if let Some(entry) = locked.warm_pool.remove(&key) {
        Self::close_warm_entry(app, entry);
      }
      if was_active {
        let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.hide_foreground_slot(app, next_seq, &mut locked).await;
        locked.notice = None;
        let account = locked.store.accounts[index].clone();
        let app_data_dir = locked.app_data_dir.clone();
        let view_instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
        drop(locked);
        return self
          .open_desktop_session(app, account, app_data_dir, next_seq, view_instance_id, false)
          .await;
      }
    }

    if let Some(slot) = &locked.active_slot {
      if slot.committed && slot.account_key.provider_id == provider_id && slot.account_key.uuid == uuid {
        let title = format!("{} · {}", spec.info.short_name, locked.store.accounts[index].alias);
        #[cfg(desktop)]
        if let Some(main_win) = app.get_window("main") {
          let _ = main_win.set_title(&title);
        }
        #[cfg(mobile)]
        {
          let _ = crate::platform::mobile::update_account_title(app, slot.switch_seq, &title).await;
        }
      }
    }

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn copy_account_cookie(
    &self,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
  ) -> Result<(), String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey {
      provider_id: provider_id.clone(),
      uuid,
    };
    let locked = self.inner.lock().await;
    if !locked.conflicts.is_empty() {
      return Err("存在 UUID 归属冲突，请先完成归属裁决".to_string());
    }
    if locked.store.pending_cleanup.contains(&key)
      || !locked.store.accounts.iter().any(|account| {
        account.provider_id == key.provider_id && account.uuid == key.uuid
      })
    {
      return Err("账号归属核对失败，未找到可用的账号".to_string());
    }
    let spec = resolve_provider(&provider_id)
      .ok_or_else(|| "未找到对应的服务配置".to_string())?;
    if !spec.info.enabled {
      return Err("该服务尚未开放应用内嵌".to_string());
    }
    let url = Url::parse(&spec.info.entry_url).map_err(|_| "服务入口地址无效".to_string())?;

    #[cfg(target_os = "macos")]
    {
      drop(locked);
      let uuid = Uuid::parse_str(&key.uuid).map_err(|_| "账号资料标识无效".to_string())?;
      let (header, summary) = platform::read_macos_cookies_for_url(app, uuid, url).await?;
      self.apply_cookie_expiry_summary(app, &key, summary).await;
      platform::write_cookie_clipboard(app, header).await
    }
    #[cfg(windows)]
    {
      let res = self.read_windows_cookies_for_url(app, &locked, &key, url).await;
      drop(locked);
      let (header, summary) = res?;
      self.apply_cookie_expiry_summary(app, &key, summary).await;
      platform::write_cookie_clipboard(app, header).await
    }
    #[cfg(mobile)]
    {
      drop(locked);
      platform::mobile::copy_profile_cookie(app, &key.uuid, url.as_str()).await
    }
    #[cfg(not(any(target_os = "macos", windows, mobile)))]
    {
      drop(locked);
      let _ = (app, url);
      Err("当前平台不支持复制 Cookie".to_string())
    }
  }

  #[cfg(windows)]
  async fn read_windows_webview_cookies(
    webview: tauri::Webview,
    url: Url,
  ) -> Result<(String, platform::CookieExpirySummary), String> {
    tauri::async_runtime::spawn_blocking(move || {
      let current_ms = now_ms();
      let cookies = webview
        .cookies_for_url(url)
        .map_err(|_| "读取账号 Cookie 失败".to_string())?;
      let mut pairs = Vec::with_capacity(cookies.len());
      let mut metas = Vec::with_capacity(cookies.len());
      for cookie in &cookies {
        let (session_only, expires_at_ms) = match cookie.expires_datetime() {
          None => (true, None),
          Some(dt) => match platform::unix_secs_i64_to_ms(dt.unix_timestamp()) {
            Some(exp) if exp > current_ms => (false, Some(exp)),
            _ => continue,
          },
        };
        let name = cookie.name().to_string();
        metas.push(platform::CookieMeta {
          name: name.clone(),
          http_only: cookie.http_only().unwrap_or(false),
          session_only,
          expires_at_ms,
        });
        pairs.push(format!("{}={}", cookie.name(), cookie.value()));
      }
      let header = pairs.join("; ");
      let summary = platform::summarize_cookie_expiry(&metas, current_ms);
      Ok((header, summary))
    })
    .await
    .map_err(|_| "读取账号 Cookie 中断".to_string())?
  }

  #[cfg(windows)]
  async fn read_windows_cookies_for_url(
    &self,
    app: &AppHandle,
    locked: &InnerState,
    key: &AccountKey,
    url: Url,
  ) -> Result<(String, platform::CookieExpirySummary), String> {
    let profile_dir = locked.app_data_dir.join("profiles").join(&key.uuid);
    let uuid = Uuid::parse_str(&key.uuid).map_err(|_| "账号资料标识无效".to_string())?;
    let existing = locked.warm_pool.get(key).and_then(|entry| app.get_webview(&entry.webview_label));
    let temporary = existing.is_none();
    let webview = match existing {
      Some(webview) => webview,
      None => {
        if !profile_dir.is_dir() {
          return Err("该账号暂无可复制的 Cookie，请先登录".to_string());
        }
        let account = locked.store.accounts.iter().find(|account| {
          account.provider_id == key.provider_id && account.uuid == key.uuid
        }).ok_or_else(|| "未找到可用的账号".to_string())?;
        let proxy = platform::parse_account_proxy(account.proxy_url.as_deref().unwrap_or_default())?;
        let window = app.get_window("main").ok_or_else(|| "主窗口不存在".to_string())?;
        let instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
        let builder = tauri::webview::WebviewBuilder::new(
          format!("cookie-{instance_id}"),
          tauri::WebviewUrl::External("about:blank".parse().expect("valid blank url")),
        )
        .data_directory(profile_dir.clone())
        .on_navigation(|url| url.as_str() == "about:blank")
        .focused(false);
        let builder =
          platform::with_windows_account_proxy(builder, proxy.as_ref(), &app.config().identifier)?;
        let (position, size) = Self::initial_hidden_child_rect(&window);
        let webview = window.add_child(builder, position, size)
          .map_err(|_| "无法打开该账号的会话资料，请重试".to_string())?;
        let _ = webview.hide();
        webview
      }
    };

    let result = async {
      platform::verify_desktop_profile_binding(&webview, uuid, &profile_dir).await?;
      Self::read_windows_webview_cookies(webview.clone(), url).await
    }.await;
    if temporary {
      let _ = webview.close();
    }
    result
  }

  async fn write_imported_cookies_to_store(
    &self,
    app: &AppHandle,
    key: &AccountKey,
    url: &Url,
    proxy_raw: &str,
    profile_dir: &Path,
    specs: &[platform::ImportedCookieSpec],
  ) -> Result<Option<platform::CookieExpirySummary>, String> {
    #[cfg(desktop)]
    {
      let _ = url;
      let parsed_uuid = Uuid::parse_str(&key.uuid).map_err(|_| "账号资料标识无效".to_string())?;
      let tauri_cookies = specs
        .iter()
        .map(platform::imported_cookie_to_tauri)
        .collect::<Result<Vec<_>, _>>()?;

      let existing_wv = {
        let locked = self.inner.lock().await;
        locked
          .warm_pool
          .get(key)
          .and_then(|entry| app.get_webview(&entry.webview_label))
      };

      let (wv, created_temp) = match existing_wv {
        Some(wv) => (wv, false),
        None => {
          let proxy = platform::parse_account_proxy(proxy_raw)?;
          let window = app.get_window("main").ok_or_else(|| "主窗口不存在".to_string())?;
          let instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
          #[allow(unused_mut)]
          let mut builder = tauri::webview::WebviewBuilder::new(
            format!("cookie-import-{instance_id}"),
            tauri::WebviewUrl::External("about:blank".parse().expect("valid blank url")),
          )
          .on_navigation(|nav_url| nav_url.as_str() == "about:blank")
          .focused(false);
          #[cfg(target_os = "macos")]
          {
            let _ = &proxy;
            builder = builder
              .data_store_identifier(*parsed_uuid.as_bytes())
              .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
          }
          #[cfg(not(target_os = "macos"))]
          {
            builder = builder.data_directory(profile_dir.to_path_buf());
          }
          #[cfg(windows)]
          {
            builder = platform::with_windows_account_proxy(
              builder,
              proxy.as_ref(),
              &app.config().identifier,
            )?;
          }
          let (position, size) = Self::initial_hidden_child_rect(&window);
          let temp_wv = window
            .add_child(builder, position, size)
            .map_err(|_| "无法创建 Cookie 导入视图，请重试".to_string())?;
          let _ = temp_wv.hide();
          (temp_wv, true)
        }
      };

      let apply_res: Result<Option<platform::CookieExpirySummary>, String> = async {
        platform::verify_desktop_profile_binding(&wv, parsed_uuid, profile_dir).await?;
        for cookie in tauri_cookies {
          wv.set_cookie(cookie)
            .map_err(|err| format!("写入 Cookie 失败: {err}"))?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        #[cfg(windows)]
        {
          if created_temp {
            if let Ok((_, summary)) = Self::read_windows_webview_cookies(wv.clone(), url.clone()).await
            {
              return Ok(Some(summary));
            }
          }
        }
        Ok(None)
      }
      .await;

      if created_temp {
        let _ = wv.close();
      }
      apply_res
    }

    #[cfg(mobile)]
    {
      let _ = (self, proxy_raw, profile_dir);
      let headers = specs
        .iter()
        .map(platform::imported_cookie_to_set_cookie_header)
        .collect::<Vec<_>>();
      platform::mobile::import_profile_cookies(app, &key.uuid, url.as_str(), headers).await?;
      Ok(None)
    }

    #[cfg(not(any(desktop, mobile)))]
    {
      let _ = (self, app, key, url, proxy_raw, profile_dir, specs);
      Err("当前平台不支持导入 Cookie".to_string())
    }
  }

  pub async fn import_account_cookies(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
    raw_cookies: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey {
      provider_id: provider_id.clone(),
      uuid: uuid.clone(),
    };

    let (spec, url, default_host, proxy_raw, profile_dir) = {
      let locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() {
        return Err("存在 UUID 归属冲突，请先完成归属裁决".to_string());
      }
      if locked.store.pending_cleanup.contains(&key) {
        return Err("该账号正在清理缓存，请稍后再试".to_string());
      }
      let account = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        .ok_or_else(|| "未找到对应的账号记录".to_string())?;
      let spec = resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      if !spec.info.enabled {
        return Err("该服务尚未开放应用内嵌".to_string());
      }
      let url = Url::parse(&spec.info.entry_url).map_err(|_| "服务入口地址无效".to_string())?;
      let default_host = url
        .host_str()
        .ok_or_else(|| "服务入口域名无效".to_string())?
        .to_string();
      (
        spec,
        url,
        default_host,
        account.proxy_url.clone().unwrap_or_default(),
        locked.app_data_dir.join("profiles").join(&key.uuid),
      )
    };

    let specs = platform::parse_imported_cookies(
      raw_cookies,
      &default_host,
      &spec.allowed_hosts,
      (now_ms() / 1000) as i64,
    )?;
    let cookie_hint = platform::extract_identity_hint_from_cookies(&specs);

    let win_cold_summary = self
      .write_imported_cookies_to_store(app, &key, &url, &proxy_raw, &profile_dir, &specs)
      .await?;

    #[cfg(desktop)]
    {
      let (was_active, was_warm) = {
        let mut locked = self.inner.lock().await;
        locked.account_page_errors.remove(&key);
        locked.prewarm_failed.remove(&key);
        locked.cookie_checked_at.remove(&key);
        if let Some(hint) = cookie_hint {
          locked.cookie_identity_hints.insert(key.clone(), hint);
        }
        if let Some(target) = locked
          .store
          .accounts
          .iter_mut()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        {
          target.auth_state = AuthState::Unknown;
          target.cookie_expires_at_ms = None;
          target.cookie_session_only = false;
        }
        let _ = save_store_atomic(&locked.store_path, &locked.store);
        let active = locked
          .active_slot
          .as_ref()
          .is_some_and(|s| s.account_key == key);
        let warm = locked.warm_pool.contains_key(&key);
        (active, warm)
      };

      if was_active || was_warm {
        let _ = self.reload_account(app, raw_provider, raw_uuid).await?;
        let snap = {
          let mut locked = self.inner.lock().await;
          locked.notice = Some("已通过 Cookie 更新账号登录态并刷新页面".to_string());
          Self::snapshot_from_locked(&locked)
        };
        self.emit_snapshot(app, &snap);
        return Ok(snap);
      }

      if let Some(summary) = win_cold_summary {
        self.apply_cookie_expiry_summary(app, &key, summary).await;
      } else {
        self.refresh_account_cookie_expiry(app, key.clone(), true).await;
      }

      let snap = {
        let mut locked = self.inner.lock().await;
        locked.notice = Some("已通过 Cookie 更新账号登录态，点击账号即可加载".to_string());
        Self::snapshot_from_locked(&locked)
      };
      self.emit_snapshot(app, &snap);
      Ok(snap)
    }

    #[cfg(mobile)]
    {
      let _ = win_cold_summary;
      let snap = {
        let mut locked = self.inner.lock().await;
        locked.account_page_errors.remove(&key);
        if let Some(hint) = cookie_hint {
          locked.cookie_identity_hints.insert(key.clone(), hint);
        }
        if let Some(target) = locked
          .store
          .accounts
          .iter_mut()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        {
          target.auth_state = AuthState::Unknown;
        }
        let _ = save_store_atomic(&locked.store_path, &locked.store);
        locked.notice = Some("已通过 Cookie 更新账号登录态，点击账号即可进入".to_string());
        Self::snapshot_from_locked(&locked)
      };
      self.emit_snapshot(app, &snap);
      Ok(snap)
    }

    #[cfg(not(any(desktop, mobile)))]
    {
      let _ = (win_cold_summary, cookie_hint);
      Err("当前平台不支持导入 Cookie".to_string())
    }
  }

  pub async fn remove_account(
    &self,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey {
      provider_id: provider_id.clone(),
      uuid: uuid.clone(),
    };

    let closed_desktop_view = {
      let mut locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() {
        return Err("存在 UUID 归属冲突，请先完成归属裁决".to_string());
      }
      let idx = locked
        .store
        .accounts
        .iter()
        .position(|a| a.provider_id == provider_id && a.uuid == uuid)
        .ok_or_else(|| "账号归属核对失败，未找到该账号".to_string())?;

      locked.store.accounts.remove(idx);
      for group in &mut locked.store.group_chats {
        group
          .members
          .retain(|m| !(m.provider_id == provider_id && m.uuid == uuid));
        group
          .member_roles
          .retain(|mr| !(mr.member.provider_id == provider_id && mr.member.uuid == uuid));
        group
          .thread_bindings
          .retain(|tb| !(tb.member.provider_id == provider_id && tb.member.uuid == uuid));
      }
      #[cfg(desktop)]
      locked
        .group_context_synced
        .retain(|(_, k), _| !(k.provider_id == provider_id && k.uuid == uuid));
      if locked.store.last_committed.as_ref() == Some(&key) {
        locked.store.last_committed = None;
      }
      if locked.last_active_per_provider.get(&provider_id) == Some(&key) {
        locked.last_active_per_provider.remove(&provider_id);
      }
      locked.account_page_errors.remove(&key);
      locked.cookie_identity_hints.remove(&key);
      if !locked.store.pending_cleanup.contains(&key) {
        locked.store.pending_cleanup.push(key.clone());
      }
      save_store_atomic(&locked.store_path, &locked.store)?;

      let active_matches = locked
        .active_slot
        .as_ref()
        .map(|s| s.account_key == key)
        .unwrap_or(false);
      if active_matches {
        let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.hide_foreground_slot(app, next_seq, &mut locked).await;
      }

      #[cfg(desktop)]
      {
        locked.prewarm_failed.remove(&key);
        locked.cookie_checked_at.remove(&key);
        if let Some(warm_entry) = locked.warm_pool.remove(&key) {
          Self::close_warm_entry(app, warm_entry);
          Self::apply_desktop_layout_locked(app, &locked);
          true
        } else {
          false
        }
      }
      #[cfg(not(desktop))]
      {
        false
      }
    };

    if closed_desktop_view {
      tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }

    if self.delete_profile_for_key(app, &uuid).await.is_ok() {
      let mut locked = self.inner.lock().await;
      locked.store.pending_cleanup.retain(|k| k != &key);
      let _ = save_store_atomic(&locked.store_path, &locked.store);
    }

    Ok(self.get_snapshot().await)
  }

  pub async fn reorder_accounts(
    &self,
    raw_provider: &str,
    ordered_uuids: Vec<String>,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let mut locked = self.inner.lock().await;
    if !locked.conflicts.is_empty() {
      return Err("存在 UUID 归属冲突，请先处理冲突后再调整顺序".to_string());
    }

    let slot_indices: Vec<usize> = locked
      .store
      .accounts
      .iter()
      .enumerate()
      .filter_map(|(idx, acc)| (acc.provider_id == provider_id).then_some(idx))
      .collect();

    if ordered_uuids.len() != slot_indices.len() {
      return Err("账号数量变化，请刷新后重试".to_string());
    }

    let existing_by_uuid: HashMap<String, usize> = slot_indices
      .iter()
      .map(|&idx| (locked.store.accounts[idx].uuid.clone(), idx))
      .collect();

    let mut seen = HashSet::with_capacity(ordered_uuids.len());
    let mut reordered = Vec::with_capacity(ordered_uuids.len());
    for raw_uuid in ordered_uuids {
      let uuid = normalize_uuid(&raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
      if !seen.insert(uuid.clone()) {
        return Err("存在重复的账号排序项".to_string());
      }
      let &src_idx = existing_by_uuid
        .get(&uuid)
        .ok_or_else(|| "账号列表已变更，请刷新后重试".to_string())?;
      reordered.push(locked.store.accounts[src_idx].clone());
    }

    let unchanged = slot_indices
      .iter()
      .zip(reordered.iter())
      .all(|(&idx, record)| locked.store.accounts[idx].uuid == record.uuid);
    if unchanged {
      return Ok(Self::snapshot_from_locked(&locked));
    }

    let previous = locked.store.accounts.clone();
    for (slot_idx, record) in slot_indices.into_iter().zip(reordered.into_iter()) {
      locked.store.accounts[slot_idx] = record;
    }
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.accounts = previous;
      return Err(err);
    }

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn reload_account(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey {
      provider_id: provider_id.clone(),
      uuid: uuid.clone(),
    };

    #[cfg(desktop)]
    {
      let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
      let action = {
        let mut locked = self.inner.lock().await;
        let Some(acc) = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == provider_id && a.uuid == uuid)
          .cloned()
        else {
          return Err("未找到对应的账号记录".to_string());
        };
        let spec = resolve_provider(&provider_id)
          .ok_or_else(|| "未找到对应的服务配置".to_string())?;
        if !spec.info.enabled {
          return Err("该服务尚未开放应用内嵌".to_string());
        }
        let was_active = locked
          .active_slot
          .as_ref()
          .is_some_and(|s| s.account_key == key);
        locked.account_page_errors.remove(&key);
        locked.prewarm_failed.remove(&key);
        if let Some(entry) = locked.warm_pool.remove(&key) {
          Self::close_warm_entry(app, entry);
        }
        if was_active {
          self.hide_foreground_slot(app, next_seq, &mut locked).await;
          let app_data_dir = locked.app_data_dir.clone();
          let vid = self.view_counter.fetch_add(1, Ordering::SeqCst);
          Some((acc, app_data_dir, vid))
        } else {
          Self::apply_desktop_layout_locked(app, &locked);
          None
        }
      };

      if let Some((account, app_data_dir, vid)) = action {
        return self
          .open_desktop_session(app, account, app_data_dir, next_seq, vid, false)
          .await;
      }
      return Ok(self.get_snapshot().await);
    }

    #[cfg(not(desktop))]
    {
      let _ = app;
      let locked = self.inner.lock().await;
      if !locked
        .store
        .accounts
        .iter()
        .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
      {
        return Err("未找到对应的账号记录".to_string());
      }
      Ok(Self::snapshot_from_locked(&locked))
    }
  }

  pub async fn clear_account_data(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey {
      provider_id: provider_id.clone(),
      uuid: uuid.clone(),
    };

    let (was_active, closed_desktop_view) = {
      let mut locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() {
        return Err("存在 UUID 归属冲突，请先完成归属裁决".to_string());
      }
      let idx = locked
        .store
        .accounts
        .iter()
        .position(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        .ok_or_else(|| "未找到对应的账号记录".to_string())?;

      let active_matches = locked
        .active_slot
        .as_ref()
        .is_some_and(|s| s.account_key == key);
      if active_matches {
        let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.hide_foreground_slot(app, next_seq, &mut locked).await;
      }

      locked.store.accounts[idx].auth_state = AuthState::LoggedOut;
      locked.store.accounts[idx].quota_detail = None;
      locked.store.accounts[idx].quota_text = None;
      locked.store.accounts[idx].quota_percent = None;
      locked.store.accounts[idx].avatar_url = None;
      locked.store.accounts[idx].cookie_expires_at_ms = None;
      locked.store.accounts[idx].cookie_session_only = false;
      locked.account_page_errors.remove(&key);
      locked.cookie_identity_hints.remove(&key);
      if !locked.store.pending_cleanup.contains(&key) {
        locked.store.pending_cleanup.push(key.clone());
      }
      save_store_atomic(&locked.store_path, &locked.store)?;

      #[cfg(desktop)]
      let closed = {
        locked.prewarm_failed.remove(&key);
        locked.cookie_checked_at.remove(&key);
        locked.group_context_synced.retain(|(_, k), _| k != &key);
        if let Some(entry) = locked.warm_pool.remove(&key) {
          Self::close_warm_entry(app, entry);
          Self::apply_desktop_layout_locked(app, &locked);
          true
        } else {
          false
        }
      };
      #[cfg(not(desktop))]
      let closed = false;

      (active_matches, closed)
    };

    if closed_desktop_view {
      tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    match self.delete_profile_for_key(app, &uuid).await {
      Ok(()) => {
        let snap = {
          let mut locked = self.inner.lock().await;
          locked.store.pending_cleanup.retain(|k| k != &key);
          save_store_atomic(&locked.store_path, &locked.store)?;
          Self::snapshot_from_locked(&locked)
        };
        if was_active {
          return self.switch_account_internal(app, key, false).await;
        }
        Ok(snap)
      }
      Err(err) => {
        let snap = self.get_snapshot().await;
        self.emit_snapshot(app, &snap);
        Err(err)
      }
    }
  }

  pub async fn update_app_settings(
    &self,
    app: &AppHandle,
    minimize_to_tray: bool,
    raw_shortcut: &str,
    auto_approve: Option<bool>,
  ) -> Result<ManagerSnapshot, String> {
    let shortcut = raw_shortcut.trim().to_string();
    if shortcut.chars().count() > 80 {
      return Err("全局快捷键长度过长".to_string());
    }

    #[cfg(desktop)]
    let (global_shortcut_active, global_shortcut_error) = {
      let (old_shortcut, old_active) = {
        let locked = self.inner.lock().await;
        (
          locked.store.settings.global_shortcut.clone(),
          locked.global_shortcut_active,
        )
      };
      match Self::apply_global_shortcut_on(app, &shortcut).await {
        Ok(active) => (active, None),
        Err(err) => {
          if old_active && !old_shortcut.trim().is_empty() {
            let _ = Self::apply_global_shortcut_on(app, &old_shortcut).await;
          }
          return Err(err);
        }
      }
    };

    #[cfg(not(desktop))]
    let (global_shortcut_active, global_shortcut_error) = {
      let _ = app;
      (false, None)
    };

    let next_auto_approve = auto_approve.unwrap_or_else(|| self.auto_approve.load(Ordering::SeqCst));

    let (snap, warm_labels) = {
      let mut locked = self.inner.lock().await;
      let previous = locked.store.settings.clone();
      locked.store.settings.minimize_to_tray = minimize_to_tray;
      locked.store.settings.global_shortcut = shortcut;
      locked.store.settings.auto_approve = next_auto_approve;
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.settings = previous;
        return Err(err);
      }
      self.minimize_to_tray.store(minimize_to_tray, Ordering::SeqCst);
      self.auto_approve.store(next_auto_approve, Ordering::SeqCst);
      locked.global_shortcut_active = global_shortcut_active;
      locked.global_shortcut_error = global_shortcut_error;
      #[cfg(desktop)]
      let labels = locked
        .warm_pool
        .values()
        .map(|e| e.webview_label.clone())
        .collect::<Vec<_>>();
      #[cfg(not(desktop))]
      let labels: Vec<String> = Vec::new();
      (Self::snapshot_from_locked(&locked), labels)
    };

    #[cfg(desktop)]
    {
      let sync_js = format!(
        "(() => {{ try {{ window.__maamAutoApprove = {}; if (window.__maamAutoApprove && typeof window.__maamScanAutoApprove === 'function') window.__maamScanAutoApprove(); }} catch (_) {{}} }})()",
        if next_auto_approve { "true" } else { "false" }
      );
      for label in warm_labels {
        if let Some(wv) = app.get_webview(&label) {
          let _ = wv.eval(&sync_js);
        }
      }
    }
    #[cfg(not(desktop))]
    let _ = warm_labels;

    Ok(snap)
  }

  pub async fn reveal_downloaded_file(&self, download_id: u64) -> Result<(), String> {
    #[cfg(desktop)]
    {
      let path = self
        .recent_downloads
        .lock()
        .unwrap()
        .get(&download_id)
        .cloned()
        .ok_or_else(|| "未找到对应的下载文件记录".to_string())?;
      platform::reveal_path_in_file_manager(&path)
    }
    #[cfg(not(desktop))]
    {
      let _ = download_id;
      Err("当前平台不支持打开本地文件夹".to_string())
    }
  }

  pub async fn set_modal_active(
    &self,
    app: &AppHandle,
    active: bool,
  ) -> Result<ManagerSnapshot, String> {
    let mut locked = self.inner.lock().await;
    locked.modal_active = active;
    #[cfg(desktop)]
    Self::apply_desktop_layout_locked(app, &locked);
    #[cfg(not(desktop))]
    let _ = app;
    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn open_official_site(
    &self,
    app: &AppHandle,
    raw_provider: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let homepage = {
      let locked = self.inner.lock().await;
      let _ = locked;
      resolve_provider(&provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?
        .info
        .homepage_url
    };

    #[cfg(desktop)]
    {
      let _ = app;
      platform::open_external_browser(&homepage)?;
    }
    #[cfg(mobile)]
    {
      crate::platform::mobile::open_external_url(app, &homepage).await?;
    }

    Ok(self.get_snapshot().await)
  }

  pub async fn resolve_uuid_conflict(
    &self,
    keep_provider_raw: &str,
    raw_uuid: &str,
  ) -> Result<ManagerSnapshot, String> {
    let keep_provider =
      ProviderId::parse(keep_provider_raw).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;

    let mut locked = self.inner.lock().await;
    let mut kept_first = false;
    for acc in &mut locked.store.accounts {
      if acc.uuid == uuid {
        if acc.provider_id == keep_provider && !kept_first {
          kept_first = true;
        } else {
          acc.uuid = Uuid::new_v4().hyphenated().to_string();
        }
      }
    }
    locked.conflicts = detect_conflicts(&locked.store.accounts);
    if locked.conflicts.is_empty() {
      locked.notice = None;
    }
    save_store_atomic(&locked.store_path, &locked.store)?;
    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn dismiss_notice(&self) -> ManagerSnapshot {
    let mut locked = self.inner.lock().await;
    locked.notice = None;
    Self::snapshot_from_locked(&locked)
  }

  pub async fn switch_account(
    self: &Arc<Self>,
    app: &AppHandle,
    raw_provider: &str,
    raw_uuid: &str,
  ) -> Result<ManagerSnapshot, String> {
    let provider_id = ProviderId::parse(raw_provider).ok_or_else(|| "无效的服务标识".to_string())?;
    let uuid = normalize_uuid(raw_uuid).ok_or_else(|| "无效的账号 UUID".to_string())?;
    let key = AccountKey { provider_id, uuid };
    self.switch_account_internal(app, key, false).await
  }

  pub async fn switch_account_by_index(
    self: &Arc<Self>,
    app: &AppHandle,
    index_1_based: u8,
  ) -> Result<ManagerSnapshot, String> {
    if index_1_based == 0 {
      return Ok(self.get_snapshot().await);
    }
    let (provider_id, target_uuid) = {
      let locked = self.inner.lock().await;
      let pid = locked.selected_provider_id.clone();
      let accounts: Vec<_> = locked
        .store
        .accounts
        .iter()
        .filter(|a| a.provider_id == pid)
        .collect();
      let idx = (index_1_based - 1) as usize;
      if let Some(acc) = accounts.get(idx) {
        (pid, acc.uuid.clone())
      } else {
        return Ok(Self::snapshot_from_locked(&locked));
      }
    };
    self.switch_account(app, provider_id.as_str(), &target_uuid).await
  }

  pub async fn trigger_silent_sync(self: &Arc<Self>, app: &AppHandle, force: bool) {
    let now = now_ms();
    if !force {
      let last = self.last_sync_timestamp.load(Ordering::SeqCst);
      if now > last && now - last < 30_000 {
        return;
      }
    }
    self.last_sync_timestamp.store(now, Ordering::SeqCst);

    #[cfg(not(desktop))]
    {
      let _ = app;
    }

    #[cfg(desktop)]
    {
      let warm_targets = {
        let mut locked = self.inner.lock().await;
        if force {
          locked.prewarm_failed.clear();
        }
        locked
          .warm_pool
          .iter()
          .map(|(k, v)| (k.clone(), v.webview_label.clone(), v.view_instance_id))
          .collect::<Vec<_>>()
      };

      #[cfg(target_os = "macos")]
      let warm_set: HashSet<AccountKey> = warm_targets.iter().map(|(k, _, _)| k.clone()).collect();

      for (key, wv_label, vid) in warm_targets {
        if let Some(wv) = app.get_webview(&wv_label) {
          let _ = wv.eval(r#"(() => {
            try {
              if (typeof window.__maamVerifySession === 'function') {
                window.__maamVerifySession(true);
              }
              if (typeof window.__maamRefreshBackendQuota === 'function') {
                window.__maamRefreshBackendQuota(true);
              }
            } catch (_) {}
          })()"#);

          let mgr = Arc::clone(self);
          let app_handle = app.clone();
          tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(850)).await;
            if let Some(probe) = Self::eval_identity_probe(&wv).await {
              if probe.is_auth {
                let resolved_identity = if !probe.email.is_empty() {
                  Some(probe.email)
                } else if !probe.typed.is_empty() {
                  Some(probe.typed)
                } else {
                  let locked = mgr.inner.lock().await;
                  locked.cookie_identity_hints.get(&key).cloned()
                };
                let soft_identity = if resolved_identity.is_none() && !probe.display_name.is_empty() {
                  Some(probe.display_name)
                } else {
                  None
                };
                let (just_authenticated, _) = mgr
                  .apply_detected_metadata(
                    &app_handle,
                    key.clone(),
                    vid,
                    resolved_identity,
                    soft_identity,
                    if probe.avatar.is_empty() {
                      None
                    } else {
                      Some(probe.avatar)
                    },
                    if probe.quota_text.is_empty() {
                      None
                    } else {
                      Some(probe.quota_text)
                    },
                    probe.quota_pct,
                    probe.quota_detail,
                  )
                  .await;
                mgr
                  .refresh_account_cookie_expiry(&app_handle, key, force || just_authenticated)
                  .await;
              } else if probe.logged_out {
                mgr.apply_logged_out_state(&app_handle, key.clone(), vid).await;
                if probe.stale_session {
                  if key.provider_id.is_muse() {
                    if let Ok(u) = Url::parse("https://muse.ai/api/logout") {
                      let _ = wv.navigate(u);
                    }
                  } else if key.provider_id.is_hark() {
                    if let Ok(u) = Url::parse("https://app.hark.com/login") {
                      let _ = wv.navigate(u);
                    }
                  }
                }
              } else if !probe.page_error.is_empty() {
                mgr
                  .apply_page_error_state(&app_handle, key, vid, probe.page_error)
                  .await;
              }
            }
          });
        }
      }

      #[cfg(target_os = "macos")]
      {
        let cold_auth_keys: Vec<AccountKey> = {
          let locked = self.inner.lock().await;
          locked
            .store
            .accounts
            .iter()
            .filter(|a| a.auth_state == AuthState::Authenticated)
            .map(|a| AccountKey {
              provider_id: a.provider_id.clone(),
              uuid: a.uuid.clone(),
            })
            .filter(|k| !warm_set.contains(k))
            .collect()
        };
        for key in cold_auth_keys {
          let mgr_cold = Arc::clone(self);
          let app_cold = app.clone();
          tauri::async_runtime::spawn(async move {
            mgr_cold.refresh_account_cookie_expiry(&app_cold, key, force).await;
          });
        }
      }

      let mgr_prewarm = Arc::clone(self);
      let app_prewarm = app.clone();
      tauri::async_runtime::spawn(async move {
        mgr_prewarm.prewarm_next_cold_account(&app_prewarm).await;
      });
    }
  }

  async fn switch_account_internal(
    self: &Arc<Self>,
    app: &AppHandle,
    key: AccountKey,
    is_cold_start: bool,
  ) -> Result<ManagerSnapshot, String> {
    {
      let locked = self.inner.lock().await;
      if locked.active_group_id.is_none() {
        if let Some(slot) = &locked.active_slot {
          if slot.committed && slot.account_key == key && !is_cold_start {
            #[cfg(desktop)]
            return Ok(Self::snapshot_from_locked(&locked));
          }
        }
      }
    }

    let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;

    #[cfg(desktop)]
    {
      enum DesktopSwitchPlan {
        InstantWarm {
          view_instance_id: u64,
          need_identity_watcher: bool,
          probe_notify: Arc<tokio::sync::Notify>,
        },
        #[cfg(target_os = "macos")]
        StageWarmPaint {
          view_instance_id: u64,
          expected_load_gen: u64,
          loaded_url: Url,
          loading_snap: ManagerSnapshot,
        },
        AwaitWarmLoading {
          view_instance_id: u64,
        },
        CreateCold {
          account: AccountRecord,
          app_data_dir: PathBuf,
          view_instance_id: u64,
        },
        Abort(ManagerSnapshot),
      }

      let plan = {
        let mut locked = self.inner.lock().await;
        if !locked.conflicts.is_empty() {
          return Err("存在 UUID 归属冲突，已停留在管理页".to_string());
        }
        let spec = resolve_provider(&key.provider_id)
          .ok_or_else(|| "未找到对应的服务配置".to_string())?;
        if !spec.info.enabled {
          return Err(format!("{} 尚未开放应用内嵌", spec.info.name));
        }
        if locked.store.pending_cleanup.contains(&key) {
          self.hide_foreground_slot(app, next_seq, &mut locked).await;
          locked.notice = Some("该账号正在清理本地资料，请稍后重试或重新创建".to_string());
          DesktopSwitchPlan::Abort(Self::snapshot_from_locked(&locked))
        } else {
          let Some(acc) = locked
            .store
            .accounts
            .iter()
            .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
            .cloned()
          else {
            self.hide_foreground_slot(app, next_seq, &mut locked).await;
            locked.notice = Some("账号归属核对失败，已回到管理页".to_string());
            return Ok(Self::snapshot_from_locked(&locked));
          };

          locked.active_group_id = None;
          locked.selected_provider_id = key.provider_id.clone();
          locked.notice = None;
          locked.prewarm_failed.remove(&key);
          self.hide_foreground_slot(app, next_seq, &mut locked).await;

          let warm_state = locked.warm_pool.get(&key).and_then(|entry| {
            if app.get_webview(&entry.webview_label).is_some() {
              Some((
                entry.view_instance_id,
                entry.paint_ready,
                entry.staged_painted,
                entry.watcher_running,
                Arc::clone(&entry.probe_notify),
                Arc::clone(&entry.load_gen),
                entry.last_url.clone(),
              ))
            } else {
              None
            }
          });

          if let Some((
            vid,
            paint_ready,
            staged_painted,
            watcher_running,
            probe_notify,
            load_gen,
            last_url,
          )) = warm_state
          {
            #[cfg(not(target_os = "macos"))]
            let _ = (staged_painted, &load_gen, &last_url);
            #[cfg(target_os = "macos")]
            let needs_stage_paint = paint_ready && !staged_painted;
            #[cfg(not(target_os = "macos"))]
            let needs_stage_paint = false;

            if needs_stage_paint {
              #[cfg(target_os = "macos")]
              {
                if let Some(entry) = locked.warm_pool.get_mut(&key) {
                  entry.last_used_ms = now_ms();
                }
                locked.active_slot = Some(ActiveSessionSlot {
                  account_key: key.clone(),
                  view_instance_id: vid,
                  switch_seq: next_seq,
                  committed: false,
                });
                self.active_view_id.store(vid, Ordering::SeqCst);
                Self::apply_desktop_layout_locked(app, &locked);
                let fallback_url = Url::parse(&spec.info.entry_url)
                  .unwrap_or_else(|_| Url::parse("about:blank").expect("valid blank url"));
                DesktopSwitchPlan::StageWarmPaint {
                  view_instance_id: vid,
                  expected_load_gen: load_gen.load(Ordering::SeqCst),
                  loaded_url: last_url.unwrap_or(fallback_url),
                  loading_snap: Self::snapshot_from_locked(&locked),
                }
              }
              #[cfg(not(target_os = "macos"))]
              unreachable!()
            } else if paint_ready {
              let prev_last = locked.store.last_committed.clone();
              locked.store.last_committed = Some(key.clone());
              if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
                locked.store.last_committed = prev_last;
                locked.notice = Some(format!("提交账号记录写盘失败: {err}"));
                DesktopSwitchPlan::Abort(Self::snapshot_from_locked(&locked))
              } else {
                locked
                  .last_active_per_provider
                  .insert(key.provider_id.clone(), key.clone());
                locked.active_slot = Some(ActiveSessionSlot {
                  account_key: key.clone(),
                  view_instance_id: vid,
                  switch_seq: next_seq,
                  committed: true,
                });
                self.active_view_id.store(vid, Ordering::SeqCst);
                let title = format!("{} · {}", spec.info.short_name, acc.alias);
                if let Some(main_win) = app.get_window("main") {
                  let _ = main_win.set_title(&title);
                }
                let need_watcher = !watcher_running;
                if let Some(entry) = locked.warm_pool.get_mut(&key) {
                  entry.last_used_ms = now_ms();
                  if need_watcher {
                    entry.watcher_running = true;
                  }
                }
                Self::apply_desktop_layout_locked(app, &locked);
                DesktopSwitchPlan::InstantWarm {
                  view_instance_id: vid,
                  need_identity_watcher: need_watcher,
                  probe_notify,
                }
              }
            } else {
              if let Some(entry) = locked.warm_pool.get_mut(&key) {
                entry.last_used_ms = now_ms();
              }
              locked.active_slot = Some(ActiveSessionSlot {
                account_key: key.clone(),
                view_instance_id: vid,
                switch_seq: next_seq,
                committed: false,
              });
              self.active_view_id.store(vid, Ordering::SeqCst);
              Self::apply_desktop_layout_locked(app, &locked);
              DesktopSwitchPlan::AwaitWarmLoading {
                view_instance_id: vid,
              }
            }
          } else {
            if let Some(entry) = locked.warm_pool.remove(&key) {
              Self::close_warm_entry(app, entry);
            }
            let vid = self.view_counter.fetch_add(1, Ordering::SeqCst);
            let dir = locked.app_data_dir.clone();
            DesktopSwitchPlan::CreateCold {
              account: acc,
              app_data_dir: dir,
              view_instance_id: vid,
            }
          }
        }
      };

      match plan {
        DesktopSwitchPlan::Abort(snap) => Ok(snap),
        DesktopSwitchPlan::InstantWarm {
          view_instance_id,
          need_identity_watcher,
          probe_notify,
        } => {
          if need_identity_watcher {
            self.spawn_desktop_identity_watcher(app.clone(), key, view_instance_id, probe_notify);
          } else {
            probe_notify.notify_one();
          }
          Ok(self.get_snapshot().await)
        }
        #[cfg(target_os = "macos")]
        DesktopSwitchPlan::StageWarmPaint {
          view_instance_id,
          expected_load_gen,
          loaded_url,
          loading_snap,
        } => {
          self.emit_snapshot(app, &loading_snap);
          let mgr_stage = Arc::clone(self);
          let app_stage = app.clone();
          let key_stage = key;
          tauri::async_runtime::spawn(async move {
            mgr_stage
              .wait_paint_and_finalize_desktop(
                &app_stage,
                key_stage,
                view_instance_id,
                loaded_url,
                expected_load_gen,
                true,
              )
              .await;
          });
          Ok(loading_snap)
        }
        DesktopSwitchPlan::AwaitWarmLoading { view_instance_id } => {
          let _ = view_instance_id;
          Ok(self.get_snapshot().await)
        }
        DesktopSwitchPlan::CreateCold {
          account,
          app_data_dir,
          view_instance_id,
        } => {
          self
            .open_desktop_session(
              app,
              account,
              app_data_dir,
              next_seq,
              view_instance_id,
              is_cold_start,
            )
            .await
        }
      }
    }

    #[cfg(mobile)]
    {
      let view_instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
      let account_record = {
        let mut locked = self.inner.lock().await;
        if !locked.conflicts.is_empty() {
          return Err("存在 UUID 归属冲突，已停留在管理页".to_string());
        }
        let spec = resolve_provider(&key.provider_id)
          .ok_or_else(|| "未找到对应的服务配置".to_string())?;
        if !spec.info.enabled {
          return Err(format!("{} 尚未开放应用内嵌", spec.info.name));
        }
        if locked.store.pending_cleanup.contains(&key) {
          self.hide_foreground_slot(app, next_seq, &mut locked).await;
          locked.notice = Some("该账号正在清理本地资料，请稍后重试或重新创建".to_string());
          return Ok(Self::snapshot_from_locked(&locked));
        }
        let Some(acc) = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
          .cloned()
        else {
          self.hide_foreground_slot(app, next_seq, &mut locked).await;
          locked.notice = Some("账号归属核对失败，已回到管理页".to_string());
          return Ok(Self::snapshot_from_locked(&locked));
        };
        platform::parse_account_proxy(acc.proxy_url.as_deref().unwrap_or_default())?;
        locked.active_group_id = None;
        locked.selected_provider_id = key.provider_id;
        locked.notice = None;
        self.hide_foreground_slot(app, next_seq, &mut locked).await;
        acc
      };

      self
        .open_mobile_session(app, account_record, next_seq, view_instance_id, is_cold_start)
        .await
    }
  }

  #[cfg(desktop)]
  fn extract_download_filename(path: Option<&std::path::Path>, url: &Url) -> String {
    if let Some(name) = path
      .and_then(|p| p.file_name())
      .and_then(|s| s.to_str())
      .map(str::trim)
      .filter(|s| !s.is_empty())
    {
      return name.to_string();
    }
    url
      .path_segments()
      .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
      .filter(|s| !s.is_empty())
      .unwrap_or("下载文件")
      .to_string()
  }

  #[cfg(desktop)]
  fn record_recent_download(&self, download_id: u64, path: PathBuf) {
    let mut recent = self.recent_downloads.lock().unwrap();
    recent.insert(download_id, path);
    while recent.len() > 32 {
      if let Some(&oldest_id) = recent.keys().min() {
        recent.remove(&oldest_id);
      } else {
        break;
      }
    }
  }

  #[cfg(desktop)]
  fn handle_desktop_download_event(
    &self,
    app: &AppHandle,
    pending: &std::sync::Mutex<HashMap<String, VecDeque<(u64, PathBuf)>>>,
    event: tauri::webview::DownloadEvent<'_>,
  ) -> bool {
    match event {
      tauri::webview::DownloadEvent::Requested { url, destination } => {
        let download_id = self.download_counter.fetch_add(1, Ordering::SeqCst);
        let filename = Self::extract_download_filename(Some(destination.as_path()), &url);
        pending
          .lock()
          .unwrap()
          .entry(url.as_str().to_string())
          .or_default()
          .push_back((download_id, destination.clone()));
        let _ = app.emit(
          "download-event",
          DownloadNoticeEvent {
            id: download_id,
            filename,
            status: "started",
            error: None,
          },
        );
        true
      }
      tauri::webview::DownloadEvent::Finished { url, path, success } => {
        let entry = {
          let mut map = pending.lock().unwrap();
          let popped = map.get_mut(url.as_str()).and_then(VecDeque::pop_front);
          if map.get(url.as_str()).is_some_and(VecDeque::is_empty) {
            map.remove(url.as_str());
          }
          popped
        };
        let (download_id, fallback_path) = match entry {
          Some((id, dest)) => (id, Some(dest)),
          None => (self.download_counter.fetch_add(1, Ordering::SeqCst), None),
        };
        let final_path = path.or(fallback_path);
        let filename = Self::extract_download_filename(final_path.as_deref(), &url);
        if success {
          if let Some(saved_path) = final_path {
            self.record_recent_download(download_id, saved_path);
          }
          let _ = app.emit(
            "download-event",
            DownloadNoticeEvent {
              id: download_id,
              filename,
              status: "completed",
              error: None,
            },
          );
        } else {
          let _ = app.emit(
            "download-event",
            DownloadNoticeEvent {
              id: download_id,
              filename,
              status: "failed",
              error: Some("下载未完成或已取消".to_string()),
            },
          );
        }
        true
      }
      _ => true,
    }
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  fn build_desktop_account_webview(
    self: &Arc<Self>,
    app: &AppHandle,
    key: AccountKey,
    provider_spec: ProviderSpec,
    parsed_uuid: Uuid,
    profile_dir: &std::path::Path,
    proxy_url: Option<&Url>,
    timezone: Option<&str>,
    webview_label: &str,
    view_instance_id: u64,
    allow_blank: Arc<AtomicBool>,
    alive_flag: Arc<AtomicBool>,
    load_gen: Arc<AtomicU64>,
  ) -> tauri::webview::WebviewBuilder<tauri::Wry> {
    let alive_nav = Arc::clone(&alive_flag);
    let mgr_for_nav = Arc::clone(self);
    let app_for_nav = app.clone();
    let allow_blank_nav = Arc::clone(&allow_blank);
    let spec_for_nav = provider_spec.clone();
    let key_for_nav = key.clone();

    let alive_popup = Arc::clone(&alive_flag);
    let active_vid_popup = Arc::clone(&self.active_view_id);
    let mgr_for_popup = Arc::clone(self);
    let app_for_popup = app.clone();
    let spec_for_popup = provider_spec.clone();
    let key_for_popup = key.clone();
    let popup_counter = Arc::new(AtomicU64::new(1));

    let alive_load = Arc::clone(&alive_flag);
    let load_gen_for_load = Arc::clone(&load_gen);
    let mgr_for_load = Arc::clone(self);
    let app_for_load = app.clone();
    let key_for_load = key;
    let handle_page_load = !cfg!(windows) || proxy_url.is_none();

    let pending_downloads: Arc<std::sync::Mutex<HashMap<String, VecDeque<(u64, PathBuf)>>>> =
      Arc::new(std::sync::Mutex::new(HashMap::new()));
    let mgr_for_dl = Arc::clone(self);
    let app_for_dl = app.clone();
    let pending_for_dl = Arc::clone(&pending_downloads);
    let mgr_for_popup_dl = Arc::clone(self);
    let app_for_popup_dl = app.clone();
    let pending_for_popup_dl = Arc::clone(&pending_downloads);

    #[allow(unused_mut)]
    let mut builder = tauri::webview::WebviewBuilder::new(
      webview_label,
      tauri::WebviewUrl::External("about:blank".parse().expect("valid blank url")),
    )
    .focused(false)
    .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
    .initialization_script(&format!(
      "window.__maamAutoApprove = {};",
      if self.auto_approve.load(Ordering::SeqCst) {
        "true"
      } else {
        "false"
      }
    ))
    .initialization_script(LOGIN_CAPTURE_INIT_JS)
    .initialization_script(BACKGROUND_RAF_INIT_JS)
    .initialization_script(crate::site_adapters::NET_HOOK_INIT_JS);

    let timezone_js = timezone.map(platform::build_timezone_override_init_js);
    #[cfg(not(windows))]
    if let Some(ref tz_script) = timezone_js {
      builder = builder.initialization_script(tz_script);
    }
    let popup_timezone_js = timezone_js;

    let mut builder = builder
    .on_navigation(move |url| {
      if !alive_nav.load(Ordering::SeqCst) {
        return false;
      }
      let can_blank = allow_blank_nav.load(Ordering::SeqCst);
      match classify_top_level_url(&spec_for_nav, url, can_blank) {
        NavigationDecision::AllowBlank => true,
        NavigationDecision::AllowHttps => true,
        NavigationDecision::ExternalGoogleOAuth => {
          let mgr_ext = Arc::clone(&mgr_for_nav);
          let app_ext = app_for_nav.clone();
          let key_ext = key_for_nav.clone();
          tauri::async_runtime::spawn(async move {
            mgr_ext
              .notify_external_auth_policy(&app_ext, key_ext, view_instance_id)
              .await;
          });
          false
        }
        NavigationDecision::Deny => false,
      }
    })
    .on_new_window(move |url, features| {
      if !alive_popup.load(Ordering::SeqCst)
        || active_vid_popup.load(Ordering::SeqCst) != view_instance_id
      {
        return tauri::webview::NewWindowResponse::Deny;
      }
      let decision = classify_top_level_url(&spec_for_popup, &url, true);
      match decision {
        NavigationDecision::ExternalGoogleOAuth => {
          let mgr_ext = Arc::clone(&mgr_for_popup);
          let app_ext = app_for_popup.clone();
          let key_ext = key_for_popup.clone();
          tauri::async_runtime::spawn(async move {
            mgr_ext
              .notify_external_auth_policy(&app_ext, key_ext, view_instance_id)
              .await;
          });
          return tauri::webview::NewWindowResponse::Deny;
        }
        NavigationDecision::Deny => {
          return tauri::webview::NewWindowResponse::Deny;
        }
        NavigationDecision::AllowBlank | NavigationDecision::AllowHttps => {}
      }

      let popup_idx = popup_counter.fetch_add(1, Ordering::SeqCst);
      let popup_label = format!("auth-popup-{view_instance_id}-{popup_idx}");
      let popup_label_for_close = popup_label.clone();
      let popup_label_for_slot = popup_label.clone();

      let alive_popup_nav = Arc::clone(&alive_popup);
      let active_vid_popup_nav = Arc::clone(&active_vid_popup);
      let app_popup_nav = app_for_popup.clone();
      let mgr_popup_nav = Arc::clone(&mgr_for_popup);
      let spec_popup_nav = spec_for_popup.clone();
      let key_popup_nav = key_for_popup.clone();

      let popup_title = url
        .host_str()
        .map(|h| format!("登录认证 · {h}"))
        .unwrap_or_else(|| "登录认证".to_string());

      let popup_mgr_dl = Arc::clone(&mgr_for_popup_dl);
      let popup_app_dl = app_for_popup_dl.clone();
      let popup_pending_dl = Arc::clone(&pending_for_popup_dl);
      let popup_tz_js = popup_timezone_js.clone();

      let popup_builder = tauri::WebviewWindowBuilder::new(
        &app_for_popup,
        &popup_label,
        tauri::WebviewUrl::External("about:blank".parse().expect("valid blank url")),
      )
      .window_features(features)
      .initialization_script(LOGIN_CAPTURE_INIT_JS);
      let popup_builder = if let Some(ref tz_script) = popup_tz_js {
        popup_builder.initialization_script(tz_script)
      } else {
        popup_builder
      };
      let popup_builder = popup_builder
      .title(&popup_title)
      .on_download(move |_wv, event| {
        popup_mgr_dl.handle_desktop_download_event(&popup_app_dl, &popup_pending_dl, event)
      })
      .on_navigation(move |nav_url| {
        if !alive_popup_nav.load(Ordering::SeqCst)
          || active_vid_popup_nav.load(Ordering::SeqCst) != view_instance_id
        {
          if let Some(pw) = app_popup_nav.get_webview_window(&popup_label_for_close) {
            let _ = pw.close();
          }
          return false;
        }
        match classify_top_level_url(&spec_popup_nav, nav_url, true) {
          NavigationDecision::AllowBlank => true,
          NavigationDecision::AllowHttps => {
            if let Some(host) = nav_url.host_str() {
              if let Some(pw) = app_popup_nav.get_webview_window(&popup_label_for_close) {
                let _ = pw.set_title(&format!("登录认证 · {host}"));
              }
            }
            true
          }
          NavigationDecision::ExternalGoogleOAuth => {
            if let Some(pw) = app_popup_nav.get_webview_window(&popup_label_for_close) {
              let _ = pw.close();
            }
            let mgr_ext = Arc::clone(&mgr_popup_nav);
            let app_ext = app_popup_nav.clone();
            let key_ext = key_popup_nav.clone();
            tauri::async_runtime::spawn(async move {
              mgr_ext
                .notify_external_auth_policy(&app_ext, key_ext, view_instance_id)
                .await;
            });
            false
          }
          NavigationDecision::Deny => false,
        }
      });

      match popup_builder.build() {
        Ok(popup_window) => {
          let mgr_slot = Arc::clone(&mgr_for_popup);
          let app_slot = app_for_popup.clone();
          let key_slot = key_for_popup.clone();
          tauri::async_runtime::spawn(async move {
            mgr_slot
              .register_desktop_popup(&app_slot, key_slot, view_instance_id, popup_label_for_slot)
              .await;
          });
          tauri::webview::NewWindowResponse::Create {
            window: popup_window,
          }
        }
        Err(_) => tauri::webview::NewWindowResponse::Deny,
      }
    })
    .on_download(move |_wv, event| {
      mgr_for_dl.handle_desktop_download_event(&app_for_dl, &pending_for_dl, event)
    })
    .on_page_load(move |_wv, payload| {
      if payload.url().as_str() == "about:blank" {
        return;
      }
      if !alive_load.load(Ordering::SeqCst) {
        return;
      }
      match payload.event() {
        tauri::webview::PageLoadEvent::Started => {
          load_gen_for_load.fetch_add(1, Ordering::SeqCst);
        }
        tauri::webview::PageLoadEvent::Finished => {
          if !handle_page_load {
            return;
          }
          let expected_gen = load_gen_for_load.fetch_add(1, Ordering::SeqCst) + 1;
          let loaded_url = payload.url().clone();
          let mgr_commit = Arc::clone(&mgr_for_load);
          let app_commit = app_for_load.clone();
          let key_commit = key_for_load.clone();
          tauri::async_runtime::spawn(async move {
            mgr_commit
              .wait_paint_and_finalize_desktop(
                &app_commit,
                key_commit,
                view_instance_id,
                loaded_url,
                expected_gen,
                false,
              )
              .await;
          });
        }
      }
    });

    #[cfg(target_os = "macos")]
    {
      let _ = profile_dir;
      builder = builder
        .data_store_identifier(*parsed_uuid.as_bytes())
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
    }

    #[cfg(not(target_os = "macos"))]
    {
      let _ = parsed_uuid;
      builder = builder.data_directory(profile_dir.to_path_buf());
    }

    if provider_spec.info.id.is_hark() {
      builder = builder.background_color(tauri::webview::Color(23, 29, 47, 255));
    }

    builder
  }

  #[cfg(desktop)]
  fn initial_hidden_child_rect(
    main_win: &tauri::Window,
  ) -> (tauri::LogicalPosition<f64>, tauri::LogicalSize<f64>) {
    let (win_w, win_h) = match (main_win.inner_size(), main_win.scale_factor()) {
      (Ok(phys), Ok(scale)) if scale > 0.0 => {
        ((phys.width as f64) / scale, (phys.height as f64) / scale)
      }
      _ => (1280.0, 800.0),
    };
    let left_w = win_w.min(LEFT_PANE_WIDTH);
    let right_w = (win_w - left_w).max(600.0);
    (
      tauri::LogicalPosition::new(win_w.max(2000.0) + 1000.0, 0.0),
      tauri::LogicalSize::new(right_w, win_h.max(400.0)),
    )
  }

  #[cfg(desktop)]
  fn discard_desktop_view_locked(
    &self,
    app: &AppHandle,
    locked: &mut InnerState,
    key: &AccountKey,
    view_instance_id: u64,
    notice: String,
  ) -> bool {
    if !Self::is_view_instance_valid(locked, key, view_instance_id) {
      return false;
    }
    if let Some(entry) = locked.warm_pool.remove(key) {
      Self::close_warm_entry(app, entry);
    }
    if locked.active_slot.as_ref().is_some_and(|slot| slot.view_instance_id == view_instance_id) {
      locked.active_slot = None;
      self.active_view_id.store(0, Ordering::SeqCst);
      locked.notice = Some(notice);
      Self::apply_desktop_layout_locked(app, locked);
    }
    true
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  async fn attach_desktop_account_webview(
    self: &Arc<Self>,
    app: &AppHandle,
    key: &AccountKey,
    parsed_uuid: Uuid,
    profile_dir: &std::path::Path,
    webview_label: &str,
    view_instance_id: u64,
    alive_flag: &Arc<AtomicBool>,
    load_gen: &Arc<AtomicU64>,
  ) -> Result<(), String> {
    let main_win = app.get_window("main").ok_or_else(|| "主窗口不存在".to_string())?;
    let (init_pos, init_size) = Self::initial_hidden_child_rect(&main_win);
    #[cfg(windows)]
    let retry_until = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
      let (proxy_url, timezone, provider_spec) = {
        let locked = self.inner.lock().await;
        if !Self::is_view_instance_valid(&locked, key, view_instance_id) {
          return Ok(());
        }
        let spec = resolve_provider(&key.provider_id)
          .ok_or_else(|| "未找到对应的服务配置".to_string())?;
        let account = locked
          .store
          .accounts
          .iter()
          .find(|account| account.provider_id == key.provider_id && account.uuid == key.uuid)
          .ok_or_else(|| "账号不存在，已阻止加载".to_string())?;
        (
          platform::parse_account_proxy(account.proxy_url.as_deref().unwrap_or_default())?,
          account.timezone.clone(),
          spec,
        )
      };
      let builder = self.build_desktop_account_webview(
        app,
        key.clone(),
        provider_spec,
        parsed_uuid,
        profile_dir,
        proxy_url.as_ref(),
        timezone.as_deref(),
        webview_label,
        view_instance_id,
        Arc::new(AtomicBool::new(true)),
        Arc::clone(alive_flag),
        Arc::clone(load_gen),
      );
      #[cfg(windows)]
      let builder =
        platform::with_windows_account_proxy(builder, proxy_url.as_ref(), &app.config().identifier)?;
      let result = main_win.add_child(builder, init_pos, init_size);

      match result {
        Ok(webview) => {
          let keep_behind = {
            let locked = self.inner.lock().await;
            cfg!(target_os = "macos")
              && locked.active_slot.as_ref().is_none_or(|s| !s.committed)
              && locked.in_flight_group_members.contains_key(key)
          };
          if keep_behind {
            #[cfg(target_os = "macos")]
            if let Some(mgr_wv) = app.get_webview("manager") {
              platform::raise_macos_webview_to_front(&mgr_wv);
            }
          } else {
            let _ = webview.hide();
          }
          return self
            .navigate_desktop_account(app, &webview, key, view_instance_id, load_gen)
            .await;
        }
        #[cfg(windows)]
        Err(err) if platform::windows_webview_options_in_use(&err) => {
          if tokio::time::Instant::now() >= retry_until {
            return Err("该账号的旧 WebView2 会话尚未退出，请稍后重新打开账号".to_string());
          }
          tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        Err(err) => return Err(format!("创建独立账号视图失败: {err}")),
      }
    }
  }

  #[cfg(desktop)]
  async fn navigate_desktop_account(
    self: &Arc<Self>,
    app: &AppHandle,
    webview: &tauri::Webview,
    key: &AccountKey,
    view_instance_id: u64,
    load_gen: &Arc<AtomicU64>,
  ) -> Result<(), String> {
    let _ = load_gen;
    let (proxy_url, timezone, uuid, profile_dir, entry_url_str) = {
      let locked = self.inner.lock().await;
      if !Self::is_view_instance_valid(&locked, key, view_instance_id) {
        drop(locked);
        let _ = webview.close();
        return Ok(());
      }
      let spec = resolve_provider(&key.provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      let account = locked
        .store
        .accounts
        .iter()
        .find(|account| account.provider_id == key.provider_id && account.uuid == key.uuid)
        .ok_or_else(|| "账号不存在，已阻止加载".to_string())?;
      let proxy_url = platform::parse_account_proxy(account.proxy_url.as_deref().unwrap_or_default())?;
      let timezone = account.timezone.clone();
      let uuid = Uuid::parse_str(&key.uuid).map_err(|err| format!("非法 UUID: {err}"))?;
      let profile_dir = locked.app_data_dir.join("profiles").join(&key.uuid);
      (proxy_url, timezone, uuid, profile_dir, spec.info.entry_url)
    };
    let uses_proxy = proxy_url.is_some();

    platform::verify_desktop_profile_binding(webview, uuid, &profile_dir).await?;
    #[cfg(windows)]
    platform::configure_windows_timezone(webview, timezone.as_deref()).await?;
    #[cfg(not(windows))]
    let _ = timezone;
    #[cfg(target_os = "macos")]
    platform::configure_desktop_proxy(webview, proxy_url).await?;
    #[cfg(target_os = "macos")]
    if uses_proxy {
      let mgr = Arc::clone(self);
      let app = app.clone();
      let key = key.clone();
      platform::on_macos_navigation_failed(webview, move || {
        let mgr = Arc::clone(&mgr);
        let app = app.clone();
        let key = key.clone();
        tauri::async_runtime::spawn(async move {
          mgr
            .fail_initial_proxy_load(
              &app,
              key,
              view_instance_id,
              "页面加载失败，请检查该账号的代理地址和连接",
            )
            .await;
        });
      })
      .await?;
    }
    #[cfg(windows)]
    if uses_proxy {
      let mgr = Arc::clone(self);
      let app = app.clone();
      let key = key.clone();
      let load_gen_win = Arc::clone(load_gen);
      platform::on_windows_navigation_completed(webview, move |result| {
        let mgr = Arc::clone(&mgr);
        let app = app.clone();
        let key = key.clone();
        let load_gen_cb = Arc::clone(&load_gen_win);
        tauri::async_runtime::spawn(async move {
          match result {
            Ok(url) => {
              let expected_gen = load_gen_cb.fetch_add(1, Ordering::SeqCst) + 1;
              mgr
                .wait_paint_and_finalize_desktop(
                  &app,
                  key,
                  view_instance_id,
                  url,
                  expected_gen,
                  false,
                )
                .await;
            }
            Err(()) => {
              mgr.fail_initial_proxy_load(
                &app,
                key,
                view_instance_id,
                "页面加载失败，请检查该账号的代理地址和连接",
              )
              .await;
            }
          }
        });
      })
      .await?;
    }

    let entry_url = entry_url_str
      .parse()
      .map_err(|err| format!("服务入口地址错误: {err}"))?;
    {
      let locked = self.inner.lock().await;
      if !Self::is_view_instance_valid(&locked, key, view_instance_id) {
        drop(locked);
        let _ = webview.close();
        return Ok(());
      }
    }
    webview.navigate(entry_url).map_err(|err| format!("导航到服务入口失败: {err}"))?;

    if uses_proxy {
      let mgr = Arc::clone(self);
      let app = app.clone();
      let key = key.clone();
      tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        mgr.fail_initial_proxy_load(
          &app,
          key,
          view_instance_id,
          "页面加载超时，请检查该账号的代理地址和连接",
        )
        .await;
      });
    }
    Ok(())
  }

  #[cfg(desktop)]
  async fn fail_initial_proxy_load(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    notice: &str,
  ) {
    let mut locked = self.inner.lock().await;
    if locked
      .warm_pool
      .get(&key)
      .is_some_and(|entry| entry.paint_ready || entry.navigation_reached)
    {
      return;
    }
    if self.discard_desktop_view_locked(app, &mut locked, &key, view_instance_id, notice.to_owned()) {
      locked.prewarm_failed.insert(key);
      self.emit_snapshot(app, &Self::snapshot_from_locked(&locked));
    }
  }

  #[cfg(desktop)]
  async fn open_desktop_session(
    self: &Arc<Self>,
    app: &AppHandle,
    account: AccountRecord,
    app_data_dir: PathBuf,
    expected_seq: u64,
    view_instance_id: u64,
    is_cold_start: bool,
  ) -> Result<ManagerSnapshot, String> {
    let parsed_uuid = Uuid::parse_str(&account.uuid).map_err(|e| format!("非法 UUID: {e}"))?;
    let profile_dir = app_data_dir.join("profiles").join(&account.uuid);

    #[cfg(windows)]
    if is_cold_start && !profile_dir.exists() {
      let mut locked = self.inner.lock().await;
      locked.notice = Some("上次已提交账号的 Windows WebView2 资料目录缺失，已停留在管理页".to_string());
      return Ok(Self::snapshot_from_locked(&locked));
    }
    #[cfg(not(windows))]
    let _ = is_cold_start;

    #[cfg(windows)]
    std::fs::create_dir_all(&profile_dir).map_err(|e| format!("创建资料目录失败: {e}"))?;

    let webview_label = format!("account-{view_instance_id}");
    let alive_flag = Arc::new(AtomicBool::new(true));
    let load_gen = Arc::new(AtomicU64::new(0));
    let key = AccountKey {
      provider_id: account.provider_id,
      uuid: account.uuid.clone(),
    };

    let loading_snap = {
      let mut locked = self.inner.lock().await;
      if self.current_seq.load(Ordering::SeqCst) != expected_seq {
        return Ok(Self::snapshot_from_locked(&locked));
      }
      if let Some(entry) = locked.warm_pool.remove(&key) {
        Self::close_warm_entry(app, entry);
      }
      locked.active_slot = Some(ActiveSessionSlot {
        account_key: key.clone(),
        view_instance_id,
        switch_seq: expected_seq,
        committed: false,
      });
      self.active_view_id.store(view_instance_id, Ordering::SeqCst);
      locked.warm_pool.insert(
        key.clone(),
        WarmDesktopEntry {
          account_key: key.clone(),
          view_instance_id,
          webview_label: webview_label.clone(),
          popup_label: None,
          paint_ready: false,
          staged_painted: false,
          navigation_reached: false,
          load_gen: Arc::clone(&load_gen),
          last_url: None,
          captured_identity_hint: None,
          watcher_running: false,
          probe_notify: Arc::new(tokio::sync::Notify::new()),
          last_used_ms: now_ms(),
          alive_flag: Arc::clone(&alive_flag),
        },
      );
      Self::evict_warm_pool_if_needed(app, &mut locked, Some(&key), MAX_WARM_DESKTOP_VIEWS);
      Self::apply_desktop_layout_locked(app, &locked);
      Self::snapshot_from_locked(&locked)
    };
    self.emit_snapshot(app, &loading_snap);

    let result = self
      .attach_desktop_account_webview(
        app,
        &key,
        parsed_uuid,
        &profile_dir,
        &webview_label,
        view_instance_id,
        &alive_flag,
        &load_gen,
      )
      .await;
    if let Err(err) = result {
      let mut locked = self.inner.lock().await;
      self.discard_desktop_view_locked(app, &mut locked, &key, view_instance_id, err);
      return Ok(Self::snapshot_from_locked(&locked));
    }

    {
      let locked = self.inner.lock().await;
      if locked
        .active_slot
        .as_ref()
        .is_some_and(|slot| {
          slot.account_key == key && slot.view_instance_id == view_instance_id && !slot.committed
        })
      {
        Self::apply_desktop_layout_locked(app, &locked);
      }
    }

    Ok(self.get_snapshot().await)
  }

  #[cfg(desktop)]
  async fn eval_paint_ready(webview: &tauri::Webview) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx_cell = Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_inner = Arc::clone(&tx_cell);
    if webview
      .eval_with_callback(DOM_PAINT_READY_JS, move |raw| {
        if let Some(sender) = tx_inner.lock().unwrap().take() {
          let _ = sender.send(raw);
        }
      })
      .is_err()
    {
      return false;
    }

    match tokio::time::timeout(std::time::Duration::from_millis(600), rx).await {
      Ok(Ok(raw)) => raw.trim() == "true",
      _ => false,
    }
  }

  #[cfg(desktop)]
  async fn wait_paint_and_finalize_desktop(
    self: &Arc<Self>,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    loaded_url: Url,
    expected_load_gen: u64,
    reset_paint_state: bool,
  ) {
    let wait_params = {
      let mut locked = self.inner.lock().await;
      let Some(spec) = resolve_provider(&key.provider_id) else {
        return;
      };
      if classify_top_level_url(&spec, &loaded_url, false) != NavigationDecision::AllowHttps {
        return;
      }
      locked.warm_pool.get_mut(&key).and_then(|e| {
        if e.view_instance_id == view_instance_id
          && e.load_gen.load(Ordering::SeqCst) == expected_load_gen
        {
          e.navigation_reached = true;
          Some((
            e.webview_label.clone(),
            e.paint_ready,
            Arc::clone(&e.alive_flag),
            Arc::clone(&e.load_gen),
          ))
        } else {
          None
        }
      })
    };

    let Some((wv_label, already_ready, alive_flag, load_gen)) = wait_params else {
      return;
    };

    if !already_ready || reset_paint_state {
      if reset_paint_state {
        if let Some(wv) = app.get_webview(&wv_label) {
          let _ = wv.eval("window.__maamReadySince = 0; window.__maamReadyFrames = 0;");
        }
      }
      for tick in 0..100 {
        if !alive_flag.load(Ordering::SeqCst)
          || load_gen.load(Ordering::SeqCst) != expected_load_gen
        {
          return;
        }
        let Some(wv) = app.get_webview(&wv_label) else {
          return;
        };
        #[cfg(target_os = "macos")]
        if platform::is_macos_webview_loading(&wv).await {
          tokio::time::sleep(std::time::Duration::from_millis(if tick < 20 { 80 } else { 150 }))
            .await;
          continue;
        }
        if Self::eval_paint_ready(&wv).await {
          tokio::time::sleep(std::time::Duration::from_millis(60)).await;
          if !alive_flag.load(Ordering::SeqCst)
            || load_gen.load(Ordering::SeqCst) != expected_load_gen
          {
            return;
          }
          #[cfg(target_os = "macos")]
          if platform::is_macos_webview_loading(&wv).await {
            continue;
          }
          break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(if tick < 20 { 80 } else { 150 }))
          .await;
      }
    }

    let check_params = {
      let locked = self.inner.lock().await;
      locked.warm_pool.get(&key).and_then(|e| {
        if e.view_instance_id == view_instance_id {
          Some((
            e.webview_label.clone(),
            locked.app_data_dir.join("profiles").join(&key.uuid),
          ))
        } else {
          None
        }
      })
    };

    let Some((wv_label, expected_dir)) = check_params else {
      return;
    };
    let Ok(parsed_uuid) = Uuid::parse_str(&key.uuid) else {
      return;
    };
    let Some(wv) = app.get_webview(&wv_label) else {
      return;
    };

    if let Err(err) =
      platform::verify_desktop_profile_binding(&wv, parsed_uuid, &expected_dir).await
    {
      let snap = {
        let mut locked = self.inner.lock().await;
        if !self.discard_desktop_view_locked(app, &mut locked, &key, view_instance_id, err) {
          return;
        }
        Self::snapshot_from_locked(&locked)
      };
      self.emit_snapshot(app, &snap);
      return;
    }

    let (snap_opt, start_watcher, probe_notify_opt) = {
      let mut locked = self.inner.lock().await;
      if !locked.conflicts.is_empty() || locked.store.pending_cleanup.contains(&key) {
        if let Some(entry) = locked.warm_pool.remove(&key) {
          Self::close_warm_entry(app, entry);
        }
        return;
      }
      let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        .cloned()
      else {
        if let Some(entry) = locked.warm_pool.remove(&key) {
          Self::close_warm_entry(app, entry);
        }
        return;
      };

      let is_foreground_waiting = locked
        .active_slot
        .as_ref()
        .map(|s| s.account_key == key && s.view_instance_id == view_instance_id && !s.committed)
        .unwrap_or(false);

      let mut need_watcher = false;
      let probe_notify;
      if let Some(entry) = locked.warm_pool.get_mut(&key) {
        if entry.view_instance_id != view_instance_id
          || entry.load_gen.load(Ordering::SeqCst) != expected_load_gen
        {
          return;
        }
        entry.paint_ready = true;
        if is_foreground_waiting {
          entry.staged_painted = true;
        }
        entry.last_url = Some(loaded_url);
        if !entry.watcher_running {
          entry.watcher_running = true;
          need_watcher = true;
        }
        probe_notify = Arc::clone(&entry.probe_notify);
      } else {
        return;
      }

      if is_foreground_waiting {
        let prev_last = locked.store.last_committed.clone();
        locked.store.last_committed = Some(key.clone());
        if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
          locked.store.last_committed = prev_last;
          locked.active_slot = None;
          self.active_view_id.store(0, Ordering::SeqCst);
          locked.notice = Some(format!("提交账号记录写盘失败，已回退管理页: {err}"));
          Self::apply_desktop_layout_locked(app, &locked);
          (Some(Self::snapshot_from_locked(&locked)), false, None)
        } else {
          if let Some(slot) = &mut locked.active_slot {
            slot.committed = true;
          }
          if let Some(entry) = locked.warm_pool.get_mut(&key) {
            entry.last_used_ms = now_ms();
          }
          locked
            .last_active_per_provider
            .insert(key.provider_id.clone(), key.clone());
          let short_name = resolve_provider(&key.provider_id)
            .map(|s| s.info.short_name)
            .unwrap_or_else(|| key.provider_id.as_str().to_string());
          let title = format!("{short_name} · {}", acc.alias);
          if let Some(main_win) = app.get_window("main") {
            let _ = main_win.set_title(&title);
          }
          Self::apply_desktop_layout_locked(app, &locked);
          (Some(Self::snapshot_from_locked(&locked)), need_watcher, Some(probe_notify))
        }
      } else {
        (Some(Self::snapshot_from_locked(&locked)), need_watcher, Some(probe_notify))
      }
    };

    if let Some(probe_notify) = probe_notify_opt {
      if start_watcher {
        self.spawn_desktop_identity_watcher(app.clone(), key, view_instance_id, probe_notify);
      } else {
        probe_notify.notify_one();
      }
    }

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }

    let mgr_next = Arc::clone(self);
    let app_next = app.clone();
    tauri::async_runtime::spawn(async move {
      tokio::time::sleep(std::time::Duration::from_millis(350)).await;
      mgr_next.prewarm_next_cold_account(&app_next).await;
    });
  }

  #[cfg(desktop)]
  async fn prewarm_desktop_account(
    self: &Arc<Self>,
    app: &AppHandle,
    account: AccountRecord,
  ) -> Result<(), String> {
    self
      .prewarm_desktop_account_with_limit(app, account, false)
      .await
  }

  #[cfg(desktop)]
  async fn prewarm_desktop_account_with_limit(
    self: &Arc<Self>,
    app: &AppHandle,
    account: AccountRecord,
    allow_evict_for_group: bool,
  ) -> Result<(), String> {
    let key = AccountKey {
      provider_id: account.provider_id,
      uuid: account.uuid.clone(),
    };
    let app_data_dir = {
      let mut locked = self.inner.lock().await;
      if locked.warm_pool.contains_key(&key) {
        return Ok(());
      }
      if locked.store.pending_cleanup.contains(&key) {
        return Err("账号资料正在清理".to_string());
      }
      if allow_evict_for_group && locked.warm_pool.len() >= MAX_WARM_DESKTOP_VIEWS {
        Self::evict_warm_pool_if_needed(
          app,
          &mut locked,
          Some(&key),
          MAX_WARM_DESKTOP_VIEWS.saturating_sub(1),
        );
      }
      if locked.warm_pool.len() >= MAX_WARM_DESKTOP_VIEWS {
        return if allow_evict_for_group {
          Err("后台常驻窗口数已达上限，请减少当前群聊或活跃会话后重试".to_string())
        } else {
          Ok(())
        };
      }
      locked.app_data_dir.clone()
    };

    let parsed_uuid = Uuid::parse_str(&account.uuid).map_err(|e| format!("非法 UUID: {e}"))?;
    let profile_dir = app_data_dir.join("profiles").join(&account.uuid);

    #[cfg(windows)]
    std::fs::create_dir_all(&profile_dir).map_err(|e| format!("创建资料目录失败: {e}"))?;

    let view_instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
    let webview_label = format!("account-{view_instance_id}");
    let alive_flag = Arc::new(AtomicBool::new(true));
    let load_gen = Arc::new(AtomicU64::new(0));

    {
      let mut locked = self.inner.lock().await;
      if locked.warm_pool.contains_key(&key)
        || locked.store.pending_cleanup.contains(&key)
        || locked.warm_pool.len() >= MAX_WARM_DESKTOP_VIEWS
        || !locked.store.accounts.iter().any(|record| {
          record.provider_id == key.provider_id && record.uuid == key.uuid
        })
      {
        return Ok(());
      }
      locked.warm_pool.insert(
        key.clone(),
        WarmDesktopEntry {
          account_key: key.clone(),
          view_instance_id,
          webview_label: webview_label.clone(),
          popup_label: None,
          paint_ready: false,
          staged_painted: false,
          navigation_reached: false,
          load_gen: Arc::clone(&load_gen),
          last_url: None,
          captured_identity_hint: None,
          watcher_running: false,
          probe_notify: Arc::new(tokio::sync::Notify::new()),
          last_used_ms: 0,
          alive_flag: Arc::clone(&alive_flag),
        },
      );
    }

    let result = self
      .attach_desktop_account_webview(
        app, &key, parsed_uuid, &profile_dir, &webview_label, view_instance_id, &alive_flag, &load_gen,
      )
      .await;
    if let Err(err) = result {
      let mut locked = self.inner.lock().await;
      if self.discard_desktop_view_locked(app, &mut locked, &key, view_instance_id, err.clone()) {
        return Err(err);
      }
    }

    {
      let locked = self.inner.lock().await;
      if locked.in_flight_group_members.contains_key(&key) {
        Self::apply_desktop_layout_locked(app, &locked);
      }
    }

    Ok(())
  }

  #[cfg(desktop)]
  pub fn prewarm_next_cold_account(
    self: &Arc<Self>,
    app: &AppHandle,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    let mgr = Arc::clone(self);
    let app_handle = app.clone();
    Box::pin(async move {
      let next_cold = {
        let locked = mgr.inner.lock().await;
        let in_flight = locked.warm_pool.values().filter(|e| !e.paint_ready).count();
        if locked.active_group_id.is_some()
          || locked.warm_pool.len() >= MAX_WARM_DESKTOP_VIEWS
          || in_flight >= MAX_CONCURRENT_PREWARMS
        {
          None
        } else {
          let is_eligible_cold = |a: &&AccountRecord| {
            if !resolve_provider(&a.provider_id).is_some_and(|s| s.info.enabled) {
              return false;
            }
            let k = AccountKey {
              provider_id: a.provider_id.clone(),
              uuid: a.uuid.clone(),
            };
            !locked.warm_pool.contains_key(&k)
              && !locked.store.pending_cleanup.contains(&k)
              && !locked.prewarm_failed.contains(&k)
          };
          locked
            .store
            .accounts
            .iter()
            .filter(is_eligible_cold)
            .find(|a| a.provider_id == locked.selected_provider_id)
            .or_else(|| locked.store.accounts.iter().find(is_eligible_cold))
            .cloned()
        }
      };

      if let Some(account) = next_cold {
        let failed_key = AccountKey {
          provider_id: account.provider_id.clone(),
          uuid: account.uuid.clone(),
        };
        if mgr.prewarm_desktop_account(&app_handle, account).await.is_err() {
          {
            let mut locked = mgr.inner.lock().await;
            locked.prewarm_failed.insert(failed_key);
          }
          tokio::time::sleep(std::time::Duration::from_millis(400)).await;
          mgr.prewarm_next_cold_account(&app_handle).await;
        }
      }
    })
  }

  #[cfg(not(desktop))]
  pub fn prewarm_next_cold_account(
    self: &Arc<Self>,
    _app: &AppHandle,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {})
  }

  #[cfg(desktop)]
  async fn register_desktop_popup(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    popup_label: String,
  ) {
    let mut locked = self.inner.lock().await;
    if self.active_view_id.load(Ordering::SeqCst) != view_instance_id {
      if let Some(pw) = app.get_webview_window(&popup_label) {
        let _ = pw.close();
      }
      return;
    }
    let Some(entry) = locked.warm_pool.get_mut(&key) else {
      if let Some(pw) = app.get_webview_window(&popup_label) {
        let _ = pw.close();
      }
      return;
    };
    if entry.view_instance_id != view_instance_id {
      if let Some(pw) = app.get_webview_window(&popup_label) {
        let _ = pw.close();
      }
      return;
    }
    if let Some(prev_popup) = entry.popup_label.replace(popup_label) {
      if let Some(pw) = app.get_webview_window(&prev_popup) {
        let _ = pw.close();
      }
    }
  }

  #[cfg(mobile)]
  async fn open_mobile_session(
    self: &Arc<Self>,
    app: &AppHandle,
    account: AccountRecord,
    expected_seq: u64,
    view_instance_id: u64,
    is_cold_start: bool,
  ) -> Result<ManagerSnapshot, String> {
    let caps = match crate::platform::mobile::check_capabilities(app).await {
      Ok(c) => c,
      Err(err) => {
        let mut locked = self.inner.lock().await;
        locked.notice = Some(format!("检测移动端 WebView 独立容器能力失败: {err}"));
        return Ok(Self::snapshot_from_locked(&locked));
      }
    };

    if !caps.multi_profile_supported {
      let mut locked = self.inner.lock().await;
      #[cfg(target_os = "ios")]
      let msg = "当前系统 WKWebView 不支持独立会话隔离（需 iOS 17+）";
      #[cfg(not(target_os = "ios"))]
      let msg = "当前设备系统 WebView 不支持 MULTI_PROFILE 多资料隔离，请先更新 Android System WebView 后重试";
      locked.notice = Some(msg.to_string());
      return Ok(Self::snapshot_from_locked(&locked));
    }

    if is_cold_start && !caps.profile_names.iter().any(|n| n == &account.uuid) {
      let mut locked = self.inner.lock().await;
      locked.notice = Some("上次已提交账号的系统 Profile 已丢失，已停留在管理页".to_string());
      return Ok(Self::snapshot_from_locked(&locked));
    }

    let key = AccountKey {
      provider_id: account.provider_id.clone(),
      uuid: account.uuid.clone(),
    };

    let spec = {
      let mut locked = self.inner.lock().await;
      if self.current_seq.load(Ordering::SeqCst) != expected_seq {
        return Ok(Self::snapshot_from_locked(&locked));
      }
      let spec = resolve_provider(&account.provider_id)
        .ok_or_else(|| "未找到对应的服务配置".to_string())?;
      locked.active_slot = Some(ActiveSessionSlot {
        account_key: key,
        view_instance_id,
        switch_seq: expected_seq,
        committed: false,
      });
      spec
    };

    let timezone_init_script = account
      .timezone
      .as_deref()
      .map(platform::build_timezone_override_init_js);
    let payload = crate::platform::mobile::OpenMobileAccountPayload {
      provider_id: account.provider_id.as_str().to_string(),
      provider_name: spec.info.short_name,
      uuid: account.uuid.clone(),
      alias: account.alias.clone(),
      auto_alias: account.auto_alias,
      entry_url: spec.info.entry_url,
      allowed_hosts: spec.allowed_hosts,
      proxy_url: account.proxy_url.clone(),
      timezone: account.timezone.clone(),
      timezone_init_script,
      switch_seq: expected_seq,
      view_instance_id,
      is_cold_start,
    };

    if let Err(err) = crate::platform::mobile::open_account_session(app, payload).await {
      let mut locked = self.inner.lock().await;
      if self.current_seq.load(Ordering::SeqCst) == expected_seq {
        locked.active_slot = None;
        locked.notice = Some(format!("打开独立会话页面失败: {err}"));
      }
      return Ok(Self::snapshot_from_locked(&locked));
    }

    Ok(self.get_snapshot().await)
  }

  #[cfg(mobile)]
  async fn handle_mobile_event(
    self: &Arc<Self>,
    app: &AppHandle,
    event: crate::platform::mobile::MobileSessionEvent,
  ) {
    use crate::platform::mobile::MobileSessionEvent;
    match event {
      MobileSessionEvent::PageCommitVisible {
        provider_id,
        uuid,
        switch_seq,
        view_instance_id,
        url,
      } => {
        let Some(pid) = ProviderId::parse(&provider_id) else {
          return;
        };
        let Some(norm_uuid) = normalize_uuid(&uuid) else {
          return;
        };
        let Ok(parsed_url) = Url::parse(&url) else {
          return;
        };
        self
          .commit_mobile_session(
            app,
            AccountKey {
              provider_id: pid,
              uuid: norm_uuid,
            },
            view_instance_id,
            switch_seq,
            parsed_url,
          )
          .await;
      }
      MobileSessionEvent::IdentityDetected {
        provider_id,
        uuid,
        switch_seq,
        view_instance_id,
        identity,
        avatar_url,
        quota_text,
        quota_percent,
        quota_detail_json,
      } => {
        let Some(pid) = ProviderId::parse(&provider_id) else {
          return;
        };
        let Some(norm_uuid) = normalize_uuid(&uuid) else {
          return;
        };
        let _ = switch_seq;
        let parsed_detail = quota_detail_json
          .as_deref()
          .and_then(|s| serde_json::from_str::<QuotaDetail>(s).ok());
        self
          .apply_detected_metadata(
            app,
            AccountKey {
              provider_id: pid,
              uuid: norm_uuid,
            },
            view_instance_id,
            if identity.is_empty() { None } else { Some(identity) },
            None,
            avatar_url,
            quota_text,
            quota_percent,
            parsed_detail,
          )
          .await;
      }
      MobileSessionEvent::SessionFailed {
        switch_seq,
        view_instance_id,
        reason,
      } => {
        let snap = {
          let mut locked = self.inner.lock().await;
          if self.current_seq.load(Ordering::SeqCst) != switch_seq {
            return;
          }
          let matches = locked
            .active_slot
            .as_ref()
            .map(|s| s.switch_seq == switch_seq && s.view_instance_id == view_instance_id)
            .unwrap_or(false);
          if !matches {
            return;
          }
          locked.active_slot = None;
          locked.notice = Some(reason);
          Self::snapshot_from_locked(&locked)
        };
        self.emit_snapshot(app, &snap);
      }
      MobileSessionEvent::ActivityClosed {
        switch_seq,
        view_instance_id,
      } => {
        let snap = {
          let mut locked = self.inner.lock().await;
          let matches = locked
            .active_slot
            .as_ref()
            .map(|s| s.switch_seq == switch_seq && s.view_instance_id == view_instance_id)
            .unwrap_or(false);
          if !matches {
            return;
          }
          locked.active_slot = None;
          locked.pending_external_auth = None;
          Self::snapshot_from_locked(&locked)
        };
        self.emit_snapshot(app, &snap);
      }
      MobileSessionEvent::ExternalAuthUnavailable {
        switch_seq,
        view_instance_id,
        reason,
      } => {
        let snap = {
          let mut locked = self.inner.lock().await;
          let matches = locked
            .active_slot
            .as_ref()
            .map(|s| s.switch_seq == switch_seq && s.view_instance_id == view_instance_id)
            .unwrap_or(false);
          if !matches {
            return;
          }
          locked.pending_external_auth = None;
          locked.notice = Some(reason);
          Self::snapshot_from_locked(&locked)
        };
        self.emit_snapshot(app, &snap);
      }
      MobileSessionEvent::RecreateRequested { previous_switch_seq } => {
        let fallback = {
          let mut locked = self.inner.lock().await;
          if self.current_seq.load(Ordering::SeqCst) == previous_switch_seq {
            locked.active_slot = None;
          }
          locked.store.last_committed.clone()
        };
        if let Some(committed_key) = fallback {
          let _ = self.switch_account_internal(app, committed_key, true).await;
        } else {
          let snap = self.get_snapshot().await;
          self.emit_snapshot(app, &snap);
        }
      }
      MobileSessionEvent::UpdateProgress {
        stage,
        downloaded,
        total,
        percent,
        message,
      } => {
        let _ = app.emit(
          "update-progress",
          crate::updater::UpdateProgressEvent {
            stage,
            downloaded,
            total,
            percent,
            message,
          },
        );
      }
      MobileSessionEvent::SessionLoggedOut {
        provider_id,
        uuid,
        switch_seq,
        view_instance_id,
      } => {
        let _ = switch_seq;
        let Some(pid) = ProviderId::parse(&provider_id) else {
          return;
        };
        let Some(norm_uuid) = normalize_uuid(&uuid) else {
          return;
        };
        self
          .apply_logged_out_state(
            app,
            AccountKey {
              provider_id: pid,
              uuid: norm_uuid,
            },
            view_instance_id,
          )
          .await;
      }
    }
  }

  #[cfg(mobile)]
  async fn commit_mobile_session(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    expected_seq: u64,
    committed_url: Url,
  ) {
    if self.current_seq.load(Ordering::SeqCst) != expected_seq {
      return;
    }

    let snap_opt = {
      let mut locked = self.inner.lock().await;
      if self.current_seq.load(Ordering::SeqCst) != expected_seq {
        return;
      }
      let Some(spec) = resolve_provider(&key.provider_id) else {
        return;
      };
      if classify_top_level_url(&spec, &committed_url, false) != NavigationDecision::AllowHttps {
        return;
      }
      if !locked.conflicts.is_empty() || locked.store.pending_cleanup.contains(&key) {
        self.hide_foreground_slot(app, expected_seq, &mut locked).await;
        return;
      }
      let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        .cloned()
      else {
        self.hide_foreground_slot(app, expected_seq, &mut locked).await;
        return;
      };

      let slot_matches = locked
        .active_slot
        .as_ref()
        .map(|s| {
          s.account_key == key
            && s.view_instance_id == view_instance_id
            && s.switch_seq == expected_seq
        })
        .unwrap_or(false);
      if !slot_matches {
        return;
      }

      let prev_last = locked.store.last_committed.clone();
      locked.store.last_committed = Some(key.clone());
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.last_committed = prev_last;
        self.hide_foreground_slot(app, expected_seq, &mut locked).await;
        locked.notice = Some(format!("提交账号记录写盘失败，已回退管理页: {err}"));
        Some(Self::snapshot_from_locked(&locked))
      } else {
        if let Some(slot) = &mut locked.active_slot {
          slot.committed = true;
        }
        let title = format!("{} · {}", spec.info.short_name, acc.alias);
        let _ = crate::platform::mobile::update_account_title(app, expected_seq, &title).await;
        Some(Self::snapshot_from_locked(&locked))
      }
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  #[cfg(desktop)]
  async fn notify_external_auth_policy(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
  ) {
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      let matches = locked
        .active_slot
        .as_ref()
        .map(|s| s.account_key == key && s.view_instance_id == view_instance_id)
        .unwrap_or(false);
      if !matches {
        return;
      }
      let cur_seq = self.current_seq.load(Ordering::SeqCst);
      let entry_ok = resolve_provider(&key.provider_id)
        .and_then(|spec| {
          Url::parse(&spec.info.entry_url)
            .ok()
            .map(|u| is_verified_callback_origin(&spec, &u))
        })
        .unwrap_or(false);
      locked.pending_external_auth = Some(PendingExternalAuth {
        account_key: key.clone(),
        switch_seq: cur_seq,
        nonce: format!("{}-{}", key.provider_id.as_str(), Uuid::new_v4().simple()),
      });
      if !entry_ok || locked.pending_external_auth.is_some() {
        locked.pending_external_auth = None;
      }
      locked.notice = Some(
        "Google OAuth 按政策禁止内嵌 WebView，请使用邮箱、手机号登录，或点击账号设置使用「导入 Cookie」载入系统浏览器登录态"
          .to_string(),
      );
      Some(Self::snapshot_from_locked(&locked))
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  #[cfg(desktop)]
  async fn probe_meta_account_center_email(
    self: Arc<Self>,
    app: AppHandle,
    key: AccountKey,
    view_instance_id: u64,
  ) {
    let Ok(meta_url) = Url::parse("https://accountscenter.meta.com/") else {
      return;
    };
    let Ok(parsed_uuid) = Uuid::parse_str(&key.uuid) else {
      return;
    };

    let (proxy_raw, profile_dir, has_win_cookies) = {
      let locked = self.inner.lock().await;
      if !Self::is_view_instance_valid(&locked, &key, view_instance_id) {
        return;
      }
      let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid && a.auto_alias)
      else {
        return;
      };
      let proxy_str = acc.proxy_url.clone().unwrap_or_default();
      let p_dir = locked.app_data_dir.join("profiles").join(&key.uuid);
      #[cfg(windows)]
      let win_ok = self
        .read_windows_cookies_for_url(&app, &locked, &key, meta_url.clone())
        .await
        .is_ok_and(|(hdr, _)| !hdr.trim().is_empty());
      #[cfg(not(windows))]
      let win_ok = true;
      (proxy_str, p_dir, win_ok)
    };

    #[cfg(target_os = "macos")]
    {
      let Ok((hdr, _)) =
        platform::read_macos_cookies_for_url(&app, parsed_uuid, meta_url.clone()).await
      else {
        return;
      };
      if hdr.trim().is_empty() {
        return;
      }
    }
    #[cfg(windows)]
    {
      if !has_win_cookies {
        return;
      }
    }
    #[cfg(not(windows))]
    let _ = has_win_cookies;

    let Ok(proxy) = platform::parse_account_proxy(&proxy_raw) else {
      return;
    };
    let Some(window) = app.get_window("main") else {
      return;
    };
    let instance_id = self.view_counter.fetch_add(1, Ordering::SeqCst);
    #[allow(unused_mut)]
    let mut builder = tauri::webview::WebviewBuilder::new(
      format!("meta-id-{instance_id}"),
      tauri::WebviewUrl::External("about:blank".parse().expect("valid blank url")),
    )
    .focused(false)
    .on_navigation(|nav_url| {
      if nav_url.as_str() == "about:blank" {
        return true;
      }
      if nav_url.scheme() != "https" {
        return false;
      }
      nav_url
        .host_str()
        .map(|h| {
          let low = h.to_ascii_lowercase();
          low == "meta.com" || low.ends_with(".meta.com")
        })
        .unwrap_or(false)
    });

    #[cfg(target_os = "macos")]
    {
      let _ = &proxy;
      builder = builder
        .data_store_identifier(*parsed_uuid.as_bytes())
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
    }
    #[cfg(not(target_os = "macos"))]
    {
      builder = builder.data_directory(profile_dir.clone());
    }
    #[cfg(windows)]
    {
      let Ok(b) = platform::with_windows_account_proxy(
        builder,
        proxy.as_ref(),
        &app.config().identifier,
      ) else {
        return;
      };
      builder = b;
    }

    let (position, size) = Self::initial_hidden_child_rect(&window);
    let Ok(temp_wv) = window.add_child(builder, position, size) else {
      return;
    };
    let _ = temp_wv.hide();

    let run_probe = async {
      if platform::verify_desktop_profile_binding(&temp_wv, parsed_uuid, &profile_dir)
        .await
        .is_err()
      {
        return;
      }
      if temp_wv.navigate(meta_url).is_err() {
        return;
      }
      for attempt in 0..16 {
        tokio::time::sleep(std::time::Duration::from_millis(if attempt == 0 { 800 } else { 600 }))
          .await;
        let still_needed = {
          let locked = self.inner.lock().await;
          Self::is_view_instance_valid(&locked, &key, view_instance_id)
            && locked
              .store
              .accounts
              .iter()
              .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid && a.auto_alias)
        };
        if !still_needed {
          break;
        }
        let Some(probe) = Self::eval_identity_probe(&temp_wv).await else {
          continue;
        };
        if probe.logged_out {
          break;
        }
        if !probe.email.is_empty() {
          let _ = self
            .apply_detected_metadata(
              &app,
              key.clone(),
              view_instance_id,
              Some(probe.email),
              None,
              None,
              None,
              None,
              None,
            )
            .await;
          break;
        }
      }
    };

    run_probe.await;
    let _ = temp_wv.close();
  }

  #[cfg(desktop)]
  fn spawn_desktop_identity_watcher(
    self: &Arc<Self>,
    app: AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    probe_notify: Arc<tokio::sync::Notify>,
  ) {
    let mgr = Arc::clone(self);
    tauri::async_runtime::spawn(async move {
      let mut tick_count: u32 = 0;
      let mut consecutive_auth_ticks: u32 = 0;
      let mut consecutive_logged_out: u8 = 0;
      let mut consecutive_page_error: u8 = 0;
      let mut stale_session_reset_done = false;
      let mut meta_center_probed = false;
      let mut next_delay_ms: Option<u64> = None;
      loop {
        if let Some(delay_ms) = next_delay_ms.take() {
          if consecutive_logged_out == 1 || consecutive_page_error == 1 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
          } else {
            tokio::select! {
              _ = tokio::time::sleep(std::time::Duration::from_millis(delay_ms)) => {},
              _ = probe_notify.notified() => {},
            }
          }
        }
        tick_count = tick_count.saturating_add(1);

        let labels_opt = {
          let locked = mgr.inner.lock().await;
          let account_exists = locked
            .store
            .accounts
            .iter()
            .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid);
          if !account_exists {
            None
          } else {
            locked.warm_pool.get(&key).and_then(|e| {
              if e.view_instance_id == view_instance_id && e.alive_flag.load(Ordering::SeqCst) {
                Some((e.webview_label.clone(), e.popup_label.clone()))
              } else {
                None
              }
            })
          }
        };

        let Some((wv_label, popup_label)) = labels_opt else {
          let mut locked = mgr.inner.lock().await;
          if let Some(entry) = locked.warm_pool.get_mut(&key) {
            if entry.view_instance_id == view_instance_id {
              entry.watcher_running = false;
            }
          }
          break;
        };

        if let Some(p_label) = popup_label {
          if let Some(p_wv) = app.get_webview(&p_label) {
            if let Some(probe) = Self::eval_identity_probe(&p_wv).await {
              let popup_hint = if !probe.email.is_empty() {
                Some(probe.email)
              } else if !probe.typed.is_empty() {
                Some(probe.typed)
              } else {
                None
              };
              if let Some(hint_val) = popup_hint {
                let mut locked = mgr.inner.lock().await;
                if let Some(entry) = locked.warm_pool.get_mut(&key) {
                  if entry.view_instance_id == view_instance_id {
                    entry.captured_identity_hint = Some(hint_val);
                  }
                }
              }
            }
          }
        }

        let Some(main_wv) = app.get_webview(&wv_label) else {
          let mut locked = mgr.inner.lock().await;
          if let Some(entry) = locked.warm_pool.get_mut(&key) {
            if entry.view_instance_id == view_instance_id {
              entry.watcher_running = false;
            }
          }
          break;
        };
        let Some(probe) = Self::eval_identity_probe(&main_wv).await else {
          next_delay_ms = Some(if tick_count < 6 { 800 } else { 6500 });
          continue;
        };

        if let Some(idx) = probe.pending_hotkey {
          let mgr_hotkey = Arc::clone(&mgr);
          let app_hotkey = app.clone();
          tauri::async_runtime::spawn(async move {
            let _ = mgr_hotkey.switch_account_by_index(&app_hotkey, idx).await;
          });
        }

        let (resolved_identity, soft_identity) = {
          let mut locked = mgr.inner.lock().await;
          let cookie_hint = locked.cookie_identity_hints.get(&key).cloned();
          if let Some(entry) = locked.warm_pool.get_mut(&key) {
            if entry.view_instance_id == view_instance_id {
              if !probe.typed.is_empty() {
                entry.captured_identity_hint = Some(probe.typed);
              }
              if probe.is_auth {
                let strong = if !probe.email.is_empty() {
                  Some(probe.email)
                } else if entry.captured_identity_hint.is_some() {
                  entry.captured_identity_hint.clone()
                } else {
                  cookie_hint
                };
                let soft = if strong.is_none() {
                  if !probe.display_name.is_empty() {
                    Some(probe.display_name)
                  } else if consecutive_auth_ticks >= 3 {
                    Some("Signed-in account".to_string())
                  } else {
                    None
                  }
                } else {
                  None
                };
                (strong, soft)
              } else {
                (None, None)
              }
            } else {
              (None, None)
            }
          } else {
            (None, None)
          }
        };

        if probe.is_auth {
          consecutive_auth_ticks = consecutive_auth_ticks.saturating_add(1);
          consecutive_logged_out = 0;
          consecutive_page_error = 0;
          stale_session_reset_done = false;
          if key.provider_id.is_muse() && resolved_identity.is_none() && !meta_center_probed {
            let needs_meta_probe = {
              let locked = mgr.inner.lock().await;
              locked
                .store
                .accounts
                .iter()
                .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid && a.auto_alias)
            };
            if needs_meta_probe {
              meta_center_probed = true;
              tauri::async_runtime::spawn(
                Arc::clone(&mgr).probe_meta_account_center_email(
                  app.clone(),
                  key.clone(),
                  view_instance_id,
                ),
              );
            }
          }
          let (just_authenticated, needs_cookie_check) = mgr
            .apply_detected_metadata(
              &app,
              key.clone(),
              view_instance_id,
              resolved_identity,
              soft_identity,
              if probe.avatar.is_empty() {
                None
              } else {
                Some(probe.avatar)
              },
              if probe.quota_text.is_empty() {
                None
              } else {
                Some(probe.quota_text)
              },
              probe.quota_pct,
              probe.quota_detail,
            )
            .await;
          if just_authenticated || needs_cookie_check {
            let mgr_cookie = Arc::clone(&mgr);
            let app_cookie = app.clone();
            let key_cookie = key.clone();
            tauri::async_runtime::spawn(async move {
              mgr_cookie
                .refresh_account_cookie_expiry(&app_cookie, key_cookie, just_authenticated)
                .await;
            });
          }
          next_delay_ms = Some(if tick_count < 6 { 950 } else { 6500 });
        } else if probe.logged_out {
          consecutive_auth_ticks = 0;
          meta_center_probed = false;
          consecutive_page_error = 0;
          consecutive_logged_out = consecutive_logged_out.saturating_add(1);
          if consecutive_logged_out >= 2 {
            mgr
              .apply_logged_out_state(&app, key.clone(), view_instance_id)
              .await;
            if probe.stale_session && !stale_session_reset_done {
              stale_session_reset_done = true;
              if key.provider_id.is_muse() {
                if let Ok(u) = Url::parse("https://muse.ai/api/logout") {
                  let _ = main_wv.navigate(u);
                }
              } else if key.provider_id.is_hark() {
                if let Ok(u) = Url::parse("https://app.hark.com/login") {
                  let _ = main_wv.navigate(u);
                }
              }
            }
            next_delay_ms = Some(if tick_count < 6 { 950 } else { 6500 });
          } else {
            next_delay_ms = Some(300);
          }
        } else if !probe.page_error.is_empty() {
          consecutive_auth_ticks = 0;
          consecutive_logged_out = 0;
          consecutive_page_error = consecutive_page_error.saturating_add(1);
          if consecutive_page_error >= 2 {
            mgr
              .apply_page_error_state(&app, key.clone(), view_instance_id, probe.page_error)
              .await;
            next_delay_ms = Some(if tick_count < 6 { 950 } else { 6500 });
          } else {
            next_delay_ms = Some(400);
          }
        } else {
          consecutive_auth_ticks = 0;
          consecutive_logged_out = 0;
          consecutive_page_error = 0;
          next_delay_ms = Some(if tick_count < 6 { 500 } else { 6500 });
        }
      }
    });
  }

  #[cfg(desktop)]
  async fn eval_identity_probe(webview: &tauri::Webview) -> Option<DesktopProbeResult> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx_cell = Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_inner = Arc::clone(&tx_cell);
    webview
      .eval_with_callback(IDENTITY_PROBE_JS, move |raw| {
        if let Some(sender) = tx_inner.lock().unwrap().take() {
          let _ = sender.send(raw);
        }
      })
      .ok()?;

    let raw = tokio::time::timeout(std::time::Duration::from_millis(1200), rx)
      .await
      .ok()?
      .ok()?;

    let inner_json = serde_json::from_str::<String>(&raw).ok()?;
    let val = serde_json::from_str::<serde_json::Value>(&inner_json).ok()?;
    let is_auth = val.get("auth").and_then(|v| v.as_bool()).unwrap_or(false);
    let logged_out = val.get("loggedOut").and_then(|v| v.as_bool()).unwrap_or(false);
    let stale_session = val.get("staleSession").and_then(|v| v.as_bool()).unwrap_or(false);
    let page_error = val
      .get("pageError")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let typed = val
      .get("typed")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let email = val
      .get("email")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let display_name = val
      .get("displayName")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let avatar = val
      .get("avatar")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let quota_text = val
      .get("quotaText")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let quota_pct = val
      .get("quotaPct")
      .and_then(|v| v.as_u64())
      .map(|n| n.min(100) as u8);
    let quota_detail = val
      .get("quotaDetail")
      .filter(|v| !v.is_null())
      .and_then(|v| serde_json::from_value::<QuotaDetail>(v.clone()).ok())
      .filter(|d| !d.plan_label.trim().is_empty() || !d.plan_value.trim().is_empty());
    let pending_hotkey = val
      .get("pendingHotkey")
      .and_then(|v| v.as_u64())
      .map(|n| n as u8);

    Some(DesktopProbeResult {
      is_auth,
      logged_out,
      stale_session,
      page_error,
      typed,
      email,
      display_name,
      avatar,
      quota_text,
      quota_pct,
      quota_detail,
      pending_hotkey,
    })
  }

  fn is_view_instance_valid(locked: &InnerState, key: &AccountKey, view_instance_id: u64) -> bool {
    #[cfg(desktop)]
    {
      locked
        .warm_pool
        .get(key)
        .map(|e| e.view_instance_id == view_instance_id)
        .unwrap_or(false)
    }
    #[cfg(not(desktop))]
    {
      locked
        .active_slot
        .as_ref()
        .map(|s| s.account_key == *key && s.view_instance_id == view_instance_id)
        .unwrap_or(false)
    }
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn apply_page_error_state_locked(
    locked: &mut InnerState,
    key: AccountKey,
    view_instance_id: u64,
    message: &str,
  ) -> Option<ManagerSnapshot> {
    let trimmed = message.trim().to_string();
    if trimmed.is_empty() || !Self::is_view_instance_valid(locked, &key, view_instance_id) {
      return None;
    }
    if !locked
      .store
      .accounts
      .iter()
      .any(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
    {
      return None;
    }
    if locked.account_page_errors.get(&key) == Some(&trimmed) {
      return None;
    }
    locked.account_page_errors.insert(key, trimmed);
    Some(Self::snapshot_from_locked(locked))
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  async fn apply_page_error_state(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    message: String,
  ) {
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      Self::apply_page_error_state_locked(&mut locked, key, view_instance_id, &message)
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  fn apply_logged_out_state_locked(
    locked: &mut InnerState,
    key: &AccountKey,
    view_instance_id: u64,
  ) -> Option<ManagerSnapshot> {
    if !Self::is_view_instance_valid(locked, key, view_instance_id) {
      return None;
    }

    #[cfg(desktop)]
    locked.cookie_checked_at.remove(key);
    locked.cookie_identity_hints.remove(key);
    let cleared_err = locked.account_page_errors.remove(key).is_some();

    let target = locked
      .store
      .accounts
      .iter_mut()
      .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)?;

    let revert_placeholder = target.auto_alias && (target.alias == "Signed-in account" || target.alias == "已登录账号");
    let store_changed = revert_placeholder
      || !(target.auth_state == AuthState::LoggedOut
        && target.quota_detail.is_none()
        && target.quota_text.is_none()
        && target.quota_percent.is_none()
        && target.cookie_expires_at_ms.is_none()
        && !target.cookie_session_only);

    if !store_changed && !cleared_err {
      return None;
    }

    if store_changed {
      target.auth_state = AuthState::LoggedOut;
      if revert_placeholder {
        target.alias = "Account not signed in".to_string();
      }
      target.quota_detail = None;
      target.quota_text = None;
      target.quota_percent = None;
      target.cookie_expires_at_ms = None;
      target.cookie_session_only = false;

      if save_store_atomic(&locked.store_path, &locked.store).is_err() {
        return None;
      }
    }

    Some(Self::snapshot_from_locked(locked))
  }

  async fn apply_logged_out_state(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
  ) {
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      Self::apply_logged_out_state_locked(&mut locked, &key, view_instance_id)
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  #[cfg(desktop)]
  #[allow(dead_code)]
  async fn apply_cookie_expiry_summary(
    &self,
    app: &AppHandle,
    key: &AccountKey,
    summary: platform::CookieExpirySummary,
  ) {
    let checked_at = now_ms();
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      let Some(idx) = locked
        .store
        .accounts
        .iter()
        .position(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
      else {
        return;
      };
      if locked.store.accounts[idx].auth_state == AuthState::LoggedOut {
        return;
      }
      if summary.expires_at_ms.is_some() || summary.session_only {
        locked.cookie_checked_at.insert(key.clone(), checked_at);
      }
      let target = &mut locked.store.accounts[idx];
      if target.cookie_expires_at_ms == summary.expires_at_ms
        && target.cookie_session_only == summary.session_only
      {
        return;
      }
      target.cookie_expires_at_ms = summary.expires_at_ms;
      target.cookie_session_only = summary.session_only;
      if save_store_atomic(&locked.store_path, &locked.store).is_err() {
        return;
      }
      Some(Self::snapshot_from_locked(&locked))
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  #[cfg(desktop)]
  #[allow(dead_code)]
  async fn refresh_account_cookie_expiry(
    self: &Arc<Self>,
    app: &AppHandle,
    key: AccountKey,
    force: bool,
  ) {
    const COOKIE_CHECK_COOLDOWN_MS: u64 = 10 * 60 * 1000;
    let now = now_ms();

    #[cfg(not(any(target_os = "macos", windows)))]
    {
      let _ = (app, key, force, now, COOKIE_CHECK_COOLDOWN_MS);
      return;
    }

    #[cfg(any(target_os = "macos", windows))]
    {
      let url_opt = {
        let locked = self.inner.lock().await;
        let Some(account) = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        else {
          return;
        };
        if account.auth_state == AuthState::LoggedOut {
          return;
        }
        if !force {
          if let Some(&last_checked) = locked.cookie_checked_at.get(&key) {
            if now.saturating_sub(last_checked) < COOKIE_CHECK_COOLDOWN_MS {
              return;
            }
          }
        }
        let Some(spec) = resolve_provider(&key.provider_id) else {
          return;
        };
        if !spec.info.enabled {
          return;
        }
        let Ok(url) = Url::parse(&spec.info.entry_url) else {
          return;
        };

        #[cfg(target_os = "macos")]
        let platform_target = Uuid::parse_str(&key.uuid).ok().map(|uuid| (url, uuid));

        #[cfg(windows)]
        let platform_target = locked
          .warm_pool
          .get(&key)
          .and_then(|entry| app.get_webview(&entry.webview_label))
          .map(|webview| (url, webview));

        platform_target
      };

      let Some((url, handle)) = url_opt else {
        return;
      };

      #[cfg(target_os = "macos")]
      let summary_res = platform::read_macos_cookies_for_url(app, handle, url)
        .await
        .map(|(_, summary)| summary);

      #[cfg(windows)]
      let summary_res = Self::read_windows_webview_cookies(handle, url)
        .await
        .map(|(_, summary)| summary);

      if let Ok(summary) = summary_res {
        self.apply_cookie_expiry_summary(app, &key, summary).await;
      }
    }
  }

  #[allow(clippy::too_many_arguments)]
  fn apply_detected_metadata_locked(
    locked: &mut InnerState,
    key: &AccountKey,
    view_instance_id: u64,
    identity_opt: Option<String>,
    soft_identity_opt: Option<String>,
    avatar_opt: Option<String>,
    quota_text_opt: Option<String>,
    quota_pct_opt: Option<u8>,
    quota_detail_opt: Option<QuotaDetail>,
  ) -> (Option<ManagerSnapshot>, bool, bool, Option<String>) {
    if !Self::is_view_instance_valid(locked, key, view_instance_id) {
      return (None, false, false, None);
    }

    let cleared_err = locked.account_page_errors.remove(key).is_some();

    let Some(target) = locked
      .store
      .accounts
      .iter_mut()
      .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
    else {
      return (None, false, false, None);
    };

    let mut changed = false;
    let mut became_authenticated = false;
    let mut alias_updated = None;

    if target.auth_state != AuthState::Authenticated {
      target.auth_state = AuthState::Authenticated;
      became_authenticated = true;
      changed = true;
    }

    if target.auto_alias {
      if let Some(raw_id) = identity_opt {
        let cleaned = raw_id.trim();
        if !cleaned.is_empty() && cleaned.len() <= 80 {
          if target.alias != cleaned {
            target.alias = cleaned.to_string();
            alias_updated = Some(cleaned.to_string());
          }
          target.auto_alias = false;
          changed = true;
        }
      } else if let Some(raw_soft) = soft_identity_opt {
        let cleaned = raw_soft.trim();
        if !cleaned.is_empty() && cleaned.len() <= 80 {
          let should_apply_soft = target.alias == "Account not signed in" || target.alias == "待登录账号"
            || (cleaned != "Signed-in account" && cleaned != "已登录账号" && target.alias != cleaned);
          if should_apply_soft {
            target.alias = cleaned.to_string();
            alias_updated = Some(cleaned.to_string());
            changed = true;
          }
        }
      }
    }

    if let Some(av) = avatar_opt {
      if av.starts_with("data:image/") && av.len() < 80_000 && target.avatar_url.as_ref() != Some(&av) {
        target.avatar_url = Some(av);
        changed = true;
      }
    }

    if let Some(detail) = quota_detail_opt {
      if target.quota_detail.as_ref() != Some(&detail) {
        target.quota_detail = Some(detail);
        changed = true;
      }
    }

    if let Some(qt) = quota_text_opt {
      let cleaned_qt = qt.trim();
      if !cleaned_qt.is_empty() && cleaned_qt.len() <= 60 {
        let is_placeholder = cleaned_qt == "额度充足" || cleaned_qt == "Quota available" || cleaned_qt == "正在同步额度…";
        let should_overwrite = match (&target.quota_text, is_placeholder) {
          (Some(existing), true) if existing != "额度充足" && existing != "Quota available" && existing != "正在同步额度…" => false,
          (Some(existing), _) if existing == cleaned_qt && target.quota_percent == quota_pct_opt => false,
          _ => true,
        };
        if should_overwrite {
          target.quota_text = Some(cleaned_qt.to_string());
          target.quota_percent = quota_pct_opt;
          changed = true;
        }
      }
    }

    let needs_cookie_check =
      became_authenticated || (target.cookie_expires_at_ms.is_none() && !target.cookie_session_only);

    let snap_opt = if changed {
      if save_store_atomic(&locked.store_path, &locked.store).is_ok() {
        Some(Self::snapshot_from_locked(locked))
      } else {
        None
      }
    } else if cleared_err {
      Some(Self::snapshot_from_locked(locked))
    } else {
      None
    };

    let fg_title = if snap_opt.is_some() {
      alias_updated.and_then(|new_alias| {
        let is_fg = locked
          .active_slot
          .as_ref()
          .map(|s| s.committed && &s.account_key == key)
          .unwrap_or(false);
        if is_fg {
          let short_name = resolve_provider(&key.provider_id)
            .map(|s| s.info.short_name)
            .unwrap_or_else(|| key.provider_id.as_str().to_string());
          Some(format!("{short_name} · {new_alias}"))
        } else {
          None
        }
      })
    } else {
      None
    };

    (snap_opt, became_authenticated, needs_cookie_check, fg_title)
  }

  #[allow(clippy::too_many_arguments)]
  async fn apply_detected_metadata(
    &self,
    app: &AppHandle,
    key: AccountKey,
    view_instance_id: u64,
    identity_opt: Option<String>,
    soft_identity_opt: Option<String>,
    avatar_opt: Option<String>,
    quota_text_opt: Option<String>,
    quota_pct_opt: Option<u8>,
    quota_detail_opt: Option<QuotaDetail>,
  ) -> (bool, bool) {
    let (snap_opt, became_authenticated, needs_cookie_check, fg_title) = {
      let mut locked = self.inner.lock().await;
      Self::apply_detected_metadata_locked(
        &mut locked,
        &key,
        view_instance_id,
        identity_opt,
        soft_identity_opt,
        avatar_opt,
        quota_text_opt,
        quota_pct_opt,
        quota_detail_opt,
      )
    };

    if let Some(title) = fg_title {
      #[cfg(desktop)]
      if let Some(main_win) = app.get_window("main") {
        let _ = main_win.set_title(&title);
      }
      #[cfg(mobile)]
      {
        let seq_opt = {
          let locked = self.inner.lock().await;
          locked.active_slot.as_ref().map(|s| s.switch_seq)
        };
        if let Some(seq) = seq_opt {
          let _ = crate::platform::mobile::update_account_title(app, seq, &title).await;
        }
      }
    }

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }

    (became_authenticated, needs_cookie_check)
  }

  #[cfg(any(desktop, test))]
  fn resolve_member_alias_locked(locked: &InnerState, key: &AccountKey) -> Option<String> {
    if key.provider_id.is_external() {
      locked
        .store
        .external_agents
        .iter()
        .find(|e| e.account_key() == *key)
        .map(|e| e.name.clone())
    } else {
      locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        .map(|a| a.alias.clone())
    }
  }

  fn is_account_group_eligible(acc: &AccountRecord, now: u64) -> bool {
    resolve_provider(&acc.provider_id).is_some_and(|s| s.info.enabled)
      && acc.auth_state == AuthState::Authenticated
      && acc.cookie_expires_at_ms.is_none_or(|exp| exp > now)
  }

  fn normalize_group_members(
    locked: &InnerState,
    raw_members: Vec<AccountKey>,
    allowed_existing: Option<&[AccountKey]>,
  ) -> Result<Vec<AccountKey>, String> {
    let now = now_ms();
    let mut members = Vec::new();
    for raw in raw_members {
      let Some(uuid) = normalize_uuid(&raw.uuid) else {
        continue;
      };
      let key = AccountKey {
        provider_id: raw.provider_id,
        uuid,
      };
      if members.contains(&key) {
        continue;
      }
      if key.provider_id.is_external() {
        if !locked
          .store
          .external_agents
          .iter()
          .any(|e| e.account_key() == key)
        {
          return Err("所选 API/CLI 成员不存在或已被移除".to_string());
        }
        members.push(key);
        continue;
      }
      let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
      else {
        return Err("所选账号不存在或已被移除".to_string());
      };
      if acc.auth_state == AuthState::LoggedOut {
        return Err(format!("账号「{}」已退出登录，不能加入群聊", acc.alias));
      }
      let already_member = allowed_existing.is_some_and(|ex| ex.contains(&key));
      if !already_member && !Self::is_account_group_eligible(acc, now) {
        return Err(format!(
          "账号「{}」尚未完成有效登录（或登录态已过期）",
          acc.alias
        ));
      }
      members.push(key);
    }

    if members.is_empty() {
      return Err("请至少选择 1 个可用成员".to_string());
    }
    if members.len() > MAX_GROUP_MEMBERS {
      return Err(format!("单个群聊最多支持 {} 个成员", MAX_GROUP_MEMBERS));
    }
    Ok(members)
  }

  fn set_group_window_title(app: &AppHandle, group_name: Option<&str>) {
    #[cfg(desktop)]
    if let Some(main_win) = app.get_window("main") {
      let title = match group_name {
        Some(name) if !name.trim().is_empty() => format!("{APP_TITLE} · 群聊 · {}", name.trim()),
        _ => format!("{APP_TITLE} · 群聊"),
      };
      let _ = main_win.set_title(&title);
    }
    #[cfg(not(desktop))]
    let _ = (app, group_name);
  }

  fn starts_with_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let n_chars: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    if n_chars.is_empty() {
      return None;
    }
    let mut matched_idx = 0usize;
    for (byte_idx, ch) in haystack.char_indices() {
      for lc in ch.to_lowercase() {
        if matched_idx >= n_chars.len() || lc != n_chars[matched_idx] {
          return None;
        }
        matched_idx += 1;
      }
      if matched_idx == n_chars.len() {
        return Some(byte_idx + ch.len_utf8());
      }
    }
    None
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn parse_at_mentions(
    content: &str,
    members: &[(AccountKey, String)],
  ) -> (bool, Vec<AccountKey>) {
    let mut sorted_aliases: Vec<(&AccountKey, &str)> = members
      .iter()
      .filter_map(|(k, alias)| {
        let trimmed = alias.trim();
        if trimmed.is_empty() {
          None
        } else {
          Some((k, trimmed))
        }
      })
      .collect();
    sorted_aliases.sort_by_key(|(_, alias)| std::cmp::Reverse(alias.chars().count()));

    let mut mentioned = Vec::new();
    for (idx, ch) in content.char_indices() {
      if ch != '@' {
        continue;
      }
      if idx > 0
        && content[..idx]
          .chars()
          .next_back()
          .is_some_and(|prev| prev.is_ascii_alphanumeric())
      {
        continue;
      }
      let after = &content[idx + 1..];
      if after.starts_with("全体成员")
        || after.starts_with("全体")
        || after.starts_with("所有人")
      {
        return (true, Vec::new());
      }
      if let Some(end) = Self::starts_with_ignore_case(after, "all") {
        if !after[end..]
          .chars()
          .next()
          .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_' || next == '-')
        {
          return (true, Vec::new());
        }
      }
      for (key, alias) in &sorted_aliases {
        if let Some(end) = Self::starts_with_ignore_case(after, alias) {
          let needs_ascii_boundary = alias
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphanumeric());
          if needs_ascii_boundary
            && after[end..]
              .chars()
              .next()
              .is_some_and(|next| next.is_ascii_alphanumeric())
          {
            continue;
          }
          if !mentioned.contains(*key) {
            mentioned.push((*key).clone());
          }
          break;
        }
      }
    }
    (false, mentioned)
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn strip_routing_mentions(content: &str, members: &[(AccountKey, String)]) -> String {
    let mut sorted_aliases: Vec<&str> = members
      .iter()
      .map(|(_, alias)| alias.trim())
      .filter(|a| !a.is_empty())
      .collect();
    sorted_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.chars().count()));

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    while cursor < content.len() {
      let rest = &content[cursor..];
      let Some(ch) = rest.chars().next() else {
        break;
      };
      if ch != '@' {
        out.push(ch);
        cursor += ch.len_utf8();
        continue;
      }
      let preceded_by_alnum = cursor > 0
        && content[..cursor]
          .chars()
          .next_back()
          .is_some_and(|prev| prev.is_ascii_alphanumeric());
      if preceded_by_alnum {
        out.push(ch);
        cursor += ch.len_utf8();
        continue;
      }

      let after = &content[cursor + 1..];
      let mut matched_len: Option<usize> = None;
      for kw in ["全体成员", "所有人", "全体"] {
        if after.starts_with(kw) {
          matched_len = Some(kw.len());
          break;
        }
      }
      if matched_len.is_none() {
        if let Some(end) = Self::starts_with_ignore_case(after, "all") {
          if !after[end..]
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_' || next == '-')
          {
            matched_len = Some(end);
          }
        }
      }
      if matched_len.is_none() {
        for alias in &sorted_aliases {
          if let Some(end) = Self::starts_with_ignore_case(after, alias) {
            let needs_ascii_boundary = alias
              .chars()
              .last()
              .is_some_and(|c| c.is_ascii_alphanumeric());
            if needs_ascii_boundary
              && after[end..]
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_alphanumeric())
            {
              continue;
            }
            matched_len = Some(end);
            break;
          }
        }
      }

      if let Some(m_len) = matched_len {
        cursor += 1 + m_len;
        while let Some(sep) = content[cursor..].chars().next() {
          if sep == ' ' || sep == '\t' || sep == '、' {
            cursor += sep.len_utf8();
          } else {
            break;
          }
        }
      } else {
        out.push(ch);
        cursor += ch.len_utf8();
      }
    }

    let cleaned = out
      .trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '，' | ',' | '、' | '：' | ':')
      })
      .trim()
      .to_string();
    if cleaned.is_empty() {
      content.trim().to_string()
    } else {
      cleaned
    }
  }

  #[cfg(desktop)]
  fn compute_group_knowledge_fingerprint(group: &GroupChatRecord, member: &AccountKey) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    group.shared_context.hash(&mut hasher);
    for skill in &group.shared_skills {
      if skill.enabled {
        skill.id.hash(&mut hasher);
        skill.name.hash(&mut hasher);
        skill.content.hash(&mut hasher);
      }
    }
    if let Some(role_entry) = group.member_roles.iter().find(|r| &r.member == member) {
      role_entry.role.hash(&mut hasher);
    }
    hasher.finish()
  }

  pub async fn select_group_chat(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: Option<String>,
  ) -> Result<ManagerSnapshot, String> {
    let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let (snap, group_name) = {
      let mut locked = self.inner.lock().await;
      let cleaned_id = group_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
      let resolved = match cleaned_id {
        Some(ref gid) => {
          let found = locked
            .store
            .group_chats
            .iter()
            .find(|g| &g.id == gid)
            .cloned()
            .ok_or_else(|| "未找到对应的群聊".to_string())?;
          Some(found)
        }
        None => locked.store.group_chats.first().cloned(),
      };

      self.hide_foreground_slot(app, next_seq, &mut locked).await;
      locked.notice = None;
      locked.active_group_id = resolved.as_ref().map(|g| g.id.clone());
      #[cfg(desktop)]
      Self::apply_desktop_layout_locked(app, &locked);
      (Self::snapshot_from_locked(&locked), resolved.map(|g| g.name))
    };

    Self::set_group_window_title(app, group_name.as_deref());
    Ok(snap)
  }

  pub async fn create_group_chat(
    self: &Arc<Self>,
    app: &AppHandle,
    name: String,
    members: Vec<AccountKey>,
  ) -> Result<ManagerSnapshot, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
      return Err("群聊名称不能为空".to_string());
    }
    if clean_name.chars().count() > 32 {
      return Err("群聊名称不能超过 32 个字".to_string());
    }

    let next_seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let (snap, final_name) = {
      let mut locked = self.inner.lock().await;
      let valid_members = Self::normalize_group_members(&locked, members, None)?;
      let record = GroupChatRecord {
        id: Uuid::new_v4().hyphenated().to_string(),
        name: clean_name.to_string(),
        members: valid_members,
        created_at_ms: now_ms(),
        messages: Vec::new(),
        shared_context: String::new(),
        shared_skills: Vec::new(),
        member_roles: Vec::new(),
        workflow_mode: GroupWorkflowMode::Parallel,
        max_relay_hops: 2,
        thread_bindings: Vec::new(),
      };
      let new_id = record.id.clone();
      locked.store.group_chats.push(record);
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.group_chats.pop();
        return Err(err);
      }

      self.hide_foreground_slot(app, next_seq, &mut locked).await;
      locked.active_group_id = Some(new_id);
      locked.notice = None;
      #[cfg(desktop)]
      Self::apply_desktop_layout_locked(app, &locked);
      (Self::snapshot_from_locked(&locked), clean_name.to_string())
    };

    Self::set_group_window_title(app, Some(&final_name));
    Ok(snap)
  }

  pub async fn update_group_chat(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: String,
    name: String,
    members: Vec<AccountKey>,
  ) -> Result<ManagerSnapshot, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
      return Err("群聊名称不能为空".to_string());
    }
    if clean_name.chars().count() > 32 {
      return Err("群聊名称不能超过 32 个字".to_string());
    }

    let (snap, active_name) = {
      let mut locked = self.inner.lock().await;
      let idx = locked
        .store
        .group_chats
        .iter()
        .position(|g| g.id == group_id)
        .ok_or_else(|| "未找到对应的群聊".to_string())?;
      let existing_members = locked.store.group_chats[idx].members.clone();
      let valid_members =
        Self::normalize_group_members(&locked, members, Some(&existing_members))?;

      let prev_name = locked.store.group_chats[idx].name.clone();
      let prev_roles = locked.store.group_chats[idx].member_roles.clone();
      let prev_bindings = locked.store.group_chats[idx].thread_bindings.clone();
      locked.store.group_chats[idx].name = clean_name.to_string();
      locked.store.group_chats[idx].members = valid_members.clone();
      locked.store.group_chats[idx]
        .member_roles
        .retain(|mr| valid_members.contains(&mr.member));
      locked.store.group_chats[idx]
        .thread_bindings
        .retain(|tb| valid_members.contains(&tb.member));
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.group_chats[idx].name = prev_name;
        locked.store.group_chats[idx].members = existing_members;
        locked.store.group_chats[idx].member_roles = prev_roles;
        locked.store.group_chats[idx].thread_bindings = prev_bindings;
        return Err(err);
      }

      #[cfg(desktop)]
      locked
        .group_context_synced
        .retain(|(gid, k), _| gid != &group_id || valid_members.contains(k));

      let is_active = locked.active_group_id.as_deref() == Some(group_id.as_str());
      (
        Self::snapshot_from_locked(&locked),
        if is_active {
          Some(clean_name.to_string())
        } else {
          None
        },
      )
    };

    if let Some(active_name) = active_name {
      Self::set_group_window_title(app, Some(&active_name));
    }
    Ok(snap)
  }

  pub async fn update_group_knowledge(
    &self,
    group_id: String,
    shared_context: String,
    shared_skills: Vec<GroupSharedSkill>,
    member_roles: Vec<GroupMemberRole>,
  ) -> Result<ManagerSnapshot, String> {
    let clean_context: String = shared_context.trim().chars().take(8000).collect();

    let mut clean_skills = Vec::new();
    let mut seen_ids = HashSet::new();
    for sk in shared_skills {
      let name: String = sk.name.trim().chars().take(40).collect();
      let content: String = sk.content.trim().chars().take(2000).collect();
      if name.is_empty() || content.is_empty() {
        continue;
      }
      if clean_skills.len() >= 30 {
        return Err("单个群聊最多支持 30 个共享技能".to_string());
      }
      let id = if sk.id.trim().is_empty() || seen_ids.contains(sk.id.trim()) {
        Uuid::new_v4().hyphenated().to_string()
      } else {
        sk.id.trim().to_string()
      };
      seen_ids.insert(id.clone());
      clean_skills.push(GroupSharedSkill {
        id,
        name,
        description: sk.description.trim().chars().take(120).collect(),
        content,
        enabled: sk.enabled,
      });
    }

    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;
    let members = locked.store.group_chats[idx].members.clone();

    let mut clean_roles: Vec<GroupMemberRole> = Vec::new();
    for mr in member_roles {
      let Some(uuid) = normalize_uuid(&mr.member.uuid) else {
        continue;
      };
      let key = AccountKey {
        provider_id: mr.member.provider_id,
        uuid,
      };
      let role_str: String = mr.role.trim().chars().take(120).collect();
      if role_str.is_empty()
        || !members.contains(&key)
        || clean_roles.iter().any(|existing| existing.member == key)
      {
        continue;
      }
      clean_roles.push(GroupMemberRole {
        member: key,
        role: role_str,
      });
    }

    let prev_ctx = locked.store.group_chats[idx].shared_context.clone();
    let prev_skills = locked.store.group_chats[idx].shared_skills.clone();
    let prev_roles = locked.store.group_chats[idx].member_roles.clone();
    locked.store.group_chats[idx].shared_context = clean_context;
    locked.store.group_chats[idx].shared_skills = clean_skills;
    locked.store.group_chats[idx].member_roles = clean_roles;
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.group_chats[idx].shared_context = prev_ctx;
      locked.store.group_chats[idx].shared_skills = prev_skills;
      locked.store.group_chats[idx].member_roles = prev_roles;
      return Err(err);
    }

    #[cfg(desktop)]
    locked
      .group_context_synced
      .retain(|(gid, _), _| gid != &group_id);

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn append_group_shared_context(
    &self,
    group_id: String,
    snippet: String,
  ) -> Result<ManagerSnapshot, String> {
    let clean_snippet = snippet.trim();
    if clean_snippet.is_empty() {
      return Err("沉淀内容不能为空".to_string());
    }

    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;

    let prev = locked.store.group_chats[idx].shared_context.clone();
    let combined = if prev.trim().is_empty() {
      clean_snippet.to_string()
    } else {
      format!("{}\n\n{}", prev.trim(), clean_snippet)
    };
    let final_ctx: String = if combined.chars().count() > 8000 {
      let skip = combined.chars().count() - 8000;
      combined.chars().skip(skip).collect()
    } else {
      combined
    };

    locked.store.group_chats[idx].shared_context = final_ctx;
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.group_chats[idx].shared_context = prev;
      return Err(err);
    }

    #[cfg(desktop)]
    locked
      .group_context_synced
      .retain(|(gid, _), _| gid != &group_id);

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn delete_group_chat(
    &self,
    app: &AppHandle,
    group_id: String,
  ) -> Result<ManagerSnapshot, String> {
    let (snap, next_title, is_group_view) = {
      let mut locked = self.inner.lock().await;
      let idx = locked
        .store
        .group_chats
        .iter()
        .position(|g| g.id == group_id)
        .ok_or_else(|| "未找到对应的群聊".to_string())?;
      let removed = locked.store.group_chats.remove(idx);
      if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
        locked.store.group_chats.insert(idx, removed);
        return Err(err);
      }
      #[cfg(desktop)]
      locked
        .group_context_synced
        .retain(|(gid, _), _| gid != &group_id);
      let was_active = locked.active_group_id.as_deref() == Some(group_id.as_str());
      if was_active {
        locked.active_group_id = locked.store.group_chats.first().map(|g| g.id.clone());
      }
      let next_name = locked
        .active_group_id
        .as_ref()
        .and_then(|gid| locked.store.group_chats.iter().find(|g| &g.id == gid))
        .map(|g| g.name.clone());
      (Self::snapshot_from_locked(&locked), next_name, was_active)
    };

    if is_group_view {
      Self::set_group_window_title(app, next_title.as_deref());
    }
    Ok(snap)
  }

  #[allow(clippy::too_many_arguments)]
  pub async fn upsert_external_agent(
    &self,
    id: Option<String>,
    kind: ExternalAgentKind,
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    command: String,
    args: Vec<String>,
    workdir: Option<String>,
  ) -> Result<ManagerSnapshot, String> {
    let mut locked = self.inner.lock().await;
    let existing_pos = id
      .as_deref()
      .and_then(normalize_uuid)
      .and_then(|norm_id| locked.store.external_agents.iter().position(|e| e.id == norm_id));
    let created_at = existing_pos.map(|pos| locked.store.external_agents[pos].created_at_ms);

    let record = group_transport::normalize_external_agent_input(
      id.as_deref(),
      kind,
      &name,
      &base_url,
      &api_key,
      &model,
      &command,
      args,
      workdir.as_deref(),
      created_at,
    )?;

    let prev_agents = locked.store.external_agents.clone();
    let prev_groups = locked.store.group_chats.clone();
    if let Some(pos) = existing_pos {
      let old_key = locked.store.external_agents[pos].account_key();
      let new_key = record.account_key();
      locked.store.external_agents[pos] = record;
      if old_key != new_key {
        for group in &mut locked.store.group_chats {
          for m in &mut group.members {
            if *m == old_key {
              *m = new_key.clone();
            }
          }
          for mr in &mut group.member_roles {
            if mr.member == old_key {
              mr.member = new_key.clone();
            }
          }
          for tb in &mut group.thread_bindings {
            if tb.member == old_key {
              tb.member = new_key.clone();
            }
          }
        }
      }
    } else {
      locked.store.external_agents.push(record);
    }

    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.external_agents = prev_agents;
      locked.store.group_chats = prev_groups;
      return Err(err);
    }

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn delete_external_agent(&self, id: String) -> Result<ManagerSnapshot, String> {
    let norm_id = normalize_uuid(&id).ok_or_else(|| "外部成员 ID 格式非法".to_string())?;
    let mut locked = self.inner.lock().await;
    let pos = locked
      .store
      .external_agents
      .iter()
      .position(|e| e.id == norm_id)
      .ok_or_else(|| "未找到对应的外部成员".to_string())?;
    let removed_key = locked.store.external_agents[pos].account_key();
    let prev_agents = locked.store.external_agents.clone();
    let prev_groups = locked.store.group_chats.clone();

    locked.store.external_agents.remove(pos);
    for group in &mut locked.store.group_chats {
      group.members.retain(|m| m != &removed_key);
      group.member_roles.retain(|mr| mr.member != removed_key);
      group.thread_bindings.retain(|tb| tb.member != removed_key);
    }

    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.external_agents = prev_agents;
      locked.store.group_chats = prev_groups;
      return Err(err);
    }

    #[cfg(desktop)]
    locked
      .group_context_synced
      .retain(|(_, k), _| k != &removed_key);

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn update_group_workflow(
    &self,
    group_id: String,
    workflow_mode: GroupWorkflowMode,
    max_relay_hops: u8,
  ) -> Result<ManagerSnapshot, String> {
    let clamped_hops = max_relay_hops.clamp(1, 3);
    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;
    let prev_mode = locked.store.group_chats[idx].workflow_mode;
    let prev_hops = locked.store.group_chats[idx].max_relay_hops;
    locked.store.group_chats[idx].workflow_mode = workflow_mode;
    locked.store.group_chats[idx].max_relay_hops = clamped_hops;

    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.group_chats[idx].workflow_mode = prev_mode;
      locked.store.group_chats[idx].max_relay_hops = prev_hops;
      return Err(err);
    }

    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn reset_group_thread_binding(
    &self,
    group_id: String,
    member: AccountKey,
  ) -> Result<ManagerSnapshot, String> {
    let norm_uuid = normalize_uuid(&member.uuid).ok_or_else(|| "成员标识格式非法".to_string())?;
    let target_key = AccountKey {
      provider_id: member.provider_id,
      uuid: norm_uuid,
    };
    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;
    let prev_bindings = locked.store.group_chats[idx].thread_bindings.clone();
    locked.store.group_chats[idx]
      .thread_bindings
      .retain(|tb| tb.member != target_key);
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.group_chats[idx].thread_bindings = prev_bindings;
      return Err(err);
    }
    #[cfg(desktop)]
    locked
      .group_context_synced
      .remove(&(group_id, target_key));
    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn get_site_adapters(&self) -> SiteAdaptersSnapshot {
    let app_data_dir = {
      let locked = self.inner.lock().await;
      locked.app_data_dir.clone()
    };
    site_adapters::get_adapters_snapshot(&app_data_dir)
  }

  pub async fn save_site_adapter_overrides(
    &self,
    raw_json: String,
  ) -> Result<SiteAdaptersSnapshot, String> {
    let app_data_dir = {
      let locked = self.inner.lock().await;
      locked.app_data_dir.clone()
    };
    site_adapters::validate_and_save_overrides(&app_data_dir, &raw_json)
  }

  pub async fn reset_site_adapter_overrides(&self) -> Result<SiteAdaptersSnapshot, String> {
    let app_data_dir = {
      let locked = self.inner.lock().await;
      locked.app_data_dir.clone()
    };
    site_adapters::reset_overrides(&app_data_dir)
  }

  pub async fn clear_group_messages(
    &self,
    group_id: String,
  ) -> Result<ManagerSnapshot, String> {
    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;
    let prev = std::mem::take(&mut locked.store.group_chats[idx].messages);
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      locked.store.group_chats[idx].messages = prev;
      return Err(err);
    }
    #[cfg(desktop)]
    locked
      .group_context_synced
      .retain(|(gid, _), _| gid != &group_id);
    Ok(Self::snapshot_from_locked(&locked))
  }

  pub async fn start_group_new_topic(
    &self,
    group_id: String,
    keep_shared_context: bool,
  ) -> Result<ManagerSnapshot, String> {
    let mut locked = self.inner.lock().await;
    let idx = locked
      .store
      .group_chats
      .iter()
      .position(|g| g.id == group_id)
      .ok_or_else(|| "未找到对应的群聊".to_string())?;
    if locked.store.group_chats[idx].messages.iter().any(|m| {
      matches!(
        m.status,
        GroupReplyStatus::Queued | GroupReplyStatus::Sending | GroupReplyStatus::Streaming
      )
    }) {
      return Err("有成员正在回复，请等待完成后再开启新话题".to_string());
    }
    let prev_msgs = std::mem::take(&mut locked.store.group_chats[idx].messages);
    let prev_bindings = std::mem::take(&mut locked.store.group_chats[idx].thread_bindings);
    let prev_ctx = locked.store.group_chats[idx].shared_context.clone();
    if !keep_shared_context {
      locked.store.group_chats[idx].shared_context.clear();
    }
    if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
      let g = &mut locked.store.group_chats[idx];
      g.messages = prev_msgs;
      g.thread_bindings = prev_bindings;
      g.shared_context = prev_ctx;
      return Err(err);
    }
    #[cfg(desktop)]
    locked
      .group_context_synced
      .retain(|(gid, _), _| gid != &group_id);
    locked
      .group_channel_last_account
      .retain(|(gid, _), _| gid != &group_id);
    Ok(Self::snapshot_from_locked(&locked))
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn collect_group_eligible_members_locked(
    locked: &InnerState,
    g_idx: usize,
  ) -> Vec<(AccountKey, String)> {
    let mut eligible_members = Vec::new();
    for member in &locked.store.group_chats[g_idx].members {
      if member.provider_id.is_external() {
        if let Some(ext) = locked
          .store
          .external_agents
          .iter()
          .find(|e| &e.account_key() == member)
        {
          eligible_members.push((member.clone(), ext.name.clone()));
        }
      } else if let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == member.provider_id && a.uuid == member.uuid)
      {
        if acc.auth_state != AuthState::LoggedOut
          && resolve_provider(&acc.provider_id).is_some_and(|s| s.info.enabled)
          && !locked.account_page_errors.contains_key(member)
        {
          eligible_members.push((member.clone(), acc.alias.clone()));
        }
      }
    }
    eligible_members
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn channel_display_label_locked(_locked: &InnerState, channel_pid: &ProviderId) -> String {
    if channel_pid.is_api() {
      return "OpenAI API".to_string();
    }
    if channel_pid.is_cli() {
      return "本地 CLI".to_string();
    }
    resolve_provider(channel_pid)
      .map(|s| s.info.short_name)
      .unwrap_or_else(|| channel_pid.as_str().to_string())
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn select_auto_channel_account_locked(
    locked: &InnerState,
    group_id: &str,
    channel_pid: &ProviderId,
    eligible_members: &[(AccountKey, String)],
    excluded: &[AccountKey],
    now: u64,
  ) -> Result<(AccountKey, String), String> {
    let channel_label = Self::channel_display_label_locked(locked, channel_pid);
    let channel_members: Vec<(AccountKey, String)> = eligible_members
      .iter()
      .filter(|(k, _)| &k.provider_id == channel_pid && !excluded.contains(k))
      .cloned()
      .collect();

    if channel_members.is_empty() {
      return Err(format!(
        "渠道「{channel_label}」下暂无可用且未登录失效的成员"
      ));
    }

    let group_messages = locked
      .store
      .group_chats
      .iter()
      .find(|g| g.id == group_id)
      .map(|g| g.messages.as_slice())
      .unwrap_or(&[]);

    #[derive(Clone)]
    struct Candidate {
      key: AccountKey,
      alias: String,
      quota: AccountQuotaEval,
      is_busy: bool,
      in_cooldown: bool,
      last_assigned: u64,
    }

    let mut usable: Vec<Candidate> = Vec::new();
    for (key, alias) in channel_members {
      let latest_msg = group_messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.sender.as_ref() == Some(&key));
      if latest_msg.is_some_and(|m| m.status == GroupReplyStatus::NeedsAuth) {
        continue;
      }

      let quota = if key.provider_id.is_external() {
        AccountQuotaEval {
          depleted: false,
          sticky_ok: true,
          tier: 3,
          effective_used_pct: 50,
        }
      } else if let Some(acc) = locked
        .store
        .accounts
        .iter()
        .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
      {
        evaluate_account_quota(acc)
      } else {
        continue;
      };

      if quota.depleted {
        continue;
      }

      #[cfg(desktop)]
      let in_flight = locked
        .in_flight_group_members
        .get(&key)
        .copied()
        .unwrap_or(0)
        > 0;
      #[cfg(not(desktop))]
      let in_flight = false;

      let msg_busy = group_messages.iter().any(|m| {
        m.sender.as_ref() == Some(&key)
          && matches!(
            m.status,
            GroupReplyStatus::Queued | GroupReplyStatus::Sending | GroupReplyStatus::Streaming
          )
      });
      let is_busy = in_flight || msg_busy;
      let in_cooldown = locked
        .account_cooldown_until_ms
        .get(&key)
        .is_some_and(|&until| until > now);
      let last_assigned = locked
        .account_last_assigned_ms
        .get(&key)
        .copied()
        .unwrap_or(0);

      usable.push(Candidate {
        key,
        alias,
        quota,
        is_busy,
        in_cooldown,
        last_assigned,
      });
    }

    if usable.is_empty() {
      return Err(format!(
        "渠道「{channel_label}」下所有账号额度已耗尽或待授权，请切换其他渠道或处理授权"
      ));
    }

    if let Some(last_key) = locked
      .group_channel_last_account
      .get(&(group_id.to_string(), channel_pid.clone()))
    {
      if let Some(sticky) = usable.iter().find(|c| &c.key == last_key) {
        if !sticky.is_busy && !sticky.in_cooldown && sticky.quota.sticky_ok {
          return Ok((sticky.key.clone(), sticky.alias.clone()));
        }
      }
    }

    usable.sort_by_key(|c| {
      (
        c.in_cooldown,
        c.is_busy,
        c.quota.tier,
        c.quota.effective_used_pct,
        c.last_assigned,
      )
    });
    let best = &usable[0];
    Ok((best.key.clone(), best.alias.clone()))
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn resolve_group_dispatch_targets_locked(
    locked: &mut InnerState,
    group_id: &str,
    trimmed_content: &str,
    eligible_members: &[(AccountKey, String)],
    target_members: Option<Vec<AccountKey>>,
    auto_channels: Option<Vec<String>>,
    now: u64,
  ) -> Result<Vec<(AccountKey, String, Option<ProviderId>)>, String> {
    let (mention_all, mentioned_keys) = Self::parse_at_mentions(trimmed_content, eligible_members);
    if mention_all {
      return Ok(
        eligible_members
          .iter()
          .map(|(k, a)| (k.clone(), a.clone(), None))
          .collect(),
      );
    }

    let requested_keys: Vec<AccountKey> = target_members
      .unwrap_or_default()
      .into_iter()
      .filter_map(|k| {
        normalize_uuid(&k.uuid).map(|uuid| AccountKey {
          provider_id: k.provider_id,
          uuid,
        })
      })
      .collect();

    let has_inline_mentions = !mentioned_keys.is_empty();
    let explicit_targets: Vec<(AccountKey, String)> = if has_inline_mentions {
      mentioned_keys
        .into_iter()
        .filter_map(|mk| {
          eligible_members
            .iter()
            .find(|(ek, _)| ek == &mk)
            .cloned()
        })
        .collect()
    } else if !requested_keys.is_empty() {
      let mut seen = Vec::new();
      requested_keys
        .iter()
        .filter_map(|rk| {
          if seen.contains(&rk) {
            return None;
          }
          let found = eligible_members.iter().find(|(ek, _)| ek == rk).cloned()?;
          seen.push(rk);
          Some(found)
        })
        .collect()
    } else {
      Vec::new()
    };

    if has_inline_mentions && explicit_targets.is_empty() {
      return Err("指定的 @成员当前未登录或不可用，请重新选择".to_string());
    }

    let mut parsed_channels: Vec<ProviderId> = Vec::new();
    for raw_ch in auto_channels.unwrap_or_default() {
      if let Some(pid) = ProviderId::parse(&raw_ch) {
        if !parsed_channels.contains(&pid) {
          parsed_channels.push(pid);
        }
      }
    }

    let explicit_providers: std::collections::HashSet<ProviderId> = explicit_targets
      .iter()
      .map(|(k, _)| k.provider_id.clone())
      .collect();

    let mut auto_targets: Vec<(AccountKey, String, Option<ProviderId>)> = Vec::new();
    let mut excluded: Vec<AccountKey> = explicit_targets.iter().map(|(k, _)| k.clone()).collect();
    for channel_pid in parsed_channels {
      if explicit_providers.contains(&channel_pid) {
        continue;
      }
      let (picked_key, picked_alias) = Self::select_auto_channel_account_locked(
        locked,
        group_id,
        &channel_pid,
        eligible_members,
        &excluded,
        now,
      )?;
      locked
        .account_last_assigned_ms
        .insert(picked_key.clone(), now);
      excluded.push(picked_key.clone());
      auto_targets.push((picked_key, picked_alias, Some(channel_pid)));
    }

    if explicit_targets.is_empty() && auto_targets.is_empty() {
      if !requested_keys.is_empty() {
        return Err("指定的 @成员当前未登录或不可用，请重新选择".to_string());
      }
      return Ok(
        eligible_members
          .iter()
          .map(|(k, a)| (k.clone(), a.clone(), None))
          .collect(),
      );
    }

    let mut combined: Vec<(AccountKey, String, Option<ProviderId>)> = explicit_targets
      .into_iter()
      .map(|(k, a)| (k, a, None))
      .collect();
    combined.extend(auto_targets);
    Ok(combined)
  }

  pub async fn send_group_message(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: String,
    content: String,
    target_members: Option<Vec<AccountKey>>,
    auto_channels: Option<Vec<String>>,
  ) -> Result<ManagerSnapshot, String> {
    #[cfg(not(desktop))]
    {
      let _ = (app, group_id, content, target_members, auto_channels);
      Err("群聊功能暂仅支持桌面端".to_string())
    }

    #[cfg(desktop)]
    {
      let trimmed = content.trim().to_string();
      if trimmed.is_empty() {
        return Err("发送内容不能为空".to_string());
      }
      if trimmed.chars().count() > 4000 {
        return Err("单条群聊消息不能超过 4000 字".to_string());
      }

      let (snap, user_msg_id, workflow_mode, dispatch_jobs) = {
        let mut locked = self.inner.lock().await;
        if !locked.conflicts.is_empty() {
          return Err("存在 UUID 归属冲突，请先处理冲突后再发消息".to_string());
        }
        let g_idx = locked
          .store
          .group_chats
          .iter()
          .position(|g| g.id == group_id)
          .ok_or_else(|| "未找到对应的群聊".to_string())?;

        let eligible_members = Self::collect_group_eligible_members_locked(&locked, g_idx);
        if eligible_members.is_empty() {
          return Err("没有可接收消息的可用成员（请检查成员登录或配置状态）".to_string());
        }

        let ts = now_ms();
        let target_accounts = Self::resolve_group_dispatch_targets_locked(
          &mut locked,
          &group_id,
          &trimmed,
          &eligible_members,
          target_members,
          auto_channels,
          ts,
        )?;

        let workflow_mode = locked.store.group_chats[g_idx].workflow_mode;
        let user_id = Uuid::new_v4().hyphenated().to_string();
        let target_keys: Vec<AccountKey> =
          target_accounts.iter().map(|(k, _, _)| k.clone()).collect();
        locked.store.group_chats[g_idx].messages.push(GroupChatMessage {
          id: user_id.clone(),
          role: "user".to_string(),
          sender: None,
          sender_name: "Me".to_string(),
          content: trimmed.clone(),
          created_at_ms: ts,
          status: GroupReplyStatus::Done,
          targets: target_keys,
          error: None,
          stage_label: None,
          hop_depth: 0,
          auto_routed_channel: None,
        });

        let total_targets = target_accounts.len();
        let mut jobs = Vec::with_capacity(total_targets);
        for (offset, (member_key, alias, auto_ch)) in target_accounts.into_iter().enumerate() {
          let stage_label = if workflow_mode == GroupWorkflowMode::Review && total_targets >= 2 {
            if offset == 0 {
              Some("Initial draft".to_string())
            } else if offset + 1 == total_targets && total_targets >= 3 {
              Some("Final draft".to_string())
            } else {
              Some("Cross-review".to_string())
            }
          } else {
            None
          };
          let reply_msg_id = Uuid::new_v4().hyphenated().to_string();
          locked.store.group_chats[g_idx].messages.push(GroupChatMessage {
            id: reply_msg_id.clone(),
            role: "assistant".to_string(),
            sender: Some(member_key.clone()),
            sender_name: alias,
            content: String::new(),
            created_at_ms: ts + (offset as u64) + 1,
            status: GroupReplyStatus::Queued,
            targets: Vec::new(),
            error: None,
            stage_label: stage_label.clone(),
            hop_depth: 0,
            auto_routed_channel: auto_ch.as_ref().map(|p| p.as_str().to_string()),
          });
          jobs.push((offset, member_key, reply_msg_id, stage_label, auto_ch));
        }

        let msgs = &mut locked.store.group_chats[g_idx].messages;
        if msgs.len() > 120 {
          let excess = msgs.len() - 120;
          msgs.drain(..excess);
        }

        save_store_atomic(&locked.store_path, &locked.store)?;
        (
          Self::snapshot_from_locked(&locked),
          user_id,
          workflow_mode,
          jobs,
        )
      };

      self.emit_snapshot(app, &snap);

      match workflow_mode {
        GroupWorkflowMode::Review if dispatch_jobs.len() >= 2 => {
          let mgr = Arc::clone(self);
          let app_handle = app.clone();
          let gid = group_id.clone();
          let uid = user_msg_id.clone();
          let raw_content = trimmed.clone();
          tauri::async_runtime::spawn(async move {
            mgr
              .run_group_review_pipeline(app_handle, gid, uid, raw_content, dispatch_jobs)
              .await;
          });
        }
        GroupWorkflowMode::A2aRelay => {
          let initial_visited: Vec<AccountKey> =
            dispatch_jobs.iter().map(|(_, k, _, _, _)| k.clone()).collect();
          for (order_idx, member_key, msg_id, _, _) in dispatch_jobs {
            let mgr = Arc::clone(self);
            let app_handle = app.clone();
            let gid = group_id.clone();
            let uid = user_msg_id.clone();
            let raw_content = trimmed.clone();
            let visited = initial_visited.clone();
            tauri::async_runtime::spawn(async move {
              if order_idx > 0 {
                tokio::time::sleep(std::time::Duration::from_millis((order_idx as u64) * 40))
                  .await;
              }
              mgr
                .run_group_a2a_chain(
                  app_handle,
                  gid,
                  member_key,
                  msg_id,
                  uid,
                  raw_content,
                  0,
                  visited,
                )
                .await;
            });
          }
        }
        _ => {
          for (order_idx, member_key, msg_id, stage_label, auto_ch) in dispatch_jobs {
            let mgr = Arc::clone(self);
            let app_handle = app.clone();
            let gid = group_id.clone();
            let uid = user_msg_id.clone();
            let raw_content = trimmed.clone();
            tauri::async_runtime::spawn(async move {
              if order_idx > 0 {
                tokio::time::sleep(std::time::Duration::from_millis((order_idx as u64) * 40))
                  .await;
              }
              let _ = mgr
                .run_group_member_turn_with_failover(
                  &app_handle,
                  &gid,
                  &member_key,
                  &msg_id,
                  &uid,
                  &raw_content,
                  stage_label.as_deref(),
                  None,
                  None,
                  0,
                  auto_ch.as_ref(),
                )
                .await;
            });
          }
        }
      }

      Ok(snap)
    }
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  async fn run_group_member_turn_with_failover(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: &str,
    member_key: &AccountKey,
    msg_id: &str,
    user_msg_id: &str,
    raw_user_content: &str,
    stage_instruction: Option<&str>,
    fallback_stage_instruction: Option<&str>,
    attachment: Option<&GroupAttachment>,
    hop_depth: u8,
    auto_channel: Option<&ProviderId>,
  ) -> Option<(AccountKey, String)> {
    if let Some(reply) = self
      .run_group_member_turn(
        app,
        group_id,
        member_key,
        msg_id,
        user_msg_id,
        raw_user_content,
        stage_instruction,
        fallback_stage_instruction,
        attachment,
        hop_depth,
      )
      .await
    {
      let mut locked = self.inner.lock().await;
      locked.group_channel_last_account.insert(
        (group_id.to_string(), member_key.provider_id.clone()),
        member_key.clone(),
      );
      locked.account_cooldown_until_ms.remove(member_key);
      return Some((member_key.clone(), reply));
    }

    let now = now_ms();
    let failover_snap = {
      let mut locked = self.inner.lock().await;
      locked
        .account_cooldown_until_ms
        .insert(member_key.clone(), now + AUTO_ROUTE_COOLDOWN_MS);
      if let Some(channel_pid) = auto_channel {
        let Some(g_idx) = locked
          .store
          .group_chats
          .iter()
          .position(|g| g.id == group_id)
        else {
          return None;
        };
        let can_failover = locked.store.group_chats[g_idx]
          .messages
          .iter()
          .find(|m| m.id == msg_id)
          .is_some_and(|m| m.content.trim().is_empty());
        if !can_failover {
          return None;
        }
        let eligible_members = Self::collect_group_eligible_members_locked(&locked, g_idx);
        let excluded = [member_key.clone()];
        let Ok((next_key, next_alias)) = Self::select_auto_channel_account_locked(
          &locked,
          group_id,
          channel_pid,
          &eligible_members,
          &excluded,
          now,
        ) else {
          return None;
        };

        locked
          .account_last_assigned_ms
          .insert(next_key.clone(), now);
        if let Some(m) = locked.store.group_chats[g_idx]
          .messages
          .iter_mut()
          .find(|m| m.id == msg_id)
        {
          m.sender = Some(next_key.clone());
          m.sender_name = next_alias;
          m.status = GroupReplyStatus::Queued;
          m.error = None;
          m.content.clear();
        }
        if let Some(um) = locked.store.group_chats[g_idx]
          .messages
          .iter_mut()
          .find(|m| m.id == user_msg_id && m.role == "user")
        {
          if let Some(slot) = um.targets.iter_mut().find(|t| *t == member_key) {
            *slot = next_key.clone();
          }
        }
        let _ = save_store_atomic(&locked.store_path, &locked.store);
        Some((next_key, Self::snapshot_from_locked(&locked)))
      } else {
        None
      }
    };

    if let Some((next_key, snap)) = failover_snap {
      self.emit_snapshot(app, &snap);
      if let Some(reply) = self
        .run_group_member_turn(
          app,
          group_id,
          &next_key,
          msg_id,
          user_msg_id,
          raw_user_content,
          stage_instruction,
          fallback_stage_instruction,
          attachment,
          hop_depth,
        )
        .await
      {
        let mut locked = self.inner.lock().await;
        locked.group_channel_last_account.insert(
          (group_id.to_string(), next_key.provider_id.clone()),
          next_key.clone(),
        );
        locked.account_cooldown_until_ms.remove(&next_key);
        return Some((next_key, reply));
      } else {
        let mut locked = self.inner.lock().await;
        locked
          .account_cooldown_until_ms
          .insert(next_key, now_ms() + AUTO_ROUTE_COOLDOWN_MS);
      }
    }

    None
  }

  pub async fn retry_group_message(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: String,
    msg_id: String,
  ) -> Result<ManagerSnapshot, String> {
    #[cfg(not(desktop))]
    {
      let _ = (app, group_id, msg_id);
      Err("群聊功能暂仅支持桌面端".to_string())
    }

    #[cfg(desktop)]
    {
      let (snap, member_key, user_msg_id, raw_content, stage_label, hop_depth, auto_ch) = {
        let mut locked = self.inner.lock().await;
        if !locked.conflicts.is_empty() {
          return Err("存在 UUID 归属冲突，请先处理冲突后再重试".to_string());
        }
        let g_idx = locked
          .store
          .group_chats
          .iter()
          .position(|g| g.id == group_id)
          .ok_or_else(|| "未找到对应的群聊".to_string())?;
        let msg_idx = locked.store.group_chats[g_idx]
          .messages
          .iter()
          .position(|m| m.id == msg_id && m.role == "assistant")
          .ok_or_else(|| "未找到可重试的回复消息".to_string())?;

        let target_msg = &locked.store.group_chats[g_idx].messages[msg_idx];
        if !matches!(
          target_msg.status,
          GroupReplyStatus::Error | GroupReplyStatus::NeedsAuth
        ) {
          return Err("仅支持对失败或待授权的消息点击重试".to_string());
        }
        let member_key = target_msg
          .sender
          .clone()
          .ok_or_else(|| "找不到该消息对应的成员".to_string())?;
        let stage_label = target_msg.stage_label.clone();
        let hop_depth = target_msg.hop_depth;
        let auto_ch = target_msg
          .auto_routed_channel
          .as_deref()
          .and_then(ProviderId::parse);

        let member_available = if member_key.provider_id.is_external() {
          locked
            .store
            .external_agents
            .iter()
            .any(|e| e.account_key() == member_key)
        } else {
          locked
            .store
            .accounts
            .iter()
            .find(|a| a.provider_id == member_key.provider_id && a.uuid == member_key.uuid)
            .is_some_and(|acc| {
              acc.auth_state != AuthState::LoggedOut
                && resolve_provider(&acc.provider_id).is_some_and(|s| s.info.enabled)
            })
        };
        if !member_available {
          return Err("该成员当前未登录或配置不可用，请先检查成员状态".to_string());
        }

        let (user_msg_id, raw_content) = locked.store.group_chats[g_idx].messages[..msg_idx]
          .iter()
          .rev()
          .find(|m| m.role == "user" && !m.content.trim().is_empty())
          .map(|m| (m.id.clone(), m.content.clone()))
          .ok_or_else(|| "找不到对应的原始提问消息，请重新发送".to_string())?;

        let msg_mut = &mut locked.store.group_chats[g_idx].messages[msg_idx];
        let prev_status = msg_mut.status;
        let prev_content = std::mem::take(&mut msg_mut.content);
        let prev_error = msg_mut.error.take();
        msg_mut.status = GroupReplyStatus::Queued;
        if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
          let msg_revert = &mut locked.store.group_chats[g_idx].messages[msg_idx];
          msg_revert.status = prev_status;
          msg_revert.content = prev_content;
          msg_revert.error = prev_error;
          return Err(err);
        }

        (
          Self::snapshot_from_locked(&locked),
          member_key,
          user_msg_id,
          raw_content,
          stage_label,
          hop_depth,
          auto_ch,
        )
      };

      self.emit_snapshot(app, &snap);

      let mgr = Arc::clone(self);
      let app_handle = app.clone();
      let gid = group_id.clone();
      let mid = msg_id.clone();
      tauri::async_runtime::spawn(async move {
        let _ = mgr
          .run_group_member_turn_with_failover(
            &app_handle,
            &gid,
            &member_key,
            &mid,
            &user_msg_id,
            &raw_content,
            stage_label.as_deref(),
            None,
            None,
            hop_depth,
            auto_ch.as_ref(),
          )
          .await;
      });

      Ok(snap)
    }
  }

  pub async fn sync_group_message_reply(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: String,
    msg_id: String,
  ) -> Result<ManagerSnapshot, String> {
    #[cfg(not(desktop))]
    {
      let _ = (app, group_id, msg_id);
      Err("群聊功能暂仅支持桌面端".to_string())
    }

    #[cfg(desktop)]
    {
      let (member_key, raw_user_content, app_data_dir) = {
        let locked = self.inner.lock().await;
        if !locked.conflicts.is_empty() {
          return Err("存在 UUID 归属冲突，请先处理冲突后再同步".to_string());
        }
        let g_idx = locked
          .store
          .group_chats
          .iter()
          .position(|g| g.id == group_id)
          .ok_or_else(|| "未找到对应的群聊".to_string())?;
        let msg_idx = locked.store.group_chats[g_idx]
          .messages
          .iter()
          .position(|m| m.id == msg_id && m.role == "assistant")
          .ok_or_else(|| "未找到可同步的回复消息".to_string())?;

        let target_msg = &locked.store.group_chats[g_idx].messages[msg_idx];
        if matches!(
          target_msg.status,
          GroupReplyStatus::Queued | GroupReplyStatus::Sending | GroupReplyStatus::Streaming
        ) {
          return Err("该消息仍在生成中，请稍候".to_string());
        }
        let member_key = target_msg
          .sender
          .clone()
          .ok_or_else(|| "找不到该消息对应的成员".to_string())?;
        if member_key.provider_id.is_external() {
          return Err("API/CLI 成员不支持从页面同步，请使用重试".to_string());
        }
        let member_available = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == member_key.provider_id && a.uuid == member_key.uuid)
          .is_some_and(|acc| {
            acc.auth_state != AuthState::LoggedOut
              && resolve_provider(&acc.provider_id).is_some_and(|s| s.info.enabled)
          });
        if !member_available {
          return Err("该成员当前未登录或配置不可用，请先检查成员状态".to_string());
        }
        let raw_user_content = locked.store.group_chats[g_idx].messages[..msg_idx]
          .iter()
          .rev()
          .find(|m| m.role == "user" && !m.content.trim().is_empty())
          .map(|m| m.content.clone())
          .unwrap_or_default();

        (member_key, raw_user_content, locked.app_data_dir.clone())
      };

      let (wv, _) = self
        .ensure_group_member_webview_ready(app, &member_key)
        .await?;
      Self::wake_and_settle_group_webview(&wv).await;

      let host = wv
        .url()
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_ascii_lowercase()))
        .unwrap_or_default();
      let adapter = site_adapters::resolve_adapter_for_host(&app_data_dir, &host);
      let is_hark = host.contains("hark.com");

      let sync_js = build_group_sync_latest_js(&adapter, &raw_user_content);
      let res = Self::eval_json_callback(&wv, &sync_js)
        .await
        .ok_or_else(|| "读取该账号页面内容超时，请前往原生页面确认后重试".to_string())?;
      let raw_text = res
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
      let mut final_text = Self::clean_extracted_reply_text(&raw_text);
      if is_hark && !final_text.trim().is_empty() {
        final_text = self
          .extract_hark_expanded_card_if_needed(&wv, &final_text)
          .await;
        final_text = Self::clean_extracted_reply_text(&final_text);
      }
      if final_text.trim().is_empty() {
        let err_hint = res
          .get("error")
          .and_then(|v| v.as_str())
          .filter(|s| !s.trim().is_empty())
          .unwrap_or("暂未从该账号原生页面读取到有效回复，请前往页面确认后再同步");
        return Err(err_hint.to_string());
      }

      let snap = {
        let mut locked = self.inner.lock().await;
        let g = locked
          .store
          .group_chats
          .iter_mut()
          .find(|g| g.id == group_id)
          .ok_or_else(|| "未找到对应的群聊".to_string())?;
        let m = g
          .messages
          .iter_mut()
          .find(|m| m.id == msg_id && m.role == "assistant")
          .ok_or_else(|| "未找到可同步的回复消息".to_string())?;
        let prev_status = m.status;
        let prev_content = std::mem::replace(&mut m.content, final_text);
        let prev_error = m.error.take();
        m.status = GroupReplyStatus::Done;
        if let Err(err) = save_store_atomic(&locked.store_path, &locked.store) {
          if let Some(g_rev) = locked.store.group_chats.iter_mut().find(|g| g.id == group_id) {
            if let Some(m_rev) = g_rev.messages.iter_mut().find(|m| m.id == msg_id) {
              m_rev.status = prev_status;
              m_rev.content = prev_content;
              m_rev.error = prev_error;
            }
          }
          return Err(err);
        }
        Self::snapshot_from_locked(&locked)
      };

      self.emit_snapshot(app, &snap);
      Ok(snap)
    }
  }

  #[cfg(desktop)]
  async fn eval_json_callback(webview: &tauri::Webview, js: &str) -> Option<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx_cell = Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_inner = Arc::clone(&tx_cell);
    webview
      .eval_with_callback(js, move |raw| {
        if let Some(sender) = tx_inner.lock().unwrap().take() {
          let _ = sender.send(raw);
        }
      })
      .ok()?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(5000), rx)
      .await
      .ok()?
      .ok()?;
    if let Ok(inner_json) = serde_json::from_str::<String>(&raw) {
      if let Ok(val) = serde_json::from_str::<serde_json::Value>(&inner_json) {
        return Some(val);
      }
    }
    serde_json::from_str::<serde_json::Value>(&raw).ok()
  }

  #[cfg(desktop)]
  async fn update_group_message_state(
    &self,
    app: &AppHandle,
    group_id: &str,
    msg_id: &str,
    status: GroupReplyStatus,
    content_opt: Option<String>,
    error_opt: Option<Option<String>>,
    persist: bool,
  ) -> bool {
    let snap_opt = {
      let mut locked = self.inner.lock().await;
      let Some(g) = locked.store.group_chats.iter_mut().find(|g| g.id == group_id) else {
        return false;
      };
      let Some(m) = g.messages.iter_mut().find(|m| m.id == msg_id) else {
        return false;
      };
      let mut changed = m.status != status;
      m.status = status;
      if let Some(content) = content_opt {
        if m.content != content {
          m.content = content;
          changed = true;
        }
      }
      if let Some(err) = error_opt {
        if m.error != err {
          m.error = err;
          changed = true;
        }
      }
      if !changed && !persist {
        return true;
      }
      if persist {
        let _ = save_store_atomic(&locked.store_path, &locked.store);
      }
      Some(Self::snapshot_from_locked(&locked))
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
      true
    } else {
      false
    }
  }

  #[cfg(desktop)]
  async fn ensure_group_member_webview_ready(
    self: &Arc<Self>,
    app: &AppHandle,
    key: &AccountKey,
  ) -> Result<(tauri::Webview, u64), String> {
    let account_to_start = {
      let mut locked = self.inner.lock().await;
      if let Some(entry) = locked.warm_pool.get_mut(key) {
        if entry.paint_ready {
          entry.last_used_ms = now_ms();
          let vid = entry.view_instance_id;
          let label = entry.webview_label.clone();
          if let Some(wv) = app.get_webview(&label) {
            return Ok((wv, vid));
          }
        }
        None
      } else {
        let Some(acc) = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
          .cloned()
        else {
          return Err("账号不存在或已被移除".to_string());
        };
        if acc.auth_state == AuthState::LoggedOut {
          return Err("该账号当前处于未登录状态，请先前往登录".to_string());
        }
        Some(acc)
      }
    };

    if let Some(acc) = account_to_start {
      self
        .prewarm_desktop_account_with_limit(app, acc, true)
        .await?;
    }

    for _ in 0..200 {
      {
        let mut locked = self.inner.lock().await;
        let Some(acc) = locked
          .store
          .accounts
          .iter()
          .find(|a| a.provider_id == key.provider_id && a.uuid == key.uuid)
        else {
          return Err("账号已被移除".to_string());
        };
        if acc.auth_state == AuthState::LoggedOut {
          return Err("该账号未登录或会话已失效，请前往原生页面重新登录".to_string());
        }
        if let Some(entry) = locked.warm_pool.get_mut(key) {
          if entry.paint_ready {
            entry.last_used_ms = now_ms();
            let vid = entry.view_instance_id;
            let label = entry.webview_label.clone();
            if let Some(wv) = app.get_webview(&label) {
              return Ok((wv, vid));
            }
          }
        }
      }
      tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    Err("成员页面加载超时，请切换至该账号页确认状态".to_string())
  }

  #[cfg(desktop)]
  fn normalize_url_for_thread(url: &Url) -> String {
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path().trim_end_matches('/');
    match url.query() {
      Some(q) if !q.is_empty() => format!("{scheme}://{host}{path}?{q}"),
      _ => format!("{scheme}://{host}{path}"),
    }
  }

  #[cfg(desktop)]
  async fn wait_webview_settled(wv: &tauri::Webview, max_ticks: usize) {
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    for _ in 0..max_ticks {
      #[cfg(target_os = "macos")]
      let loading = platform::is_macos_webview_loading(wv).await;
      #[cfg(not(target_os = "macos"))]
      let loading = false;
      if !loading {
        if let Some(val) = Self::eval_json_callback(wv, "document.readyState === 'complete'").await {
          if val.as_bool() == Some(true) {
            return;
          }
        }
      }
      tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
  }

  #[cfg(desktop)]
  async fn prepare_group_member_thread(
    &self,
    group_id: &str,
    member_key: &AccountKey,
    wv: &tauri::Webview,
  ) {
    let Ok(cur_url) = wv.url() else {
      return;
    };
    let (app_data_dir, spec_opt, bound_url) = {
      let locked = self.inner.lock().await;
      let spec = resolve_provider(&member_key.provider_id);
      let bound = locked
        .store
        .group_chats
        .iter()
        .find(|g| g.id == group_id)
        .and_then(|g| g.thread_bindings.iter().find(|tb| &tb.member == member_key))
        .map(|tb| tb.thread_url.clone());
      (locked.app_data_dir.clone(), spec, bound)
    };
    let Some(spec) = spec_opt else {
      return;
    };
    let cur_host = cur_url.host_str().unwrap_or("");
    let adapter = site_adapters::resolve_adapter_for_host(&app_data_dir, cur_host);
    if adapter.thread_url_patterns.is_empty() {
      return;
    }

    if let Some(bound_str) = bound_url {
      if let Ok(target_url) = Url::parse(&bound_str) {
        if matches!(
          classify_top_level_url(&spec, &target_url, false),
          NavigationDecision::AllowHttps
        ) && Self::normalize_url_for_thread(&target_url)
          != Self::normalize_url_for_thread(&cur_url)
        {
          if wv.navigate(target_url).is_ok() {
            Self::wait_webview_settled(wv, 24).await;
          }
        }
      }
      return;
    }

    let cur_str = cur_url.as_str();
    let on_existing_thread = adapter
      .thread_url_patterns
      .iter()
      .any(|p| !p.is_empty() && cur_str.contains(p));
    if !on_existing_thread {
      return;
    }

    if !adapter.new_chat_selectors.is_empty() {
      let click_js = build_new_chat_click_js(&adapter);
      if Self::eval_json_callback(wv, &click_js)
        .await
        .and_then(|v| v.get("clicked").and_then(|b| b.as_bool()))
        .unwrap_or(false)
      {
        Self::wait_webview_settled(wv, 14).await;
        return;
      }
    }

    if let Some(new_url) = adapter
      .new_chat_url
      .as_deref()
      .and_then(|s| Url::parse(s).ok())
    {
      if matches!(
        classify_top_level_url(&spec, &new_url, false),
        NavigationDecision::AllowHttps
      ) && Self::normalize_url_for_thread(&new_url)
        != Self::normalize_url_for_thread(&cur_url)
      {
        if wv.navigate(new_url).is_ok() {
          Self::wait_webview_settled(wv, 20).await;
        }
      }
    }
  }

  #[cfg(desktop)]
  async fn persist_group_thread_binding(
    &self,
    app: &AppHandle,
    group_id: &str,
    member_key: &AccountKey,
    _pre_send_norm: Option<&str>,
    wv: &tauri::Webview,
    adapter: &SiteAdapterConfig,
  ) {
    if adapter.thread_url_patterns.is_empty() {
      return;
    }
    let Ok(cur_url) = wv.url() else {
      return;
    };
    let cur_str = cur_url.as_str().to_string();
    let matches_pat = adapter
      .thread_url_patterns
      .iter()
      .any(|p| !p.is_empty() && cur_str.contains(p));
    if !matches_pat {
      return;
    }

    let snap_opt = {
      let mut locked = self.inner.lock().await;
      let valid_url = resolve_provider(&member_key.provider_id)
        .is_some_and(|spec| {
          matches!(
            classify_top_level_url(&spec, &cur_url, false),
            NavigationDecision::AllowHttps
          )
        });
      if !valid_url {
        return;
      }
      let Some(g) = locked
        .store
        .group_chats
        .iter_mut()
        .find(|g| g.id == group_id)
      else {
        return;
      };
      if let Some(existing) = g
        .thread_bindings
        .iter_mut()
        .find(|tb| &tb.member == member_key)
      {
        if existing.thread_url == cur_str {
          return;
        }
        existing.thread_url = cur_str;
        existing.updated_at_ms = now_ms();
      } else {
        g.thread_bindings.push(GroupThreadBinding {
          member: member_key.clone(),
          thread_url: cur_str,
          updated_at_ms: now_ms(),
        });
      }
      let _ = save_store_atomic(&locked.store_path, &locked.store);
      Some(Self::snapshot_from_locked(&locked))
    };

    if let Some(snap) = snap_opt {
      self.emit_snapshot(app, &snap);
    }
  }

  #[cfg(desktop)]
  async fn resolve_member_transport_locked(
    locked: &InnerState,
    key: &AccountKey,
  ) -> Result<ResolvedMemberTransport, String> {
    if key.provider_id.is_external() {
      let ext = locked
        .store
        .external_agents
        .iter()
        .find(|e| e.account_key() == *key)
        .cloned()
        .ok_or_else(|| "API/CLI 成员不存在或已被移除".to_string())?;
      return Ok(match ext.kind {
        ExternalAgentKind::OpenaiApi => ResolvedMemberTransport::OpenAiApi { agent: ext },
        ExternalAgentKind::LocalCli => ResolvedMemberTransport::LocalCli { agent: ext },
      });
    }
    Ok(ResolvedMemberTransport::WebView { key: key.clone() })
  }

  #[cfg(desktop)]
  fn sanitize_history_assistant_text(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if s.is_empty() {
      return None;
    }
    for prefix in [
      "I don't collaborate with other accounts' assistants, but",
      "I don't work with other accounts' assistants, but",
      "I can't speak for another account in the group chat, but",
      "I can't speak on behalf of another account in the group chat, but",
      "我不和其他账号的助手一起协作分工，但",
      "我不和其他账号的助手一起协作分工,",
      "我不和其他账号的助手一起协作分工。",
      "群聊里替某个账号发言我做不了，不过",
      "群聊里替某个账号发言我做不了，",
      "群聊里替某个账号发言我做不了。",
    ] {
      if let Some(rest) = s.strip_prefix(prefix) {
        s = rest.trim();
      }
    }
    if s.is_empty() {
      return None;
    }
    if s.contains("2项预定任务") && s.contains("查看所有预定任务") {
      return None;
    }
    let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
      return None;
    }
    if lines.len() <= 3 && s.chars().count() <= 80 {
      let all_transient = lines.iter().all(|l| {
        matches!(
          *l,
          "Muse"
            | "Hark"
            | "Idle"
            | "Connected"
            | "Connecting"
            | "Still sending"
            | "Sending"
            | "Working hard"
            | "Working"
            | "Thinking hard"
            | "Thinking"
            | "Searching"
            | "Searching the web"
            | "Searching online"
            | "Retrieving"
            | "Loading"
            | "Standby"
            | "空闲中"
            | "已连接"
            | "连接中"
            | "正在连接"
            | "仍在发送中"
            | "发送中"
            | "努力工作中"
            | "工作中"
            | "努力思考中"
            | "正在思考"
            | "思考中"
            | "正在搜索"
            | "正在搜索网页"
            | "正在联网搜索"
            | "搜索中"
            | "检索中"
            | "正在检索"
            | "加载中"
            | "待命中"
            | "Researching channels"
        ) || l.starts_with("Searching for ")
          || l.starts_with("Researching ")
      });
      if all_transient {
        return None;
      }
    }
    Some(s.chars().take(240).collect())
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  #[cfg_attr(not(desktop), allow(dead_code))]
  fn sanitize_shared_context_for_prompt(raw: &str) -> String {
    let mut out_lines: Vec<String> = Vec::new();
    let mut idx = 0usize;
    for line in raw.trim().lines() {
      let t = line.trim();
      if let Some(inner) = t
        .strip_prefix("[Round conclusion summary").or_else(|| t.strip_prefix("[本轮结论汇总"))
        .and_then(|s| s.strip_suffix(']'))
      {
        idx = 0;
        let topic = inner.trim_start_matches([' ', '·']).trim();
        out_lines.push(if topic.is_empty() {
          "Conclusions so far".to_string()
        } else {
          format!("Conclusions so far (topic: {topic})")
        });
        continue;
      }
      if let Some(rest) = t.strip_prefix("- ") {
        if let Some(pos) = rest.find(": ") {
          if rest[..pos].contains('@') {
            idx += 1;
            out_lines.push(format!("- Point {idx}: {}", &rest[pos + 2..]));
            continue;
          }
        }
      }
      out_lines.push(line.to_string());
    }
    out_lines.join("\n").chars().take(3000).collect()
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn clean_extracted_reply_text(raw: &str) -> String {
    const SPEAKERS: [&str; 6] = ["Gemini", "ChatGPT", "Claude", "DeepSeek", "Grok", "Kimi"];
    const VERBS: [&str; 3] = ["说", "said", "says"];

    let mut s = raw.trim();
    loop {
      let mut stripped = None;
      'outer: for sp in SPEAKERS {
        if let Some(after_sp) = Self::starts_with_ignore_case(s, sp).map(|len| &s[len..]) {
          let after_sp_ws = after_sp.trim_start_matches([' ', '\t']);
          for verb in VERBS {
            if let Some(v_len) = Self::starts_with_ignore_case(after_sp_ws, verb) {
              let tail = &after_sp_ws[v_len..];
              let tail_ws = tail.trim_start_matches([' ', '\t']);
              if tail_ws.is_empty() {
                return String::new();
              }
              if let Some(after_colon) = tail_ws
                .strip_prefix(':')
                .or_else(|| tail_ws.strip_prefix('：'))
              {
                stripped = Some(after_colon.trim_start());
                break 'outer;
              }
              if tail_ws.starts_with('\n') || tail_ws.starts_with('\r') {
                stripped = Some(tail_ws.trim_start());
                break 'outer;
              }
            }
          }
        }
      }
      match stripped {
        Some(next) => s = next,
        None => break,
      }
    }
    s.trim().to_string()
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn adapt_question_for_hark(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for (pat, rep) in [
      ("everyone in the group", "you"),
      ("Everyone in the group", "you"),
      ("all of you", "you"),
      ("All of you", "you"),
      ("hey everyone", "hello"),
      ("Hey everyone", "hello"),
      ("hello everyone", "hello"),
      ("Hello everyone", "hello"),
      ("please everyone", "please"),
      ("Please everyone", "please"),
      ("each of you", "you"),
      ("you all", "you"),
      ("everyone", "you"),
      ("Everyone", "you"),
      ("群里的各位", "你"),
      ("群里各位", "你"),
      ("群里大家", "你"),
      ("各位好", "你好"),
      ("大家好", "你好"),
      ("请各位", "请你"),
      ("请大家", "请你"),
      ("各位成员", "你"),
      ("各位助手", "你"),
      ("你们分别", "请你"),
      ("你们各自", "请你"),
      ("你们几个", "你"),
      ("你们", "你"),
    ] {
      s = s.replace(pat, rep);
    }

    for action in [
      "觉得", "怎么", "看看", "看下", "看", "帮忙", "帮我", "帮", "来", "说说", "说下", "说",
      "回答", "回复", "回", "能否", "能", "可以", "一起", "各自", "都", "有", "给",
    ] {
      s = s.replace(&format!("各位{action}"), &format!("你{action}"));
      s = s.replace(&format!("大家{action}"), &format!("你{action}"));
    }

    for prefix in [
      "Everyone, ",
      "Everyone: ",
      "Everyone - ",
      "All of you, ",
      "All of you: ",
      "Hey everyone, ",
      "Hello everyone, ",
      "各位，",
      "各位,",
      "各位：",
      "各位:",
      "各位、",
      "各位 ",
      "大家，",
      "大家,",
      "大家：",
      "大家:",
      "大家、",
      "大家 ",
    ] {
      if let Some(rest) = s.strip_prefix(prefix) {
        let rest_trim = rest.trim();
        if !rest_trim.is_empty() {
          s = rest_trim.to_string();
          break;
        }
      }
    }

    // The replacement pass above turns a leading group address such as
    // "Everyone, " / "All of you, " into a leading "you, ", which the
    // prefix strip above (written for the pre-replacement text) no longer
    // matches. Strip that leftover vocative, mirroring how the Chinese
    // "各位，" / "大家，" prefixes are dropped entirely. Only a punctuated
    // leading vocative is stripped, so a mid-sentence "you" (e.g. "What do
    // you think ...") and "hello, ..." are left untouched.
    for prefix in [
      "you,", "you:", "you - ", "You,", "You:", "You - ", "你，", "你,", "你：", "你:",
    ] {
      if let Some(rest) = s.strip_prefix(prefix) {
        let rest_trim = rest.trim();
        if !rest_trim.is_empty() {
          s = rest_trim.to_string();
          break;
        }
      }
    }

    let cleaned = s.trim().to_string();
    if cleaned.is_empty() {
      raw.trim().to_string()
    } else {
      cleaned
    }
  }

  #[cfg(desktop)]
  fn sanitize_shared_context_for_hark(raw: &str) -> String {
    let base = Self::sanitize_shared_context_for_prompt(raw);
    let mut out_lines: Vec<String> = Vec::new();
    for line in base.lines() {
      let t = line.trim();
      if t == "Conclusions so far" || t == "已有结论" {
        out_lines.push("Background points:".to_string());
        continue;
      }
      if let Some(inner) = t
        .strip_prefix("Conclusions so far (topic: ")
        .and_then(|s| s.strip_suffix(')'))
        .or_else(|| t.strip_prefix("已有结论（议题：").and_then(|s| s.strip_suffix('）')))
      {
        out_lines.push(format!("Background points ({inner}):"));
        continue;
      }
      if let Some(rest) = t.strip_prefix("- Point").or_else(|| t.strip_prefix("- 观点")) {
        if let Some(pos) = rest.find(": ") {
          if rest[..pos].chars().all(|c| c.is_ascii_digit()) {
            out_lines.push(format!("- {}", &rest[pos + 2..]));
            continue;
          }
        }
      }
      out_lines.push(line.to_string());
    }
    out_lines.join("\n").chars().take(3000).collect()
  }

  #[cfg(desktop)]
  async fn build_turn_prompt_for_member(
    &self,
    group_id: &str,
    member_key: &AccountKey,
    view_instance_id: u64,
    msg_id: &str,
    user_msg_id: &str,
    raw_user_content: &str,
    stage_instruction: Option<&str>,
    hop_depth: u8,
  ) -> Option<(String, String, String, u64)> {
    let locked = self.inner.lock().await;
    let group = locked.store.group_chats.iter().find(|g| g.id == group_id)?;

    let fingerprint = Self::compute_group_knowledge_fingerprint(group, member_key);
    let synced_key = (group_id.to_string(), member_key.clone());
    let needs_full_sync = member_key.provider_id.is_external()
      || locked.group_context_synced.get(&synced_key) != Some(&(view_instance_id, fingerprint));

    let member_pairs: Vec<(AccountKey, String)> = group
      .members
      .iter()
      .filter_map(|m| Self::resolve_member_alias_locked(&locked, m).map(|a| (m.clone(), a)))
      .collect();

    let is_hark = member_key.provider_id.is_hark();
    let stripped_question = Self::strip_routing_mentions(raw_user_content, &member_pairs);
    let clean_question = if is_hark {
      Self::adapt_question_for_hark(&stripped_question)
    } else {
      stripped_question
    };

    let my_role = group
      .member_roles
      .iter()
      .find(|r| &r.member == member_key)
      .map(|r| r.role.trim())
      .filter(|r| !r.is_empty());

    let enabled_skills: Vec<&GroupSharedSkill> =
      group.shared_skills.iter().filter(|s| s.enabled).collect();
    let has_shared_ctx = !group.shared_context.trim().is_empty();
    let stage_clean = stage_instruction.map(str::trim).filter(|s| !s.is_empty());

    let format_skill_lines = |max_chars: usize| -> Vec<String> {
      let mut skill_lines = Vec::new();
      let mut total_chars = 0usize;
      for sk in &enabled_skills {
        let line = format!("- “{}”: {}", sk.name.trim(), sk.content.trim());
        let count = line.chars().count();
        if total_chars + count > max_chars && !skill_lines.is_empty() {
          break;
        }
        total_chars += count;
        skill_lines.push(line);
      }
      skill_lines
    };

    let is_webview_parallel = !member_key.provider_id.is_external()
      && group.workflow_mode == GroupWorkflowMode::Parallel
      && stage_clean.is_none();

    if is_webview_parallel {
      let mut sys_sections: Vec<String> = Vec::new();
      if needs_full_sync {
        if let Some(role) = my_role {
          sys_sections.push(format!("Please answer mainly from the perspective of “{role}”."));
        }
        if has_shared_ctx {
          let ctx_snip = if is_hark {
            Self::sanitize_shared_context_for_hark(&group.shared_context)
          } else {
            Self::sanitize_shared_context_for_prompt(&group.shared_context)
          };
          sys_sections.push(format!("Reference background:\n{ctx_snip}"));
        }
        if !enabled_skills.is_empty() {
          let skill_lines = format_skill_lines(4000);
          if !skill_lines.is_empty() {
            sys_sections.push(format!("Answer requirements:\n{}", skill_lines.join("\n")));
          }
        }
      }

      if sys_sections.is_empty() {
        return Some((
          String::new(),
          clean_question.clone(),
          clean_question,
          fingerprint,
        ));
      }

      let system_section = sys_sections.join("\n\n");
      let user_section = format!("Question:\n{clean_question}");
      let full_prompt = format!("{system_section}\n\n{user_section}");
      return Some((system_section, user_section, full_prompt, fingerprint));
    }

    let mut cutoff = group
      .messages
      .iter()
      .position(|m| m.id == msg_id)
      .unwrap_or(group.messages.len());
    if group.workflow_mode == GroupWorkflowMode::Review && stage_clean.is_some() {
      if let Some(user_pos) = group.messages.iter().position(|m| m.id == user_msg_id) {
        cutoff = cutoff.min(user_pos);
      }
    }
    let history_lines = group.messages[..cutoff]
      .iter()
      .rev()
      .filter(|m| m.id != user_msg_id && m.status == GroupReplyStatus::Done && !m.content.trim().is_empty())
      .filter_map(|m| {
        if m.role == "user" {
          let q = Self::strip_routing_mentions(&m.content, &member_pairs);
          let snip: String = q.trim().chars().take(200).collect();
          if snip.is_empty() {
            None
          } else {
            Some(format!("- Question: {snip}"))
          }
        } else {
          let snip = Self::sanitize_history_assistant_text(&m.content)?;
          let raw_speaker = m
            .sender
            .as_ref()
            .and_then(|sk| Self::resolve_member_alias_locked(&locked, sk))
            .unwrap_or_else(|| m.sender_name.clone());
          let label = if is_hark || raw_speaker.contains('@') {
            "Known reference".to_string()
          } else {
            format!("Reference ({raw_speaker})")
          };
          Some(format!("- {label}: {snip}"))
        }
      })
      .take(6)
      .collect::<Vec<_>>()
      .into_iter()
      .rev()
      .collect::<Vec<_>>();

    let is_a2a_active =
      !is_hark && group.workflow_mode == GroupWorkflowMode::A2aRelay && hop_depth < group.max_relay_hops;

    let mut sys_sections: Vec<String> = Vec::new();
    if needs_full_sync {
      if let Some(role) = my_role {
        sys_sections.push(format!(
          "[Answer perspective]\nPlease focus on analyzing and answering from the “{role}” perspective."
        ));
      }
      if has_shared_ctx {
        let ctx_snip = if is_hark {
          Self::sanitize_shared_context_for_hark(&group.shared_context)
        } else {
          Self::sanitize_shared_context_for_prompt(&group.shared_context)
        };
        sys_sections.push(format!("[Background]\n{ctx_snip}"));
      }
      if !enabled_skills.is_empty() {
        let skill_lines = format_skill_lines(4000);
        if !skill_lines.is_empty() {
          sys_sections.push(format!("[Output format]\n{}", skill_lines.join("\n")));
        }
      }
    } else if has_shared_ctx || !enabled_skills.is_empty() || my_role.is_some() {
      sys_sections.push("(Please continue to follow the background and output format already provided in this conversation)".to_string());
    }

    if let Some(stage_inst) = stage_clean {
      sys_sections.push(format!("[Stage requirements]\n{stage_inst}"));
    }

    let mut user_sections: Vec<String> = Vec::new();
    if !history_lines.is_empty() {
      user_sections.push(format!("[Previous context]\n{}", history_lines.join("\n")));
    }

    if is_a2a_active {
      let other_names = group
        .members
        .iter()
        .filter(|m| *m != member_key)
        .filter_map(|m| Self::resolve_member_alias_locked(&locked, m))
        .collect::<Vec<_>>()
        .join(", ");
      if !other_names.is_empty() {
        user_sections.push(format!(
          "[Relay note]\nIf a follow-up check is needed, you may append an @mention at the end of your answer (optional: {other_names}); if no relay is needed, do not write @."
        ));
      }
    }

    if sys_sections.is_empty() && user_sections.is_empty() {
      return Some((
        String::new(),
        clean_question.clone(),
        clean_question,
        fingerprint,
      ));
    }

    user_sections.push(format!(
      "[Current question]\n{}\n(Please answer the “Current question” directly, and do not repeat the bracketed headings above)",
      clean_question
    ));

    let system_section = sys_sections.join("\n\n");
    let user_section = user_sections.join("\n\n");
    let full_prompt = if system_section.is_empty() {
      user_section.clone()
    } else {
      format!("{system_section}\n\n{user_section}")
    };
    Some((system_section, user_section, full_prompt, fingerprint))
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  async fn run_group_member_turn(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: &str,
    member_key: &AccountKey,
    msg_id: &str,
    user_msg_id: &str,
    raw_user_content: &str,
    stage_instruction: Option<&str>,
    fallback_stage_instruction: Option<&str>,
    attachment: Option<&GroupAttachment>,
    hop_depth: u8,
  ) -> Option<String> {
    let transport = {
      let locked = self.inner.lock().await;
      Self::resolve_member_transport_locked(&locked, member_key).await
    };

    let transport = match transport {
      Ok(t) => t,
      Err(err) => {
        self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            GroupReplyStatus::Error,
            None,
            Some(Some(err)),
            true,
          )
          .await;
        return None;
      }
    };

    match transport {
      ResolvedMemberTransport::WebView { .. } => {
        let member_lock = {
          let mut locked = self.inner.lock().await;
          *locked
            .in_flight_group_members
            .entry(member_key.clone())
            .or_insert(0) += 1;
          Self::apply_desktop_layout_locked(app, &locked);
          Arc::clone(
            locked
              .group_member_locks
              .entry(member_key.clone())
              .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
          )
        };
        let _member_guard = member_lock.lock_owned().await;
        let reply = if let Ok(_permit) = Arc::clone(&self.group_worker_semaphore).acquire_owned().await {
          self
            .execute_webview_member_turn(
              app,
              group_id,
              member_key,
              msg_id,
              user_msg_id,
              raw_user_content,
              stage_instruction,
              fallback_stage_instruction,
              attachment,
              hop_depth,
            )
            .await
        } else {
          None
        };

        {
          let mut locked = self.inner.lock().await;
          if let Some(cnt) = locked.in_flight_group_members.get_mut(member_key) {
            *cnt = cnt.saturating_sub(1);
            if *cnt == 0 {
              locked.in_flight_group_members.remove(member_key);
            }
          }
          if let Some(entry) = locked.warm_pool.get_mut(member_key) {
            entry.last_used_ms = now_ms();
          }
          Self::apply_desktop_layout_locked(app, &locked);
        }
        reply
      }
      ResolvedMemberTransport::OpenAiApi { agent }
      | ResolvedMemberTransport::LocalCli { agent } => {
        self
          .execute_external_member_turn(
            app,
            group_id,
            member_key,
            &agent,
            msg_id,
            user_msg_id,
            raw_user_content,
            stage_instruction,
            hop_depth,
          )
          .await
      }
    }
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  async fn execute_external_member_turn(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: &str,
    member_key: &AccountKey,
    agent: &ExternalAgentRecord,
    msg_id: &str,
    user_msg_id: &str,
    raw_user_content: &str,
    stage_instruction: Option<&str>,
    hop_depth: u8,
  ) -> Option<String> {
    if !self
      .update_group_message_state(
        app,
        group_id,
        msg_id,
        GroupReplyStatus::Sending,
        None,
        Some(None),
        false,
      )
      .await
    {
      return None;
    }

    let Some((system_section, user_section, full_prompt, _fingerprint)) = self
      .build_turn_prompt_for_member(
        group_id,
        member_key,
        0,
        msg_id,
        user_msg_id,
        raw_user_content,
        stage_instruction,
        hop_depth,
      )
      .await
    else {
      return None;
    };

    if !self
      .update_group_message_state(
        app,
        group_id,
        msg_id,
        GroupReplyStatus::Streaming,
        None,
        Some(None),
        false,
      )
      .await
    {
      return None;
    }

    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mgr_prog = Arc::clone(self);
    let app_prog = app.clone();
    let gid_prog = group_id.to_string();
    let mid_prog = msg_id.to_string();
    let fwd_handle = tauri::async_runtime::spawn(async move {
      while let Some(partial) = prog_rx.recv().await {
        let _ = mgr_prog
          .update_group_message_state(
            &app_prog,
            &gid_prog,
            &mid_prog,
            GroupReplyStatus::Streaming,
            Some(partial),
            None,
            false,
          )
          .await;
      }
    });

    let result = match agent.kind {
      ExternalAgentKind::OpenaiApi => {
        group_transport::execute_openai_api_turn(agent, &system_section, &user_section, |partial| {
          let _ = prog_tx.send(partial);
        })
        .await
      }
      ExternalAgentKind::LocalCli => {
        group_transport::execute_local_cli_turn(agent, &full_prompt, |partial| {
          let _ = prog_tx.send(partial);
        })
        .await
      }
    };

    drop(prog_tx);
    let _ = fwd_handle.await;

    match result {
      Ok(final_text) => {
        self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            GroupReplyStatus::Done,
            Some(final_text.clone()),
            Some(None),
            true,
          )
          .await;
        Some(final_text)
      }
      Err(err_msg) => {
        self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            GroupReplyStatus::Error,
            None,
            Some(Some(err_msg)),
            true,
          )
          .await;
        None
      }
    }
  }

  #[cfg(desktop)]
  async fn wake_and_settle_group_webview(wv: &tauri::Webview) {
    let wake_js = r#"(() => {
      try {
        window.dispatchEvent(new Event('focus'));
        document.dispatchEvent(new Event('visibilitychange'));
        window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        const txt = (document.body && document.body.innerText) || '';
        const connecting = /(?:正在连接|连接中|Connecting|Reconnecting|离线|重连中)/i.test(txt.slice(0, 600));
        return JSON.stringify({ connecting });
      } catch (_) {
        return JSON.stringify({ connecting: false });
      }
    })()"#;
    let first = Self::eval_json_callback(wv, wake_js).await;
    tokio::time::sleep(std::time::Duration::from_millis(380)).await;
    if first
      .and_then(|v| v.get("connecting").and_then(|b| b.as_bool()))
      .unwrap_or(false)
    {
      for _ in 0..7 {
        tokio::time::sleep(std::time::Duration::from_millis(450)).await;
        let still = Self::eval_json_callback(wv, wake_js)
          .await
          .and_then(|v| v.get("connecting").and_then(|b| b.as_bool()))
          .unwrap_or(false);
        if !still {
          break;
        }
      }
    }
  }

  #[cfg(desktop)]
  async fn discard_hung_group_webview(&self, app: &AppHandle, group_id: &str, key: &AccountKey) {
    let mut locked = self.inner.lock().await;
    locked
      .group_context_synced
      .remove(&(group_id.to_string(), key.clone()));
    if locked.active_slot.as_ref().map(|s| &s.account_key) != Some(key) {
      if let Some(entry) = locked.warm_pool.remove(key) {
        Self::close_warm_entry(app, entry);
      }
    }
  }

  #[cfg(desktop)]
  async fn try_attach_file_in_webview(
    wv: &tauri::Webview,
    adapter: &SiteAdapterConfig,
    attachment: &GroupAttachment,
  ) -> bool {
    let attach_js = build_group_attach_file_js(adapter, attachment);
    let dispatched = Self::eval_json_callback(wv, &attach_js)
      .await
      .and_then(|v| v.get("dispatched").and_then(|b| b.as_bool()))
      .unwrap_or(false);
    if !dispatched {
      return false;
    }
    let check_js = build_group_check_attached_js(adapter, &attachment.filename);
    for _ in 0..6 {
      tokio::time::sleep(std::time::Duration::from_millis(400)).await;
      if Self::eval_json_callback(wv, &check_js)
        .await
        .and_then(|v| v.get("ready").and_then(|b| b.as_bool()))
        .unwrap_or(false)
      {
        return true;
      }
    }
    false
  }

  #[cfg(desktop)]
  async fn extract_hark_expanded_card_if_needed(
    &self,
    wv: &tauri::Webview,
    preview_text: &str,
  ) -> String {
    let click_js = build_hark_click_preview_card_js(preview_text);
    let Some(click_res) = Self::eval_json_callback(wv, &click_js).await else {
      return preview_text.to_string();
    };
    if !click_res
      .get("clicked")
      .and_then(|v| v.as_bool())
      .unwrap_or(false)
    {
      return preview_text.to_string();
    }

    let intro_prefix = click_res
      .get("introPrefix")
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    let card_preview = click_res
      .get("cardPreview")
      .and_then(|v| v.as_str())
      .unwrap_or(preview_text)
      .trim()
      .to_string();

    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let read_js = build_hark_read_expanded_js(&card_preview);
    let mut best_expanded = String::new();
    let mut stable_hits = 0usize;
    for _ in 0..8 {
      if let Some(res) = Self::eval_json_callback(wv, &read_js).await {
        let cur = res
          .get("text")
          .and_then(|v| v.as_str())
          .map(Self::clean_extracted_reply_text)
          .unwrap_or_default();
        if cur.chars().count() > card_preview.chars().count() + 24 {
          if cur == best_expanded {
            stable_hits += 1;
            if stable_hits >= 1 {
              break;
            }
          } else {
            best_expanded = cur;
            stable_hits = 0;
          }
        }
      }
      tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    }

    let _ = Self::eval_json_callback(wv, &build_hark_close_expanded_js()).await;
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;

    if best_expanded.chars().count() <= card_preview.chars().count() + 24 {
      return preview_text.to_string();
    }
    if intro_prefix.is_empty() {
      best_expanded
    } else if best_expanded.starts_with(&intro_prefix) {
      best_expanded
    } else {
      format!("{intro_prefix}\n\n{best_expanded}")
    }
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  async fn execute_webview_member_turn(
    self: &Arc<Self>,
    app: &AppHandle,
    group_id: &str,
    member_key: &AccountKey,
    msg_id: &str,
    user_msg_id: &str,
    raw_user_content: &str,
    stage_instruction: Option<&str>,
    fallback_stage_instruction: Option<&str>,
    attachment: Option<&GroupAttachment>,
    hop_depth: u8,
  ) -> Option<String> {
    if !self
      .update_group_message_state(
        app,
        group_id,
        msg_id,
        GroupReplyStatus::Sending,
        None,
        Some(None),
        false,
      )
      .await
    {
      return None;
    }

    let (mut wv, mut view_instance_id) =
      match self.ensure_group_member_webview_ready(app, member_key).await {
        Ok(pair) => pair,
        Err(err) => {
          self
            .update_group_message_state(
              app,
              group_id,
              msg_id,
              GroupReplyStatus::Error,
              Some(String::new()),
              Some(Some(err)),
              true,
            )
            .await;
          return None;
        }
      };

    self
      .prepare_group_member_thread(group_id, member_key, &wv)
      .await;

    for retry_round in 0..2usize {
      Self::wake_and_settle_group_webview(&wv).await;

      let Some((_, _, prompt_text, fingerprint)) = self
        .build_turn_prompt_for_member(
          group_id,
          member_key,
          view_instance_id,
          msg_id,
          user_msg_id,
          raw_user_content,
          stage_instruction,
          hop_depth,
        )
        .await
      else {
        return None;
      };

      let (adapter, pre_send_norm) = {
        let locked = self.inner.lock().await;
        let app_data_dir = locked.app_data_dir.clone();
        let url_opt = wv.url().ok();
        let host = url_opt
          .as_ref()
          .and_then(|u| u.host_str())
          .unwrap_or("")
          .to_string();
        let pre = url_opt.as_ref().map(Self::normalize_url_for_thread);
        (
          site_adapters::resolve_adapter_for_host(&app_data_dir, &host),
          pre,
        )
      };

      let mut effective_stage_instruction = stage_instruction;
      let mut effective_prompt_text = prompt_text;
      if let Some(att) = attachment {
        let attached_ok = Self::try_attach_file_in_webview(&wv, &adapter, att).await;
        if !attached_ok {
          if let Some(fb_stage) = fallback_stage_instruction {
            effective_stage_instruction = Some(fb_stage);
            if let Some((_, _, fb_prompt, _)) = self
              .build_turn_prompt_for_member(
                group_id,
                member_key,
                view_instance_id,
                msg_id,
                user_msg_id,
                raw_user_content,
                effective_stage_instruction,
                hop_depth,
              )
              .await
            {
              effective_prompt_text = fb_prompt;
            }
          }
        }
      }

      let mut send_js = build_group_send_js(&adapter, msg_id, &effective_prompt_text);
      let mut send_res_opt = None;
      let mut send_eval_misses = 0usize;
      let mut recovered_dead_wv = false;
      for attempt in 0..24 {
        let send_res = match Self::eval_json_callback(&wv, &send_js).await {
          Some(v) => {
            send_eval_misses = 0;
            v
          }
          None => {
            send_eval_misses += 1;
            if !recovered_dead_wv && (app.get_webview(wv.label()).is_none() || send_eval_misses >= 4)
            {
              recovered_dead_wv = true;
              send_eval_misses = 0;
              self
                .discard_hung_group_webview(app, group_id, member_key)
                .await;
              if let Ok((new_wv, new_vid)) =
                self.ensure_group_member_webview_ready(app, member_key).await
              {
                wv = new_wv;
                view_instance_id = new_vid;
                Self::wake_and_settle_group_webview(&wv).await;
                if let Some((_, _, rebuilt, _)) = self
                  .build_turn_prompt_for_member(
                    group_id,
                    member_key,
                    view_instance_id,
                    msg_id,
                    user_msg_id,
                    raw_user_content,
                    effective_stage_instruction,
                    hop_depth,
                  )
                  .await
                {
                  send_js = build_group_send_js(&adapter, msg_id, &rebuilt);
                }
                continue;
              }
            }
            if app.get_webview(wv.label()).is_none() || send_eval_misses >= 6 || attempt == 23 {
              self
                .update_group_message_state(
                  app,
                  group_id,
                  msg_id,
                  GroupReplyStatus::Error,
                  Some(String::new()),
                  Some(Some("无法向该账号页面注入消息脚本".to_string())),
                  true,
                )
                .await;
              return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            continue;
          }
        };
        let ok = send_res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let status_str = send_res.get("status").and_then(|v| v.as_str()).unwrap_or("error");
        if ok || status_str != "no_composer" || attempt == 23 {
          send_res_opt = Some(send_res);
          break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
      }
      let send_res = send_res_opt?;

      if !send_res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let status_str = send_res.get("status").and_then(|v| v.as_str()).unwrap_or("error");
        let err_msg = send_res
          .get("error")
          .and_then(|v| v.as_str())
          .unwrap_or("向该账号发送消息失败")
          .to_string();
        let final_status = if status_str == "logged_out" || status_str == "needs_auth" {
          self
            .apply_logged_out_state(app, member_key.clone(), view_instance_id)
            .await;
          GroupReplyStatus::NeedsAuth
        } else if status_str == "page_error" {
          self
            .apply_page_error_state(app, member_key.clone(), view_instance_id, err_msg.clone())
            .await;
          GroupReplyStatus::Error
        } else {
          GroupReplyStatus::Error
        };
        self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            final_status,
            Some(String::new()),
            Some(Some(err_msg)),
            true,
          )
          .await;
        return None;
      }

      {
        let mut locked = self.inner.lock().await;
        locked.group_context_synced.insert(
          (group_id.to_string(), member_key.clone()),
          (view_instance_id, fingerprint),
        );
      }

      if !self
        .update_group_message_state(
          app,
          group_id,
          msg_id,
          GroupReplyStatus::Streaming,
          Some(String::new()),
          Some(None),
          false,
        )
        .await
      {
        return None;
      }

      let poll_js = build_group_poll_js(&adapter, msg_id);
      let mut last_pushed_text = String::new();
      let mut last_pushed_is_transient = false;
      let mut poll_eval_misses = 0usize;
      let mut trigger_reload_retry = false;
      for _ in 0..420 {
        tokio::time::sleep(std::time::Duration::from_millis(450)).await;
        let res = match Self::eval_json_callback(&wv, &poll_js).await {
          Some(v) => {
            poll_eval_misses = 0;
            v
          }
          None => {
            poll_eval_misses += 1;
            if app.get_webview(wv.label()).is_some() && poll_eval_misses < 6 {
              continue;
            }
            if retry_round == 0 && last_pushed_text.is_empty() {
              trigger_reload_retry = true;
              break;
            }
            self
              .update_group_message_state(
                app,
                group_id,
                msg_id,
                GroupReplyStatus::Error,
                Some(if last_pushed_is_transient {
                  String::new()
                } else {
                  last_pushed_text
                }),
                Some(Some("页面视图已关闭或刷新，连接中断".to_string())),
                true,
              )
              .await;
            return None;
          }
        };

        let state = res.get("state").and_then(|v| v.as_str()).unwrap_or("streaming");
        let is_transient = res.get("transient").and_then(|v| v.as_bool()).unwrap_or(false);
        let raw_text = res
          .get("text")
          .and_then(|v| v.as_str())
          .unwrap_or("")
          .trim()
          .to_string();
        let text = if is_transient {
          raw_text
        } else {
          Self::clean_extracted_reply_text(&raw_text)
        };
        let err_opt = res
          .get("error")
          .and_then(|v| v.as_str())
          .map(|s| s.trim().to_string())
          .filter(|s| !s.is_empty());

        match state {
          "stalled_retry" if retry_round == 0 => {
            trigger_reload_retry = true;
            break;
          }
          "stalled_retry" => {
            self
              .update_group_message_state(
                app,
                group_id,
                msg_id,
                GroupReplyStatus::Error,
                Some(String::new()),
                Some(Some("发送后页面仍在发送中未发出，请切换至该账号页面检查网络后重试".to_string())),
                true,
              )
              .await;
            return None;
          }
          "done" => {
            let mut final_text = if !text.is_empty() {
              text
            } else if !last_pushed_is_transient {
              last_pushed_text
            } else {
              String::new()
            };
            if adapter.id == "hark" && !final_text.trim().is_empty() {
              final_text = self.extract_hark_expanded_card_if_needed(&wv, &final_text).await;
            }
            self
              .update_group_message_state(
                app,
                group_id,
                msg_id,
                GroupReplyStatus::Done,
                Some(final_text.clone()),
                Some(None),
                true,
              )
              .await;
            self
              .persist_group_thread_binding(
                app,
                group_id,
                member_key,
                pre_send_norm.as_deref(),
                &wv,
                &adapter,
              )
              .await;
            return Some(final_text);
          }
          "needs_auth" => {
            let final_text = if !text.is_empty() {
              text
            } else if !last_pushed_is_transient {
              last_pushed_text
            } else {
              String::new()
            };
            self
              .update_group_message_state(
                app,
                group_id,
                msg_id,
                GroupReplyStatus::NeedsAuth,
                Some(final_text),
                Some(Some(
                  err_opt.unwrap_or_else(|| "检测到授权确认弹窗，请前往该账号页面点击允许".to_string()),
                )),
                true,
              )
              .await;
            return None;
          }
          "error" => {
            let final_text = if !text.is_empty() {
              text
            } else if !last_pushed_is_transient {
              last_pushed_text
            } else {
              String::new()
            };
            self
              .update_group_message_state(
                app,
                group_id,
                msg_id,
                GroupReplyStatus::Error,
                Some(final_text),
                Some(Some(err_opt.unwrap_or_else(|| "生成回复失败".to_string()))),
                true,
              )
              .await;
            return None;
          }
          _ => {
            if !is_transient && !text.is_empty() && text != last_pushed_text {
              last_pushed_text = text.clone();
              last_pushed_is_transient = false;
              if !self
                .update_group_message_state(
                  app,
                  group_id,
                  msg_id,
                  GroupReplyStatus::Streaming,
                  Some(text),
                  None,
                  false,
                )
                .await
              {
                return None;
              }
            }
          }
        }
      }

      if trigger_reload_retry {
        {
          let mut locked = self.inner.lock().await;
          locked
            .group_context_synced
            .remove(&(group_id.to_string(), member_key.clone()));
        }
        let _ = self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            GroupReplyStatus::Sending,
            Some(String::new()),
            Some(None),
            false,
          )
          .await;
        if app.get_webview(wv.label()).is_some() {
          let _ = wv.eval("window.location.reload()");
          Self::wait_webview_settled(&wv, 18).await;
          tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        } else {
          self
            .discard_hung_group_webview(app, group_id, member_key)
            .await;
          if let Ok((new_wv, new_vid)) =
            self.ensure_group_member_webview_ready(app, member_key).await
          {
            wv = new_wv;
            view_instance_id = new_vid;
          } else {
            break;
          }
        }
        continue;
      }

      if !last_pushed_text.is_empty() && !last_pushed_is_transient {
        let final_text = if adapter.id == "hark" {
          self
            .extract_hark_expanded_card_if_needed(&wv, &last_pushed_text)
            .await
        } else {
          last_pushed_text
        };
        self
          .update_group_message_state(
            app,
            group_id,
            msg_id,
            GroupReplyStatus::Done,
            Some(final_text.clone()),
            Some(None),
            true,
          )
          .await;
        self
          .persist_group_thread_binding(
            app,
            group_id,
            member_key,
            pre_send_norm.as_deref(),
            &wv,
            &adapter,
          )
          .await;
        return Some(final_text);
      }
      break;
    }

    self
      .update_group_message_state(
        app,
        group_id,
        msg_id,
        GroupReplyStatus::Error,
        Some(String::new()),
        Some(Some("等待回复超时，请切换至该账号页面查看详情".to_string())),
        true,
      )
      .await;
    None
  }

  #[cfg_attr(not(desktop), allow(dead_code))]
  fn select_a2a_relay_targets(
    reply_text: &str,
    speaker: &AccountKey,
    eligible_members: &[(AccountKey, String)],
    visited: &[AccountKey],
    max_per_hop: usize,
  ) -> Vec<AccountKey> {
    let (mention_all, mentioned) = Self::parse_at_mentions(reply_text, eligible_members);
    let candidates: Vec<AccountKey> = if mention_all {
      eligible_members.iter().map(|(k, _)| k.clone()).collect()
    } else {
      mentioned
    };
    candidates
      .into_iter()
      .filter(|k| k != speaker && !visited.contains(k))
      .take(max_per_hop)
      .collect()
  }

  #[cfg(desktop)]
  async fn run_group_review_pipeline(
    self: Arc<Self>,
    app: AppHandle,
    group_id: String,
    user_msg_id: String,
    raw_user_content: String,
    jobs: Vec<(usize, AccountKey, String, Option<String>, Option<ProviderId>)>,
  ) {
    let mut completed: Vec<(String, String)> = Vec::new();
    for (pos, (idx, member_key, msg_id, stage_label, auto_ch)) in jobs.iter().enumerate() {
      let is_hark = member_key.provider_id.is_hark();
      let is_external = member_key.provider_id.is_external();
      let (stage_inst, fallback_stage_inst, attachment) = if *idx == 0 {
        let s = if is_hark {
          "For the “Current question”, give a clear, complete initial plan with the key reasons, and output it in full in the body of your reply — do not omit anything.".to_string()
        } else {
          "For the “Current question”, give a clear, complete initial plan with the key reasons.".to_string()
        };
        (s, None, None)
      } else {
        let full_prev_summary = completed
          .iter()
          .enumerate()
          .map(|(i, (speaker, text))| {
            let clean_label = if is_hark {
              format!("Draft point {}", i + 1)
            } else if speaker.contains('@') {
              format!("Stage {} draft", i + 1)
            } else {
              format!("Previous draft ({speaker})")
            };
            format!("### {clean_label}\n{}", text.trim())
          })
          .collect::<Vec<_>>()
          .join("\n\n");

        let format_with_body = |body: &str| -> String {
          if is_hark {
            if stage_label.as_deref() == Some("Final draft") {
              format!(
                "Using the draft plan and suggestions below, put together the final, directly actionable conclusion for me, and output it in full in the body of your reply — do not omit anything:\n{body}"
              )
            } else {
              format!(
                "Here is a draft plan that needs work. Please point out its flaws, boundary risks, and omissions directly, and give specific suggestions for fixing them, outputting everything in full in the body of your reply — do not omit anything:\n{body}"
              )
            }
          } else if stage_label.as_deref() == Some("Final draft") {
            format!("Using the drafts and review comments below, combine them into a final, directly actionable conclusion:\n{body}")
          } else {
            format!("Check the drafts below for flaws, boundary risks, and omissions, and give specific suggestions for fixing them:\n{body}")
          }
        };

        let char_count = full_prev_summary.chars().count();
        if is_external || char_count <= REVIEW_INLINE_CHAR_LIMIT {
          (format_with_body(&full_prev_summary), None, None)
        } else {
          let filename = format!("review-stage-{pos}.md");
          let attachment_note = if is_hark {
            if stage_label.as_deref() == Some("Final draft") {
              format!(
                "Please read the attached `{filename}` document in full (it contains the earlier plan and suggestions), and put together the final, directly actionable conclusion for me, outputting it in full in the body of your reply — do not omit anything."
              )
            } else {
              format!(
                "Please read the attached `{filename}` document in full (it contains the draft plan that needs work), and point out its flaws, boundary risks, and omissions directly, giving specific suggestions for fixing them, outputting everything in full in the body of your reply — do not omit anything."
              )
            }
          } else if stage_label.as_deref() == Some("Final draft") {
            format!(
              "Please read the drafts and review comments in the attached `{filename}` in full, and combine them into a final, directly actionable conclusion."
            )
          } else {
            format!(
              "Please read the earlier plan in the attached `{filename}` in full, and check it for flaws, boundary risks, and omissions, giving specific suggestions for fixing them."
            )
          };
          let truncated_body: String = full_prev_summary
            .chars()
            .take(REVIEW_FALLBACK_CHAR_LIMIT)
            .collect();
          let fallback_body = if char_count > REVIEW_FALLBACK_CHAR_LIMIT {
            format!("{truncated_body}\n...(further content omitted because it was too long)")
          } else {
            truncated_body
          };
          (
            attachment_note,
            Some(format_with_body(&fallback_body)),
            Some(GroupAttachment {
              filename,
              mime_type: "text/markdown".to_string(),
              content: full_prev_summary,
            }),
          )
        }
      };

      let reply_opt = self
        .run_group_member_turn_with_failover(
          &app,
          &group_id,
          member_key,
          msg_id,
          &user_msg_id,
          &raw_user_content,
          Some(&stage_inst),
          fallback_stage_inst.as_deref(),
          attachment.as_ref(),
          0,
          auto_ch.as_ref(),
        )
        .await;

      if let Some((actual_key, reply_text)) = reply_opt {
        let alias = {
          let locked = self.inner.lock().await;
          Self::resolve_member_alias_locked(&locked, &actual_key)
            .unwrap_or_else(|| actual_key.uuid.chars().take(6).collect())
        };
        completed.push((alias, reply_text));
      } else {
        for (_, _, rem_msg_id, _, _) in &jobs[pos + 1..] {
          self
            .update_group_message_state(
              &app,
              &group_id,
              rem_msg_id,
              GroupReplyStatus::Error,
              None,
              Some(Some("A previous review stage did not finish, so the remaining stages were stopped.".to_string())),
              true,
            )
            .await;
        }
        break;
      }
    }
  }

  #[cfg(desktop)]
  #[allow(clippy::too_many_arguments)]
  fn run_group_a2a_chain(
    self: Arc<Self>,
    app: AppHandle,
    group_id: String,
    member_key: AccountKey,
    msg_id: String,
    user_msg_id: String,
    raw_user_content: String,
    hop_depth: u8,
    visited: Vec<AccountKey>,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
    let stage_hint = if hop_depth > 0 {
      Some(format!("A2A relay hop {hop_depth}"))
    } else {
      None
    };
    let auto_ch = if hop_depth == 0 {
      let locked = self.inner.lock().await;
      locked
        .store
        .group_chats
        .iter()
        .find(|g| g.id == group_id)
        .and_then(|g| g.messages.iter().find(|m| m.id == msg_id))
        .and_then(|m| m.auto_routed_channel.as_deref())
        .and_then(ProviderId::parse)
    } else {
      None
    };
    let Some((actual_key, reply_text)) = self
      .run_group_member_turn_with_failover(
        &app,
        &group_id,
        &member_key,
        &msg_id,
        &user_msg_id,
        &raw_user_content,
        stage_hint.as_deref(),
        None,
        None,
        hop_depth,
        auto_ch.as_ref(),
      )
      .await
    else {
      return;
    };

    if reply_text.trim().is_empty() {
      return;
    }

    let mut active_visited = visited;
    if !active_visited.contains(&actual_key) {
      active_visited.push(actual_key.clone());
    }

    let next_hops = {
      let mut locked = self.inner.lock().await;
      let Some(g_idx) = locked
        .store
        .group_chats
        .iter()
        .position(|g| g.id == group_id)
      else {
        return;
      };
      if locked.store.group_chats[g_idx].workflow_mode != GroupWorkflowMode::A2aRelay
        || hop_depth >= locked.store.group_chats[g_idx].max_relay_hops.clamp(1, 3)
      {
        return;
      }
      let eligible_members: Vec<(AccountKey, String)> = locked.store.group_chats[g_idx]
        .members
        .iter()
        .filter_map(|m| {
          if m.provider_id.is_external() {
            locked
              .store
              .external_agents
              .iter()
              .find(|e| &e.account_key() == m)
              .map(|e| (m.clone(), e.name.clone()))
          } else {
            let now = now_ms();
            locked
              .store
              .accounts
              .iter()
              .find(|a| a.provider_id == m.provider_id && a.uuid == m.uuid)
              .filter(|a| a.auth_state != AuthState::LoggedOut && a.cookie_expires_at_ms.is_none_or(|exp| exp > now))
              .filter(|a| resolve_provider(&a.provider_id).is_some_and(|s| s.info.enabled))
              .map(|a| (m.clone(), a.alias.clone()))
          }
        })
        .collect();

      let next_targets = Self::select_a2a_relay_targets(
        &reply_text,
        &actual_key,
        &eligible_members,
        &active_visited,
        2,
      );
      if next_targets.is_empty() {
        return;
      }

      let speaker_alias = Self::resolve_member_alias_locked(&locked, &actual_key)
        .unwrap_or_else(|| actual_key.uuid.chars().take(6).collect());
      let next_hop = hop_depth + 1;
      let stage_label = format!("A2A relay #{next_hop}");
      let mut next_visited = active_visited.clone();
      for k in &next_targets {
        if !next_visited.contains(k) {
          next_visited.push(k.clone());
        }
      }

      let ts = now_ms();
      let snippet: String = reply_text.trim().chars().take(600).collect();
      let relay_prompt = format!(
        "Please follow up on the previous hop’s collaboration result from “{speaker_alias}”:\n{snippet}"
      );
      let mut spawned = Vec::with_capacity(next_targets.len());
      for (offset, target_key) in next_targets.into_iter().enumerate() {
        let alias = Self::resolve_member_alias_locked(&locked, &target_key)
          .unwrap_or_else(|| target_key.uuid.chars().take(6).collect());
        let reply_msg_id = Uuid::new_v4().hyphenated().to_string();
        locked.store.group_chats[g_idx].messages.push(GroupChatMessage {
          id: reply_msg_id.clone(),
          role: "assistant".to_string(),
          sender: Some(target_key.clone()),
          sender_name: alias,
          content: String::new(),
          created_at_ms: ts + (offset as u64) + 1,
          status: GroupReplyStatus::Queued,
          targets: Vec::new(),
          error: None,
          stage_label: Some(stage_label.clone()),
          hop_depth: next_hop,
          auto_routed_channel: None,
        });
        spawned.push((target_key, reply_msg_id, relay_prompt.clone(), next_visited.clone()));
      }
      if save_store_atomic(&locked.store_path, &locked.store).is_err() {
        return;
      }
      let snap = Self::snapshot_from_locked(&locked);
      Some((snap, spawned, next_hop))
    };

    if let Some((snap, spawned, next_hop)) = next_hops {
      self.emit_snapshot(&app, &snap);
      for (target_key, new_msg_id, relay_prompt, next_visited) in spawned {
        let mgr = Arc::clone(&self);
        let app_h = app.clone();
        let gid = group_id.clone();
        let uid = user_msg_id.clone();
        tauri::async_runtime::spawn(mgr.run_group_a2a_chain(
          app_h,
          gid,
          target_key,
          new_msg_id,
          uid,
          relay_prompt,
          next_hop,
          next_visited,
        ));
      }
    }
    })
  }
}

#[cfg(desktop)]
struct DesktopProbeResult {
  is_auth: bool,
  logged_out: bool,
  stale_session: bool,
  page_error: String,
  typed: String,
  email: String,
  display_name: String,
  avatar: String,
  quota_text: String,
  quota_pct: Option<u8>,
  quota_detail: Option<QuotaDetail>,
  pending_hotkey: Option<u8>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::storage::save_store_atomic;

  fn make_test_account(provider_id: ProviderId, uuid: &str, alias: &str) -> AccountRecord {
    AccountRecord {
      provider_id,
      uuid: uuid.to_string(),
      alias: alias.to_string(),
      created_at_ms: 1,
      auto_alias: false,
      proxy_url: None,
      timezone: None,
      avatar_url: None,
      quota_text: None,
      quota_percent: None,
      quota_detail: None,
      auth_state: AuthState::Unknown,
      cookie_expires_at_ms: None,
      cookie_session_only: false,
    }
  }

  #[tokio::test]
  async fn reorder_accounts_updates_only_target_provider_and_validates() {
    let temp_dir = std::env::temp_dir().join(format!("maam-reorder-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let store_path = temp_dir.join("accounts.json");

    let m1 = "00000000-0000-4000-8000-000000000001";
    let h1 = "00000000-0000-4000-8000-000000000002";
    let m2 = "00000000-0000-4000-8000-000000000003";
    let m3 = "00000000-0000-4000-8000-000000000004";
    let h2 = "00000000-0000-4000-8000-000000000005";

    let initial = AccountsStoreFile {
      version: crate::storage::STORE_VERSION,
      accounts: vec![
        make_test_account(ProviderId::muse(), m1, "m1"),
        make_test_account(ProviderId::hark(), h1, "h1"),
        make_test_account(ProviderId::muse(), m2, "m2"),
        make_test_account(ProviderId::muse(), m3, "m3"),
        make_test_account(ProviderId::hark(), h2, "h2"),
      ],
      last_committed: None,
      pending_cleanup: Vec::new(),
      group_chats: Vec::new(),
      external_agents: Vec::new(),
      settings: AppSettings::default(),
    };
    save_store_atomic(&store_path, &initial).unwrap();

    let mgr = SessionManager::new(temp_dir.clone());
    let snap = mgr
      .reorder_accounts("muse", vec![m3.to_string(), m1.to_string(), m2.to_string()])
      .await
      .unwrap();
    let aliases: Vec<&str> = snap.accounts.iter().map(|a| a.alias.as_str()).collect();
    assert_eq!(aliases, vec!["m3", "h1", "m1", "m2", "h2"]);

    let reloaded = SessionManager::new(temp_dir.clone()).get_snapshot().await;
    let reloaded_aliases: Vec<&str> = reloaded.accounts.iter().map(|a| a.alias.as_str()).collect();
    assert_eq!(reloaded_aliases, vec!["m3", "h1", "m1", "m2", "h2"]);

    assert!(mgr
      .reorder_accounts("muse", vec![m1.to_string(), m2.to_string()])
      .await
      .is_err());
    assert!(mgr
      .reorder_accounts("muse", vec![m1.to_string(), m1.to_string(), m3.to_string()])
      .await
      .is_err());
    assert!(mgr
      .reorder_accounts("muse", vec![m1.to_string(), h1.to_string(), m3.to_string()])
      .await
      .is_err());

    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[test]
  fn builtin_provider_navigation_policy() {
    let muse = resolve_provider(&ProviderId::muse()).unwrap();
    let google_oauth = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    assert_eq!(
      classify_top_level_url(&muse, &google_oauth, false),
      NavigationDecision::ExternalGoogleOAuth
    );
    let http_url = Url::parse("http://muse.ai/").unwrap();
    assert_eq!(
      classify_top_level_url(&muse, &http_url, false),
      NavigationDecision::Deny
    );
    let blob_null = Url::parse("blob:null/12345678-90ab-cdef-1234-567890abcdef").unwrap();
    assert_eq!(
      classify_top_level_url(&muse, &blob_null, false),
      NavigationDecision::AllowBlank
    );
    let space_asset = Url::parse("https://x.s.metaaiusercontent.com/a").unwrap();
    assert_eq!(
      classify_top_level_url(&muse, &space_asset, false),
      NavigationDecision::AllowHttps
    );
    let space_worker = Url::parse("https://app.meta-agents-apps.workers.dev/").unwrap();
    assert_eq!(
      classify_top_level_url(&muse, &space_worker, false),
      NavigationDecision::AllowHttps
    );
  }

  #[tokio::test]
  async fn group_chat_eligibility_and_member_validation() {
    let temp_dir = std::env::temp_dir().join(format!("maam-group-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let store_path = temp_dir.join("accounts.json");

    let valid_uuid = "00000000-0000-4000-8000-000000000101";
    let expired_uuid = "00000000-0000-4000-8000-000000000102";
    let logged_out_uuid = "00000000-0000-4000-8000-000000000103";

    let mut valid_acc = make_test_account(ProviderId::muse(), valid_uuid, "Muse1");
    valid_acc.auth_state = AuthState::Authenticated;
    valid_acc.cookie_expires_at_ms = Some(now_ms() + 86_400_000);

    let mut expired_acc = make_test_account(ProviderId::hark(), expired_uuid, "HarkExpired");
    expired_acc.auth_state = AuthState::Authenticated;
    expired_acc.cookie_expires_at_ms = Some(1_000);

    let mut logged_out_acc = make_test_account(ProviderId::muse(), logged_out_uuid, "Out");
    logged_out_acc.auth_state = AuthState::LoggedOut;

    let initial = AccountsStoreFile {
      version: crate::storage::STORE_VERSION,
      accounts: vec![valid_acc.clone(), expired_acc.clone(), logged_out_acc.clone()],
      last_committed: None,
      pending_cleanup: Vec::new(),
      group_chats: Vec::new(),
      external_agents: Vec::new(),
      settings: AppSettings::default(),
    };
    save_store_atomic(&store_path, &initial).unwrap();

    let mgr = SessionManager::new(temp_dir.clone());
    let locked = mgr.inner.lock().await;
    let valid_key = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: valid_uuid.to_string(),
    };
    let expired_key = AccountKey {
      provider_id: ProviderId::hark(),
      uuid: expired_uuid.to_string(),
    };
    let out_key = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: logged_out_uuid.to_string(),
    };

    let deduped = SessionManager::normalize_group_members(
      &locked,
      vec![valid_key.clone(), valid_key.clone()],
      None,
    )
    .unwrap();
    assert_eq!(deduped, vec![valid_key]);
    assert!(SessionManager::normalize_group_members(&locked, vec![expired_key], None).is_err());
    assert!(SessionManager::normalize_group_members(&locked, vec![out_key], None).is_err());
    assert!(SessionManager::normalize_group_members(&locked, vec![], None).is_err());

    drop(locked);
    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[test]
  fn parse_at_mentions_resolves_order_longest_match_and_all() {
    let k1 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000001".to_string(),
    };
    let k2 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000002".to_string(),
    };
    let k3 = AccountKey {
      provider_id: ProviderId::hark(),
      uuid: "00000000-0000-4000-8000-000000000003".to_string(),
    };
    let members = vec![
      (k1.clone(), "m1".to_string()),
      (k2.clone(), "m10".to_string()),
      (k3.clone(), "小脑袋".to_string()),
    ];

    assert_eq!(
      SessionManager::parse_at_mentions("test @m10 请和 @小脑袋帮我看看 @m1 @m10", &members),
      (false, vec![k2.clone(), k3.clone(), k1.clone()])
    );
    assert_eq!(
      SessionManager::parse_at_mentions("email test@m1.com 不应触发", &members),
      (false, vec![])
    );
    assert_eq!(
      SessionManager::parse_at_mentions("请 @全体成员 给出结论", &members),
      (true, vec![])
    );
    assert_eq!(
      SessionManager::parse_at_mentions("@all 总结", &members),
      (true, vec![])
    );
  }

  #[cfg(desktop)]
  #[tokio::test]
  async fn group_turn_prompt_differential_sync_relay_and_rename() {
    let temp_dir = std::env::temp_dir().join(format!("maam-group-prompt-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let store_path = temp_dir.join("accounts.json");

    let u1 = "00000000-0000-4000-8000-000000000201";
    let u2 = "00000000-0000-4000-8000-000000000202";
    let u_removed = "00000000-0000-4000-8000-000000000203";

    let mut a1 = make_test_account(ProviderId::muse(), u1, "架构师-小明");
    a1.auth_state = AuthState::Authenticated;
    let mut a2 = make_test_account(ProviderId::hark(), u2, "代码审查-小红");
    a2.auth_state = AuthState::Authenticated;

    let k1 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: u1.to_string(),
    };
    let k2 = AccountKey {
      provider_id: ProviderId::hark(),
      uuid: u2.to_string(),
    };
    let k_removed = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: u_removed.to_string(),
    };

    let group_id = "grp-prompt-1".to_string();
    let group = GroupChatRecord {
      id: group_id.clone(),
      name: "架构评审群".to_string(),
      members: vec![k1.clone(), k2.clone()],
      created_at_ms: 100,
      shared_context: "核心约束：低内存，禁止写注释".to_string(),
      shared_skills: vec![
        GroupSharedSkill {
          id: "sk1".to_string(),
          name: "简洁输出".to_string(),
          description: String::new(),
          content: "只输出结论与风险".to_string(),
          enabled: true,
        },
        GroupSharedSkill {
          id: "sk2".to_string(),
          name: "已禁用".to_string(),
          description: String::new(),
          content: "不应注入此条技能".to_string(),
          enabled: false,
        },
      ],
      member_roles: vec![GroupMemberRole {
        member: k2.clone(),
        role: "审查并发与边界条件".to_string(),
      }],
      workflow_mode: GroupWorkflowMode::Parallel,
      max_relay_hops: 2,
      thread_bindings: Vec::new(),
      messages: vec![
        GroupChatMessage {
          id: "msg-old-removed".to_string(),
          role: "assistant".to_string(),
          sender: Some(k_removed),
          sender_name: "离职成员-旧名".to_string(),
          content: "历史保留正文内容".to_string(),
          created_at_ms: 90,
          status: GroupReplyStatus::Done,
          targets: Vec::new(),
          error: None,
          stage_label: None,
          hop_depth: 0,
          auto_routed_channel: None,
        },
        GroupChatMessage {
          id: "msg-u-1".to_string(),
          role: "user".to_string(),
          sender: None,
          sender_name: "Me".to_string(),
          content: "@架构师-小明 @代码审查-小红 请评审".to_string(),
          created_at_ms: 100,
          status: GroupReplyStatus::Done,
          targets: vec![k1.clone(), k2.clone()],
          error: None,
          stage_label: None,
          hop_depth: 0,
          auto_routed_channel: None,
        },
        GroupChatMessage {
          id: "msg-a-1".to_string(),
          role: "assistant".to_string(),
          sender: Some(k1.clone()),
          sender_name: "架构师-小明".to_string(),
          content: "建议保持上限 2 并发以控制内存".to_string(),
          created_at_ms: 101,
          status: GroupReplyStatus::Done,
          targets: Vec::new(),
          error: None,
          stage_label: None,
          hop_depth: 0,
          auto_routed_channel: None,
        },
        GroupChatMessage {
          id: "msg-a-2".to_string(),
          role: "assistant".to_string(),
          sender: Some(k2.clone()),
          sender_name: "代码审查-小红".to_string(),
          content: String::new(),
          created_at_ms: 102,
          status: GroupReplyStatus::Queued,
          targets: Vec::new(),
          error: None,
          stage_label: None,
          hop_depth: 0,
          auto_routed_channel: None,
        },
      ],
    };

    let initial = AccountsStoreFile {
      version: crate::storage::STORE_VERSION,
      accounts: vec![a1, a2],
      last_committed: None,
      pending_cleanup: Vec::new(),
      group_chats: vec![group],
      external_agents: Vec::new(),
      settings: AppSettings::default(),
    };
    save_store_atomic(&store_path, &initial).unwrap();

    let mgr = SessionManager::new(temp_dir.clone());

    let (_, _, p1, fp1) = mgr
      .build_turn_prompt_for_member(
        &group_id,
        &k2,
        11,
        "msg-a-2",
        "msg-u-1",
        "@架构师-小明 @代码审查-小红 请评审",
        None,
        0,
      )
      .await
      .unwrap();
    assert!(p1.contains("Please answer mainly from the perspective of “审查并发与边界条件”."));
    assert!(p1.contains("Reference background:\n核心约束：低内存，禁止写注释"));
    assert!(p1.contains("Answer requirements:\n- “简洁输出”: 只输出结论与风险"));
    assert!(!p1.contains("不应注入此条技能"));
    assert!(!p1.contains("[Previous context]"));
    assert!(p1.contains("Question:\n请评审"));
    assert!(!p1.contains("@架构师-小明"));
    assert!(!p1.contains("协同"));
    assert!(!p1.contains("分工"));

    let email_members = vec![
      (k1.clone(), "ldjx7@foxmail.com".to_string()),
      (k2.clone(), "ldjx.tc@gmail.com".to_string()),
    ];
    let stripped = SessionManager::strip_routing_mentions(
      "@ldjx7@foxmail.com @ldjx.tc@gmail.com 帮我分析一下市面上有哪些渠道可以免费试用DeepSeekv",
      &email_members,
    );
    assert_eq!(stripped, "帮我分析一下市面上有哪些渠道可以免费试用DeepSeekv");
    assert_eq!(SessionManager::sanitize_history_assistant_text("仍在发送中\n空闲中"), None);
    assert_eq!(
      SessionManager::sanitize_history_assistant_text("我不和其他账号的助手一起协作分工，但直接找我能查资料"),
      Some("直接找我能查资料".to_string())
    );

    {
      let mut locked = mgr.inner.lock().await;
      locked
        .group_context_synced
        .insert((group_id.clone(), k2.clone()), (11, fp1));
    }
    let (_, _, p2, fp2) = mgr
      .build_turn_prompt_for_member(&group_id, &k2, 11, "msg-a-2", "msg-u-1", "继续确认", None, 0)
      .await
      .unwrap();
    assert_eq!(fp1, fp2);
    assert_eq!(p2, "继续确认");

    let (_, _, p3, _) = mgr
      .build_turn_prompt_for_member(&group_id, &k2, 22, "msg-a-2", "msg-u-1", "重连后继续", None, 0)
      .await
      .unwrap();
    assert!(p3.contains("Reference background:\n核心约束：低内存，禁止写注释"));

    let new_skill = vec![GroupSharedSkill {
      id: "sk1".to_string(),
      name: "简洁输出".to_string(),
      description: String::new(),
      content: "只输出结论与风险".to_string(),
      enabled: true,
    }];
    let new_roles = vec![GroupMemberRole {
      member: k2.clone(),
      role: "审查并发与边界条件".to_string(),
    }];
    mgr
      .update_group_knowledge(
        group_id.clone(),
        "新上下文：已通过压力测试".to_string(),
        new_skill,
        new_roles,
      )
      .await
      .unwrap();
    let (_, _, p4, _) = mgr
      .build_turn_prompt_for_member(&group_id, &k2, 11, "msg-a-2", "msg-u-1", "检查新上下文", None, 0)
      .await
      .unwrap();
    assert!(p4.contains("Reference background:\n新上下文：已通过压力测试"));

    {
      let mut locked = mgr.inner.lock().await;
      locked.store.accounts[0].alias = "资深架构-大明".to_string();
    }
    let (_, _, p5, _) = mgr
      .build_turn_prompt_for_member(
        &group_id,
        &k2,
        11,
        "msg-a-2",
        "msg-u-1",
        "复核备注同步",
        Some("请做交叉评审"),
        0,
      )
      .await
      .unwrap();
    assert!(p5.contains("[Stage requirements]\n请做交叉评审"));
    assert!(p5.contains("[Background]\n新上下文：已通过压力测试"));
    assert!(p5.contains("- Known reference: 建议保持上限 2 并发以控制内存"));
    assert!(p5.contains("- Known reference: 历史保留正文内容"));
    assert!(!p5.contains("资深架构-大明"));

    let (_, _, p5_muse, _) = mgr
      .build_turn_prompt_for_member(
        &group_id,
        &k1,
        11,
        "msg-a-2",
        "msg-u-1",
        "复核备注同步",
        Some("请做交叉评审"),
        0,
      )
      .await
      .unwrap();
    assert!(p5_muse.contains("- Reference (资深架构-大明): 建议保持上限 2 并发以控制内存"));
    assert!(p5_muse.contains("- Reference (离职成员-旧名): 历史保留正文内容"));

    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[cfg(desktop)]
  #[tokio::test]
  async fn hybrid_external_agents_a2a_anti_loop_and_thread_bindings() {
    let temp_dir = std::env::temp_dir().join(format!("maam-hybrid-group-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mgr = SessionManager::new(temp_dir.clone());

    let api_snap = mgr
      .upsert_external_agent(
        None,
        ExternalAgentKind::OpenaiApi,
        "DeepSeek-API".to_string(),
        "https://api.deepseek.com/v1".to_string(),
        "sk-demo".to_string(),
        "deepseek-chat".to_string(),
        String::new(),
        vec![],
        None,
      )
      .await
      .expect("create api agent");
    assert_eq!(api_snap.external_agents.len(), 1);
    let api_key = api_snap.external_agents[0].account_key();

    let cli_snap = mgr
      .upsert_external_agent(
        None,
        ExternalAgentKind::LocalCli,
        "ClaudeCLI".to_string(),
        String::new(),
        String::new(),
        String::new(),
        "claude".to_string(),
        vec!["-p".to_string()],
        None,
      )
      .await
      .expect("create cli agent");
    assert_eq!(cli_snap.external_agents.len(), 2);
    let cli_key = cli_snap.external_agents[1].account_key();

    let k3 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000333".to_string(),
    };
    let members = vec![
      (api_key.clone(), "DeepSeek-API".to_string()),
      (cli_key.clone(), "ClaudeCLI".to_string()),
      (k3.clone(), "Muse-C".to_string()),
    ];
    let picked = SessionManager::select_a2a_relay_targets(
      "我先给出初稿，请 @DeepSeek-API 自查无意义，请 @ClaudeCLI 复核，也可问 @Muse-C",
      &api_key,
      &members,
      &[api_key.clone()],
      2,
    );
    assert_eq!(picked, vec![cli_key.clone(), k3.clone()]);

    let visited_blocked = SessionManager::select_a2a_relay_targets(
      "再交回 @DeepSeek-API 或 @ClaudeCLI",
      &k3,
      &members,
      &[api_key.clone(), cli_key.clone(), k3.clone()],
      2,
    );
    assert!(visited_blocked.is_empty());

    let mut acc = make_test_account(ProviderId::muse(), &k3.uuid, "Muse-C");
    acc.auth_state = AuthState::Authenticated;
    {
      let mut locked = mgr.inner.lock().await;
      locked.store.accounts.push(acc);
      locked.store.group_chats.push(GroupChatRecord {
        id: "g-hybrid-1".to_string(),
        name: "混合群".to_string(),
        members: vec![api_key.clone(), cli_key.clone(), k3.clone()],
        created_at_ms: 1,
        messages: vec![],
        shared_context: "背景约束：低功耗".to_string(),
        shared_skills: vec![],
        member_roles: vec![],
        workflow_mode: GroupWorkflowMode::A2aRelay,
        max_relay_hops: 2,
        thread_bindings: vec![GroupThreadBinding {
          member: k3.clone(),
          thread_url: "https://muse.ai/c/bound-1".to_string(),
          updated_at_ms: 1,
        }],
      });
    }

    let (sys, usr, full, _) = mgr
      .build_turn_prompt_for_member("g-hybrid-1", &api_key, 0, "m1", "u1", "开始评审", None, 0)
      .await
      .unwrap();
    assert!(sys.contains("[Background]\n背景约束：低功耗"));
    assert!(usr.contains("[Relay note]"));
    assert!(full.contains("ClaudeCLI"));

    let wf_snap = mgr
      .update_group_workflow("g-hybrid-1".to_string(), GroupWorkflowMode::Review, 3)
      .await
      .unwrap();
    assert_eq!(wf_snap.group_chats[0].workflow_mode, GroupWorkflowMode::Review);
    assert_eq!(wf_snap.group_chats[0].max_relay_hops, 3);

    let reset_snap = mgr
      .reset_group_thread_binding("g-hybrid-1".to_string(), k3.clone())
      .await
      .unwrap();
    assert!(reset_snap.group_chats[0].thread_bindings.is_empty());

    let del_snap = mgr.delete_external_agent(api_key.uuid).await.unwrap();
    assert_eq!(del_snap.external_agents.len(), 1);
    assert!(!del_snap.group_chats[0].members.iter().any(|m| m.provider_id.is_api()));

    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[tokio::test]
  async fn evaluate_account_quota_handles_muse_extra_hark_caps_and_remaining_fallback() {
    let sanitized = SessionManager::sanitize_shared_context_for_prompt(
      "核心约束：低内存\n[本轮结论汇总 · 可以帮我做什么]\n- a@gmail.com: 回复一\n- b@x.com: 回复二\n- 备注: 保留",
    );
    assert_eq!(
      sanitized,
      "核心约束：低内存\nConclusions so far (topic: 可以帮我做什么)\n- Point 1: 回复一\n- Point 2: 回复二\n- 备注: 保留"
    );
    let mut muse_healthy = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000401",
      "Muse-Healthy",
    );
    muse_healthy.quota_detail = Some(QuotaDetail {
      plan_label: "5小时".to_string(),
      plan_value: "剩余 80%".to_string(),
      plan_subtext: None,
      plan_used_percent: Some(20),
      extra_label: None,
      extra_value: None,
      extra_subtext: None,
      extra_used_percent: None,
    });
    let eval_healthy = evaluate_account_quota(&muse_healthy);
    assert!(!eval_healthy.depleted);
    assert!(eval_healthy.sticky_ok);
    assert_eq!(eval_healthy.tier, 0);
    assert_eq!(eval_healthy.effective_used_pct, 20);

    let mut muse_using_extra = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000402",
      "Muse-Extra",
    );
    muse_using_extra.quota_detail = Some(QuotaDetail {
      plan_label: "5小时".to_string(),
      plan_value: "用尽".to_string(),
      plan_subtext: None,
      plan_used_percent: Some(100),
      extra_label: Some("额外".to_string()),
      extra_value: Some("剩余 70%".to_string()),
      extra_subtext: None,
      extra_used_percent: Some(30),
    });
    let eval_extra = evaluate_account_quota(&muse_using_extra);
    assert!(!eval_extra.depleted);
    assert_eq!(eval_extra.tier, 1);
    assert_eq!(eval_extra.effective_used_pct, 30);

    let mut hark_monthly_full = make_test_account(
      ProviderId::hark(),
      "00000000-0000-4000-8000-000000000403",
      "Hark-MonthFull",
    );
    hark_monthly_full.quota_detail = Some(QuotaDetail {
      plan_label: "每日".to_string(),
      plan_value: "剩余 90%".to_string(),
      plan_subtext: None,
      plan_used_percent: Some(10),
      extra_label: Some("月上限".to_string()),
      extra_value: Some("已满".to_string()),
      extra_subtext: None,
      extra_used_percent: Some(100),
    });
    let eval_hark = evaluate_account_quota(&hark_monthly_full);
    assert!(eval_hark.depleted);
    assert!(!eval_hark.sticky_ok);

    let mut rem_fallback = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000404",
      "Muse-RemFallback",
    );
    rem_fallback.quota_percent = Some(10);
    let eval_rem = evaluate_account_quota(&rem_fallback);
    assert!(!eval_rem.depleted);
    assert!(!eval_rem.sticky_ok);
    assert_eq!(eval_rem.effective_used_pct, 90);
    assert_eq!(eval_rem.tier, 1);
  }

  #[tokio::test]
  async fn auto_channel_routing_sticky_quota_switch_cooldown_and_failover() {
    let temp_dir = std::env::temp_dir().join(format!("maam-auto-route-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mgr = SessionManager::new(temp_dir.clone());

    let k1 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000501".to_string(),
    };
    let k2 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000502".to_string(),
    };
    let k3 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000503".to_string(),
    };

    let mut a1 = make_test_account(ProviderId::muse(), &k1.uuid, "Muse-1");
    a1.auth_state = AuthState::Authenticated;
    a1.quota_percent = Some(60); // 40% used
    let mut a2 = make_test_account(ProviderId::muse(), &k2.uuid, "Muse-2");
    a2.auth_state = AuthState::Authenticated;
    a2.quota_percent = Some(90); // 10% used (highest remaining)
    let mut a3 = make_test_account(ProviderId::muse(), &k3.uuid, "Muse-3");
    a3.auth_state = AuthState::Authenticated;
    a3.quota_percent = Some(0); // depleted

    let mut locked = mgr.inner.lock().await;
    locked.store.accounts = vec![a1, a2, a3];
    locked.store.group_chats.push(GroupChatRecord {
      id: "g-auto-1".to_string(),
      name: "自动轮换群".to_string(),
      members: vec![k1.clone(), k2.clone(), k3.clone()],
      created_at_ms: 1,
      messages: Vec::new(),
      shared_context: String::new(),
      shared_skills: Vec::new(),
      member_roles: Vec::new(),
      workflow_mode: GroupWorkflowMode::Parallel,
      max_relay_hops: 2,
      thread_bindings: Vec::new(),
    });
    let eligible = SessionManager::collect_group_eligible_members_locked(&locked, 0);

    // 首次选择剩余额度最高的 Muse-2
    let (picked1, _) = SessionManager::select_auto_channel_account_locked(
      &locked,
      "g-auto-1",
      &ProviderId::muse(),
      &eligible,
      &[],
      1_000,
    )
    .unwrap();
    assert_eq!(picked1, k2);

    // 粘性复用：即使 Muse-2 用量涨到 70%（仍 <85%），且 Muse-1 更空闲，仍继续粘性复用 Muse-2
    locked
      .group_channel_last_account
      .insert(("g-auto-1".to_string(), ProviderId::muse()), k2.clone());
    locked.store.accounts[1].quota_percent = Some(30); // 70% used
    let (picked_sticky, _) = SessionManager::select_auto_channel_account_locked(
      &locked,
      "g-auto-1",
      &ProviderId::muse(),
      &eligible,
      &[],
      2_000,
    )
    .unwrap();
    assert_eq!(picked_sticky, k2);

    // 阈值切换：Muse-2 用量 >=85% 后自动切换到剩余最多的 Muse-1
    locked.store.accounts[1].quota_percent = Some(10); // 90% used
    let (picked_switch, _) = SessionManager::select_auto_channel_account_locked(
      &locked,
      "g-auto-1",
      &ProviderId::muse(),
      &eligible,
      &[],
      3_000,
    )
    .unwrap();
    assert_eq!(picked_switch, k1);

    // 冷却与故障转移排除：Muse-1 进入冷却后，即使 Muse-2 偏低也优先于冷却中的 Muse-1；且耗尽的 Muse-3 不会被选到
    locked
      .account_cooldown_until_ms
      .insert(k1.clone(), 50_000);
    let (picked_cooldown_avoid, _) = SessionManager::select_auto_channel_account_locked(
      &locked,
      "g-auto-1",
      &ProviderId::muse(),
      &eligible,
      &[],
      4_000,
    )
    .unwrap();
    assert_eq!(picked_cooldown_avoid, k2);

    // 仅剩耗尽账号时返回清晰错误
    let err = SessionManager::select_auto_channel_account_locked(
      &locked,
      "g-auto-1",
      &ProviderId::muse(),
      &eligible,
      &[k1, k2],
      5_000,
    )
    .unwrap_err();
    assert!(err.contains("耗尽") || err.contains("待授权"));

    drop(locked);
    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[tokio::test]
  async fn resolve_group_dispatch_targets_enforces_same_channel_mutual_exclusion() {
    let temp_dir = std::env::temp_dir().join(format!("maam-auto-mutex-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mgr = SessionManager::new(temp_dir.clone());

    let m1 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000601".to_string(),
    };
    let m2 = AccountKey {
      provider_id: ProviderId::muse(),
      uuid: "00000000-0000-4000-8000-000000000602".to_string(),
    };
    let h1 = AccountKey {
      provider_id: ProviderId::hark(),
      uuid: "00000000-0000-4000-8000-000000000603".to_string(),
    };
    let h2 = AccountKey {
      provider_id: ProviderId::hark(),
      uuid: "00000000-0000-4000-8000-000000000604".to_string(),
    };

    let mut am1 = make_test_account(ProviderId::muse(), &m1.uuid, "Muse1");
    am1.auth_state = AuthState::Authenticated;
    am1.quota_percent = Some(50);
    let mut am2 = make_test_account(ProviderId::muse(), &m2.uuid, "Muse2");
    am2.auth_state = AuthState::Authenticated;
    am2.quota_percent = Some(90);
    let mut ah1 = make_test_account(ProviderId::hark(), &h1.uuid, "Hark1");
    ah1.auth_state = AuthState::Authenticated;
    ah1.quota_percent = Some(20);
    let mut ah2 = make_test_account(ProviderId::hark(), &h2.uuid, "Hark2");
    ah2.auth_state = AuthState::Authenticated;
    ah2.quota_percent = Some(80);

    let mut locked = mgr.inner.lock().await;
    locked.store.accounts = vec![am1, am2, ah1, ah2];
    locked.store.group_chats.push(GroupChatRecord {
      id: "g-mutex-1".to_string(),
      name: "互斥测试群".to_string(),
      members: vec![m1.clone(), m2.clone(), h1.clone(), h2.clone()],
      created_at_ms: 1,
      messages: Vec::new(),
      shared_context: String::new(),
      shared_skills: Vec::new(),
      member_roles: Vec::new(),
      workflow_mode: GroupWorkflowMode::Parallel,
      max_relay_hops: 2,
      thread_bindings: Vec::new(),
    });
    let eligible = SessionManager::collect_group_eligible_members_locked(&locked, 0);

    // 显式点名 @Muse1 + 同时传 auto_channels=["muse", "hark"]：
    // Muse 渠道与 @Muse1 互斥被过滤；跨渠道的 Hark 自动按最高剩余额度选中 Hark2
    let routed = SessionManager::resolve_group_dispatch_targets_locked(
      &mut locked,
      "g-mutex-1",
      "@Muse1 请看下",
      &eligible,
      Some(vec![m2.clone()]),
      Some(vec!["muse".to_string(), "hark".to_string()]),
      10_000,
    )
    .unwrap();
    assert_eq!(routed.len(), 2);
    assert_eq!(routed[0].0, m1);
    assert_eq!(routed[0].2, None);
    assert_eq!(routed[1].0, h2);
    assert_eq!(routed[1].2, Some(ProviderId::hark()));

    // 无正文 @，仅传 target_members=[Muse1] + auto_channels=["muse", "hark"]
    let routed_chip = SessionManager::resolve_group_dispatch_targets_locked(
      &mut locked,
      "g-mutex-1",
      "对比这两种实现",
      &eligible,
      Some(vec![m1.clone()]),
      Some(vec!["muse".to_string(), "hark".to_string()]),
      11_000,
    )
    .unwrap();
    assert_eq!(routed_chip.len(), 2);
    assert_eq!(routed_chip[0].0, m1);
    assert_eq!(routed_chip[0].2, None);
    assert_eq!(routed_chip[1].0, h2);
    assert_eq!(routed_chip[1].2, Some(ProviderId::hark()));

    drop(locked);
    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[test]
  fn clean_reply_text_and_hark_prompt_adaptation() {
    assert_eq!(
      SessionManager::clean_extracted_reply_text("Gemini 说\n收到！你好，有什么我可以帮你的吗？"),
      "收到！你好，有什么我可以帮你的吗？"
    );
    assert_eq!(
      SessionManager::clean_extracted_reply_text("Gemini said: Hello there"),
      "Hello there"
    );
    assert_eq!(
      SessionManager::clean_extracted_reply_text("Gemini 说"),
      ""
    );
    assert_eq!(
      SessionManager::clean_extracted_reply_text("Gemini 说的是对的，确实如此。"),
      "Gemini 说的是对的，确实如此。"
    );

    assert_eq!(
      SessionManager::adapt_question_for_hark("各位好，收到请回复"),
      "你好，收到请回复"
    );
    assert_eq!(
      SessionManager::adapt_question_for_hark("大家觉得这个方案怎么样"),
      "你觉得这个方案怎么样"
    );
    assert_eq!(
      SessionManager::adapt_question_for_hark("大部分情况不受影响"),
      "大部分情况不受影响"
    );

    assert_eq!(
      SessionManager::adapt_question_for_hark("Everyone, how does this plan look"),
      "how does this plan look"
    );
    assert_eq!(
      SessionManager::adapt_question_for_hark("What do all of you think of this plan"),
      "What do you think of this plan"
    );
    assert_eq!(
      SessionManager::adapt_question_for_hark("Hey everyone, please reply"),
      "hello, please reply"
    );

    let hark_ctx = SessionManager::sanitize_shared_context_for_hark(
      "[本轮结论汇总 · API方案]\n- a@gmail.com: 使用 REST\n- 约束: 低内存",
    );
    assert!(hark_ctx.contains("Background points (API方案):"));
    assert!(hark_ctx.contains("- 使用 REST"));
    assert!(!hark_ctx.contains("观点1"));
  }

  #[tokio::test]
  async fn page_error_state_tracks_excludes_group_members_and_clears_on_recovery_or_logout() {
    let temp_dir = std::env::temp_dir().join(format!("maam-page-err-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let store_path = temp_dir.join("accounts.json");

    let mut a1 = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000901",
      "Muse-Err",
    );
    a1.auth_state = AuthState::Authenticated;
    let mut a2 = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000902",
      "Muse-Ok",
    );
    a2.auth_state = AuthState::Authenticated;

    let k1 = AccountKey {
      provider_id: a1.provider_id.clone(),
      uuid: a1.uuid.clone(),
    };
    let k2 = AccountKey {
      provider_id: a2.provider_id.clone(),
      uuid: a2.uuid.clone(),
    };

    let initial = AccountsStoreFile {
      version: crate::storage::STORE_VERSION,
      accounts: vec![a1, a2],
      last_committed: None,
      pending_cleanup: Vec::new(),
      group_chats: vec![GroupChatRecord {
        id: "g-err-1".to_string(),
        name: "异常状态测试群".to_string(),
        members: vec![k1.clone(), k2.clone()],
        created_at_ms: 1,
        messages: vec![],
        shared_context: String::new(),
        shared_skills: vec![],
        member_roles: vec![],
        workflow_mode: GroupWorkflowMode::Parallel,
        max_relay_hops: 2,
        thread_bindings: vec![],
      }],
      external_agents: Vec::new(),
      settings: AppSettings::default(),
    };
    save_store_atomic(&store_path, &initial).unwrap();

    let mgr = SessionManager::new(temp_dir.clone());
    let vid = 42u64;
    let mut locked = mgr.inner.lock().await;
    #[cfg(desktop)]
    locked.warm_pool.insert(
      k1.clone(),
      WarmDesktopEntry {
        account_key: k1.clone(),
        view_instance_id: vid,
        webview_label: "wv-err-test".to_string(),
        popup_label: None,
        paint_ready: true,
        staged_painted: true,
        navigation_reached: true,
        load_gen: Arc::new(AtomicU64::new(1)),
        last_url: None,
        captured_identity_hint: None,
        watcher_running: false,
        probe_notify: Arc::new(tokio::sync::Notify::new()),
        last_used_ms: 1,
        alive_flag: Arc::new(AtomicBool::new(true)),
      },
    );
    #[cfg(not(desktop))]
    {
      locked.active_slot = Some(ActiveSessionSlot {
        account_key: k1.clone(),
        view_instance_id: vid,
        switch_seq: 1,
        committed: true,
      });
    }

    let snap_err = SessionManager::apply_page_error_state_locked(
      &mut locked,
      k1.clone(),
      vid,
      "页面出错，请检查代理或清理重登",
    )
    .expect("should emit snapshot on page error");
    assert_eq!(snap_err.account_errors.len(), 1);
    assert_eq!(snap_err.account_errors[0].uuid, k1.uuid);
    assert_eq!(
      snap_err.account_errors[0].message,
      "页面出错，请检查代理或清理重登"
    );

    let eligible = SessionManager::collect_group_eligible_members_locked(&locked, 0);
    assert_eq!(eligible, vec![(k2.clone(), "Muse-Ok".to_string())]);

    let (snap_recovered, became_auth, _, _) = SessionManager::apply_detected_metadata_locked(
      &mut locked,
      &k1,
      vid,
      None,
      None,
      None,
      None,
      None,
      None,
    );
    assert!(!became_auth);
    let snap_recovered = snap_recovered.expect("should emit snapshot when clearing page error");
    assert!(snap_recovered.account_errors.is_empty());
    assert_eq!(
      SessionManager::collect_group_eligible_members_locked(&locked, 0).len(),
      2
    );

    SessionManager::apply_page_error_state_locked(
      &mut locked,
      k1.clone(),
      vid,
      "登录态异常，请退出或清理重登",
    )
    .expect("re-insert page error");
    let snap_logged_out = SessionManager::apply_logged_out_state_locked(&mut locked, &k1, vid)
      .expect("should emit snapshot when transitioning to logged_out");
    assert!(snap_logged_out.account_errors.is_empty());
    assert_eq!(
      snap_logged_out
        .accounts
        .iter()
        .find(|a| a.uuid == k1.uuid)
        .unwrap()
        .auth_state,
      AuthState::LoggedOut
    );

    let _ = SessionManager::apply_page_error_state_locked(
      &mut locked,
      k1.clone(),
      vid,
      "二次异常",
    )
    .expect("insert page error while already logged out");
    let snap_still_logged_out =
      SessionManager::apply_logged_out_state_locked(&mut locked, &k1, vid)
        .expect("clearing error on already logged_out account must still emit snapshot");
    assert!(snap_still_logged_out.account_errors.is_empty());

    drop(locked);
    let _ = std::fs::remove_dir_all(&temp_dir);
  }

  #[tokio::test]
  async fn soft_identity_upgrades_to_strong_email_and_reverts_placeholder_on_logout() {
    let temp_dir = std::env::temp_dir().join(format!("maam-soft-id-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let store_path = temp_dir.join("accounts.json");

    let mut acc = make_test_account(
      ProviderId::muse(),
      "00000000-0000-4000-8000-000000000903",
      "待登录账号",
    );
    acc.auto_alias = true;
    acc.auth_state = AuthState::Unknown;

    let key = AccountKey {
      provider_id: acc.provider_id.clone(),
      uuid: acc.uuid.clone(),
    };

    let initial = AccountsStoreFile {
      version: crate::storage::STORE_VERSION,
      accounts: vec![acc],
      last_committed: None,
      pending_cleanup: Vec::new(),
      group_chats: Vec::new(),
      external_agents: Vec::new(),
      settings: AppSettings::default(),
    };
    save_store_atomic(&store_path, &initial).unwrap();

    let mgr = SessionManager::new(temp_dir.clone());
    let vid = 77u64;
    let mut locked = mgr.inner.lock().await;
    #[cfg(desktop)]
    locked.warm_pool.insert(
      key.clone(),
      WarmDesktopEntry {
        account_key: key.clone(),
        view_instance_id: vid,
        webview_label: "wv-soft-test".to_string(),
        popup_label: None,
        paint_ready: true,
        staged_painted: true,
        navigation_reached: true,
        load_gen: Arc::new(AtomicU64::new(1)),
        last_url: None,
        captured_identity_hint: None,
        watcher_running: false,
        probe_notify: Arc::new(tokio::sync::Notify::new()),
        last_used_ms: 1,
        alive_flag: Arc::new(AtomicBool::new(true)),
      },
    );
    #[cfg(not(desktop))]
    {
      locked.active_slot = Some(ActiveSessionSlot {
        account_key: key.clone(),
        view_instance_id: vid,
        switch_seq: 1,
        committed: true,
      });
    }

    // 1. Soft fallback updates alias to "Signed-in account" while keeping auto_alias = true.
    let (snap1, became_auth1, _, _) = SessionManager::apply_detected_metadata_locked(
      &mut locked,
      &key,
      vid,
      None,
      Some("Signed-in account".to_string()),
      None,
      None,
      None,
      None,
    );
    assert!(became_auth1);
    assert!(snap1.is_some());
    assert_eq!(locked.store.accounts[0].alias, "Signed-in account");
    assert!(locked.store.accounts[0].auto_alias);

    // 2. Display name replaces generic "Signed-in account" while still keeping auto_alias = true.
    let (snap2, _, _, _) = SessionManager::apply_detected_metadata_locked(
      &mut locked,
      &key,
      vid,
      None,
      Some("Alice Meta".to_string()),
      None,
      None,
      None,
      None,
    );
    assert!(snap2.is_some());
    assert_eq!(locked.store.accounts[0].alias, "Alice Meta");
    assert!(locked.store.accounts[0].auto_alias);

    // 3. Strong email overrides soft identity and clears auto_alias.
    let (snap3, _, _, _) = SessionManager::apply_detected_metadata_locked(
      &mut locked,
      &key,
      vid,
      Some("alice@gmail.com".to_string()),
      None,
      None,
      None,
      None,
      None,
    );
    assert!(snap3.is_some());
    assert_eq!(locked.store.accounts[0].alias, "alice@gmail.com");
    assert!(!locked.store.accounts[0].auto_alias);

    // 4. Logout on a soft-fallback placeholder reverts "Signed-in account" -> "Account not signed in".
    locked.store.accounts[0].alias = "Signed-in account".to_string();
    locked.store.accounts[0].auto_alias = true;
    let snap_out = SessionManager::apply_logged_out_state_locked(&mut locked, &key, vid)
      .expect("logout should emit snapshot");
    let out_acc = &snap_out.accounts[0];
    assert_eq!(out_acc.auth_state, AuthState::LoggedOut);
    assert_eq!(out_acc.alias, "Account not signed in");
    assert!(out_acc.auto_alias);

    drop(locked);
    let _ = std::fs::remove_dir_all(&temp_dir);
  }
}
