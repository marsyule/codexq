import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import {
  X,
  Eye,
  EyeOff,
  Zap,
  DownloadCloud,
  Plus,
  Check,
  Server,
  Settings2,
  ChevronDown,
  ChevronUp,
  AlertCircle,
  RotateCcw,
  Search,
  FileCode,
  Copy,
  ExternalLink,
} from 'lucide-react';
import { type ProviderData, type ConnectivityResult, type ToastPayload, type GatewayStatus, formatContextWindow, sortReasoningLevels, normalizeWireApi } from '../types';

interface AddProviderModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaved: (provider: ProviderData, shouldSwitch: boolean) => void;
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
  editingProvider?: ProviderData | null;
}

const DEFAULT_AUTH_JSON = JSON.stringify(
  {
    auth_mode: 'api_key',
    OPENAI_API_KEY: null,
    tokens: null,
  },
  null,
  2
);

/**
 * Universal modal dialog for adding or editing a third-party AI provider.
 *
 * Implements:
 * 1. Clean Clash Verge light design system (pure white dialog, slate borders, blue accents).
 * 2. Decoupled "Test Ping" vs "Fetch Upstream Models".
 * 3. Interactive Upstream Candidate selection list with search filtering.
 * 4. Active model pool management with default model toggle and manual model additions.
 * 5. Configurable context window with min 256K clamp and per-model override support.
 * 6. Collapsible Advanced Settings panel featuring wire_api, notes, and live config.toml/auth.json text editors.
 */
export const AddProviderModal: React.FC<AddProviderModalProps> = ({
  isOpen,
  onClose,
  onSaved,
  showToast,
  editingProvider,
}) => {
  const { t } = useTranslation();

  // Basic Form State
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [wireApi, setWireApi] = useState('responses');
  const [notes, setNotes] = useState('');

  // Context Window State (Default minimum 256,000)
  const [contextWindow, setContextWindow] = useState(256_000);
  const [isCustomCw, setIsCustomCw] = useState(false);
  const [customCwInput, setCustomCwInput] = useState('');
  const [modelContextWindows, setModelContextWindows] = useState<Record<string, number>>({});

  // Per-model upstream protocol overrides ('' / absent = inherit the provider default).
  const [modelWireApis, setModelWireApis] = useState<Record<string, string>>({});

  // Provider-level master switch: when off, per-model protocol selectors are hidden.
  const [localGatewayEnabled, setLocalGatewayEnabled] = useState(false);

  // Loopback gateway address, so the config preview shows the address CodexQ actually
  // writes rather than the raw upstream when Chat Completions translation is in play.
  const [gatewayBaseUrl, setGatewayBaseUrl] = useState('');

  // Live gateway port + availability. The port is a process-wide setting, so this editor only
  // displays it: showing a configurable field here would imply a per-provider port that the
  // single-listener architecture cannot honour.
  const [gatewayInfo, setGatewayInfo] = useState<{
    port: number;
    state: string;
    running: boolean;
    envOverride: boolean;
  } | null>(null);

  // Reasoning Levels State (Default: low, medium, high)
  const [reasoningLevels, setReasoningLevels] = useState<string[]>(['low', 'medium', 'high']);
  const [modelReasoningLevels, setModelReasoningLevels] = useState<Record<string, string[]>>({});
  const [customTierInput, setCustomTierInput] = useState('');
  const [showAddTierInput, setShowAddTierInput] = useState(false);
  const [catalogPath, setCatalogPath] = useState<string>('');

  // Model Pool State
  const [models, setModels] = useState<string[]>([]);
  const [activeModel, setActiveModel] = useState('');
  const [newModelInput, setNewModelInput] = useState('');

  // Upstream Fetch State
  const [upstreamModels, setUpstreamModels] = useState<string[]>([]);
  const [upstreamFilter, setUpstreamFilter] = useState('');
  const [fetchingUpstream, setFetchingUpstream] = useState(false);

  // Ping Testing State
  const [pinging, setPinging] = useState(false);
  const [pingResult, setPingResult] = useState<ConnectivityResult | null>(null);

  // Advanced Settings Collapsible State
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [customConfigToml, setCustomConfigToml] = useState('');
  const [customAuthJson, setCustomAuthJson] = useState('');
  const [configTomlManuallyEdited, setConfigTomlManuallyEdited] = useState(false);
  const [authJsonManuallyEdited, setAuthJsonManuallyEdited] = useState(false);

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Slug generator helper
  const providerSlug = useMemo(() => {
    if (editingProvider?.id) return editingProvider.id;
    const clean = name.trim().toLowerCase().replace(/[^a-z0-9_-]/g, '_');
    return clean || 'provider_id';
  }, [name, editingProvider]);

  // Filtered Upstream Models (Called unconditionally on every render)
  const filteredUpstreamModels = useMemo(() => {
    const q = upstreamFilter.trim().toLowerCase();
    if (!q) return upstreamModels;
    return upstreamModels.filter((m) => m.toLowerCase().includes(q));
  }, [upstreamModels, upstreamFilter]);

  // Standard config.toml generator
  const generateDefaultConfigToml = useCallback(
    (targetModel: string, pWireApi: string) => {
      const chosen = targetModel.trim() || 'default_model';
      const keyDisplay = apiKey.trim() || (editingProvider?.key_masked || 'sk-your-api-key');
      const urlDisplay = baseUrl.trim() || 'https://api.example.com/v1';
      const nameDisplay = name.trim() || 'Custom Provider';

      // The preview must show what CodexQ actually writes, not what the user selected.
      // Codex always speaks Responses and rejects any other value, so a preview that
      // echoed `wire_api = "chat"` would invite the user to brick their own Codex config
      // by copying it into the custom-config field. Chat models are served by the
      // loopback gateway, so the preview shows that address instead of the upstream.
      const needsGateway =
        normalizeWireApi(pWireApi) === 'chat' ||
        Object.values(modelWireApis).some((value) => normalizeWireApi(value) === 'chat');
      const effectiveBaseUrl = needsGateway
        ? gatewayBaseUrl || 'http://127.0.0.1:17871/v1'
        : urlDisplay;

      return `model = "${chosen}"\nmodel_provider = "${providerSlug}"\n\n[model_providers.${providerSlug}]\nname = "${nameDisplay}"\nbase_url = "${effectiveBaseUrl}"\nwire_api = "responses"\nexperimental_bearer_token = "${keyDisplay}"\n`;
    },
    [apiKey, baseUrl, editingProvider?.key_masked, name, providerSlug, modelWireApis, gatewayBaseUrl]
  );

  // Context Window parsing helper (supports k, m suffixes, clamps to >= 256,000)
  const parseContextWindow = useCallback((val: string): number => {
    const clean = val.trim().toLowerCase();
    if (!clean) return 256_000;
    if (clean.endsWith('m')) {
      const num = parseFloat(clean.slice(0, -1));
      return isNaN(num) ? 256_000 : Math.round(num * 1_000_000);
    }
    if (clean.endsWith('k')) {
      const num = parseFloat(clean.slice(0, -1));
      return isNaN(num) ? 256_000 : Math.round(num * 1_000);
    }
    const raw = parseInt(clean.replace(/,/g, ''), 10);
    return isNaN(raw) ? 256_000 : raw;
  }, []);

  // Reset or populate state on modal open
  useEffect(() => {
    if (editingProvider) {
      setName(editingProvider.name);
      setBaseUrl(editingProvider.base_url);
      setApiKey(''); // Do not pull plaintext key to frontend
      setWireApi(editingProvider.wire_api || 'responses');
      setLocalGatewayEnabled(editingProvider.gateway_enabled || false);
      setNotes(editingProvider.notes || '');
      setModels(editingProvider.models || []);
      const currentActive = editingProvider.active_model || editingProvider.models[0] || '';
      setActiveModel(currentActive);

      const cw = Math.max(256_000, editingProvider.context_window ?? 256_000);
      setContextWindow(cw);
      const isPreset = [256_000, 512_000, 1_000_000, 2_000_000].includes(cw);
      setIsCustomCw(!isPreset);
      setCustomCwInput(!isPreset ? String(cw) : '');
      setModelContextWindows(editingProvider.model_context_windows || {});

      const rLevels = editingProvider.reasoning_levels?.length
        ? sortReasoningLevels(editingProvider.reasoning_levels)
        : ['low', 'medium', 'high'];
      setReasoningLevels(rLevels);

      const sortedModelLevels: Record<string, string[]> = {};
      if (editingProvider.model_reasoning_levels) {
        for (const [m, lvls] of Object.entries(editingProvider.model_reasoning_levels)) {
          if (lvls?.length) {
            sortedModelLevels[m] = sortReasoningLevels(lvls);
          }
        }
      }
      setModelReasoningLevels(sortedModelLevels);

      const normalizedModelWire: Record<string, string> = {};
      if (editingProvider.model_wire_apis) {
        for (const [m, api] of Object.entries(editingProvider.model_wire_apis)) {
          if (m.trim()) {
            normalizedModelWire[m] = normalizeWireApi(api);
          }
        }
      }
      setModelWireApis(normalizedModelWire);

      invoke<string>('get_model_catalog_path', { providerId: editingProvider.id })
        .then((p) => setCatalogPath(p))
        .catch(() => setCatalogPath(''));

      if (editingProvider.custom_config_toml) {
        setCustomConfigToml(editingProvider.custom_config_toml);
        setConfigTomlManuallyEdited(true);
      } else {
        setCustomConfigToml('');
        setConfigTomlManuallyEdited(false);
      }

      if (editingProvider.custom_auth_json) {
        setCustomAuthJson(editingProvider.custom_auth_json);
        setAuthJsonManuallyEdited(true);
      } else {
        setCustomAuthJson(DEFAULT_AUTH_JSON);
        setAuthJsonManuallyEdited(false);
      }
    } else {
      setName('');
      setBaseUrl('');
      setApiKey('');
      setWireApi('responses');
      setLocalGatewayEnabled(false);
      setNotes('');
      setModels([]);
      setActiveModel('');
      setContextWindow(256_000);
      setIsCustomCw(false);
      setCustomCwInput('');
      setModelContextWindows({});
      setReasoningLevels(['low', 'medium', 'high']);
      setModelReasoningLevels({});
      setModelWireApis({});
      setCatalogPath('');
      setCustomConfigToml('');
      setCustomAuthJson(DEFAULT_AUTH_JSON);
      setConfigTomlManuallyEdited(false);
      setAuthJsonManuallyEdited(false);
    }

    setUpstreamModels([]);
    setUpstreamFilter('');
    setPingResult(null);
    setNewModelInput('');
    setShowKey(false);
    setError(null);
    setShowAdvanced(false);
  }, [editingProvider, isOpen]);

  // Keep live preview updated if not manually edited
  useEffect(() => {
    if (!configTomlManuallyEdited) {
      setCustomConfigToml(generateDefaultConfigToml(activeModel, wireApi));
    }
  }, [configTomlManuallyEdited, generateDefaultConfigToml, activeModel, wireApi]);

  // Resolve the gateway address and availability once per open, so the preview shows what
  // CodexQ actually writes and the status line never claims a stopped listener is live.
  useEffect(() => {
    if (!isOpen) return;
    invoke<GatewayStatus>('get_gateway_status')
      .then((status) => {
        setGatewayBaseUrl(status?.base_url ?? '');
        setGatewayInfo({
          port: status?.port ?? 0,
          state: status?.port_state ?? 'free',
          running: status?.running === true,
          envOverride: status?.env_override === true,
        });
      })
      .catch(() => {
        setGatewayBaseUrl('');
        setGatewayInfo(null);
      });
  }, [isOpen]);

  // Manual Model Adding
  const handleAddModel = (modelToAdd?: string) => {
    const target = (modelToAdd ?? newModelInput).trim();
    if (!target) return;
    if (!models.includes(target)) {
      const updated = [...models, target];
      setModels(updated);
      if (!activeModel) {
        setActiveModel(target);
      }
    }
    setNewModelInput('');
  };

  const handleRemoveModel = (modelToRemove: string) => {
    const updated = models.filter((m) => m !== modelToRemove);
    setModels(updated);
    if (activeModel === modelToRemove) {
      setActiveModel(updated[0] ?? '');
    }
  };

  // Add all upstream models into active pool
  const handleAddAllUpstream = () => {
    const newModels = [...models];
    upstreamModels.forEach((m) => {
      if (!newModels.includes(m)) {
        newModels.push(m);
      }
    });
    setModels(newModels);
    if (!activeModel && newModels.length > 0) {
      setActiveModel(newModels[0]);
    }
    showToast(
      t('providers.fetchSuccess', {
        defaultValue: `Added ${upstreamModels.length} models to active pool`,
        count: upstreamModels.length,
      }),
      'success'
    );
  };

  // Ping Testing
  const handlePing = async () => {
    if (!baseUrl.trim()) {
      showToast(t('providers.baseUrlRequired', 'Please enter a Base URL'), 'error');
      return;
    }
    if (!apiKey.trim() && !editingProvider) {
      showToast(t('providers.keyRequired', 'Please enter an API Key'), 'error');
      return;
    }

    setPinging(true);
    setPingResult(null);
    setError(null);

    try {
      const res = await invoke<ConnectivityResult>('test_provider_connectivity', {
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
        providerId: editingProvider?.id ?? null,
      });
      setPingResult(res);
      if (res.success) {
        showToast(
          t('providers.testSuccess', {
            defaultValue: `Connected successfully (${res.latency_ms}ms)`,
            latency: res.latency_ms,
          }),
          'success'
        );
      } else {
        showToast(res.message || t('providers.testFailed', 'Connection test failed'), 'error');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Connection test failed';
      setPingResult({
        success: false,
        message: msg,
        available_models: [],
      });
      setError(msg);
      showToast(msg, 'error');
    } finally {
      setPinging(false);
    }
  };

  // Fetch Upstream Models List
  const handleFetchUpstream = async () => {
    if (!baseUrl.trim()) {
      showToast(t('providers.baseUrlRequired', 'Please enter a Base URL'), 'error');
      return;
    }
    if (!apiKey.trim() && !editingProvider) {
      showToast(t('providers.keyRequired', 'Please enter an API Key'), 'error');
      return;
    }

    setFetchingUpstream(true);
    setError(null);

    try {
      const res = await invoke<ConnectivityResult>('test_provider_connectivity', {
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
        providerId: editingProvider?.id ?? null,
      });

      if (res.success && res.available_models && res.available_models.length > 0) {
        setUpstreamModels(res.available_models);
        showToast(
          t('providers.fetchSuccess', {
            defaultValue: `Found ${res.available_models.length} upstream models`,
            count: res.available_models.length,
          }),
          'success'
        );
      } else if (res.success) {
        setUpstreamModels([]);
        showToast(t('providers.fetchEmpty', 'No models found from upstream, add manually below'), 'info');
      } else {
        setError(res.message);
        showToast(res.message || t('providers.testFailed', 'Connection test failed'), 'error');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to fetch models';
      setError(msg);
      showToast(msg, 'error');
    } finally {
      setFetchingUpstream(false);
    }
  };

  // Save handler
  const handleSave = async (shouldSwitch: boolean) => {
    if (!name.trim()) {
      showToast(t('providers.nameRequired', 'Provider name is required'), 'error');
      return;
    }
    if (!baseUrl.trim()) {
      showToast(t('providers.baseUrlRequired', 'Base URL is required'), 'error');
      return;
    }
    if (!editingProvider && !apiKey.trim()) {
      showToast(t('providers.keyRequired', 'API Key is required for new provider'), 'error');
      return;
    }

    setSaving(true);
    setError(null);

    try {
      const effectiveCw = Math.max(
        256_000,
        isCustomCw ? parseContextWindow(customCwInput) : contextWindow
      );

      const cleanedModelCw: Record<string, number> = {};
      for (const [m, cw] of Object.entries(modelContextWindows)) {
        if (models.includes(m) || m === activeModel) {
          cleanedModelCw[m] = Math.max(256_000, cw);
        }
      }

      const cleanedModelReasoning: Record<string, string[]> = {};
      for (const [m, r] of Object.entries(modelReasoningLevels)) {
        if ((models.includes(m) || m === activeModel) && r && r.length > 0) {
          cleanedModelReasoning[m] = sortReasoningLevels(r);
        }
      }

      // Master off normalizes every model back to Responses and clears protocol
      // overrides; no hidden stale Chat routing remains.
      const defaultWire = localGatewayEnabled ? normalizeWireApi(wireApi) : 'responses';

      const cleanedModelWireApis: Record<string, string> = {};
      if (localGatewayEnabled) {
        for (const [m, api] of Object.entries(modelWireApis)) {
          const slug = m.trim();
          if (!slug || !(models.includes(m) || m === activeModel)) continue;
          const normalized = normalizeWireApi(api);
          if (normalized !== defaultWire) {
            cleanedModelWireApis[slug] = normalized;
          }
        }
      }

      const payload = {
        id: editingProvider ? editingProvider.id : undefined,
        name: name.trim(),
        base_url: baseUrl.trim(),
        wire_api: localGatewayEnabled ? wireApi : 'responses',
        gateway_enabled: localGatewayEnabled,
        active_model: activeModel.trim() || (models[0] ?? 'default'),
        models: models.length > 0 ? models : [activeModel.trim() || 'default'],
        context_window: effectiveCw,
        model_context_windows: Object.keys(cleanedModelCw).length > 0 ? cleanedModelCw : undefined,
        reasoning_levels: reasoningLevels.length > 0 ? sortReasoningLevels(reasoningLevels) : undefined,
        model_reasoning_levels: Object.keys(cleanedModelReasoning).length > 0 ? cleanedModelReasoning : undefined,
        model_wire_apis: Object.keys(cleanedModelWireApis).length > 0 ? cleanedModelWireApis : undefined,
        notes: notes.trim() || null,
        custom_config_toml: configTomlManuallyEdited ? customConfigToml.trim() || null : null,
        custom_auth_json: authJsonManuallyEdited ? customAuthJson.trim() || null : null,
        api_key: apiKey.trim() ? apiKey.trim() : undefined,
      };

      const saved = await invoke<ProviderData>('save_provider', { payload });
      showToast(
        editingProvider
          ? t('providers.updated', 'Provider updated successfully')
          : t('providers.created', 'Provider created successfully'),
        'success'
      );
      onSaved(saved, shouldSwitch);
      onClose();
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to save provider';
      setError(msg);
      showToast(msg, 'error');
    } finally {
      setSaving(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/45 p-4 backdrop-blur-xs animate-in fade-in duration-150">
      <div
        className="relative w-full max-w-xl max-h-[90vh] flex flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-slate-100 px-6 py-4 shrink-0">
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-blue-100/80 text-blue-700">
              <Server className="h-5 w-5" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-slate-900">
                {editingProvider ? t('providers.editTitle', 'Edit Model Provider') : t('providers.addTitle', 'Add Model Provider')}
              </h3>
              <p className="text-[11px] text-slate-500">
                {t('providers.subtitle', 'Connect any OpenAI Responses compatible provider endpoint')}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Scrollable Body */}
        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {error && (
            <div className="flex items-start gap-2 rounded-xl border border-rose-200 bg-rose-50/90 p-3 text-xs text-rose-700">
              <AlertCircle className="h-4 w-4 shrink-0 mt-0.5 text-rose-600" />
              <span>{error}</span>
            </div>
          )}

          {/* Name & Base URL */}
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-semibold text-slate-700 mb-1">
                {t('providers.nameLabel', 'Provider Name')} <span className="text-rose-500">*</span>
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. DeepSeek, StepFun, SiliconFlow"
                className="w-full rounded-xl border border-slate-200 bg-slate-50/50 px-3 py-2 text-xs text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:bg-white focus:outline-hidden transition-all"
              />
            </div>

            <div>
              <div className="flex items-center justify-between mb-1">
                <label className="block text-xs font-semibold text-slate-700">
                  {t('providers.baseUrlLabel', 'API Base URL')} <span className="text-rose-500">*</span>
                </label>
                <span className="text-[10px] text-slate-400 font-mono">
                  {t('providers.responsesApiHint', 'Supports OpenAI Responses format')}
                </span>
              </div>
              <input
                type="text"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.example.com/v1"
                className="w-full rounded-xl border border-slate-200 bg-slate-50/50 px-3 py-2 text-xs font-mono text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:bg-white focus:outline-hidden transition-all"
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-700 mb-1">
                {t('providers.apiKeyLabel', 'API Key')}{' '}
                {editingProvider ? (
                  <span className="text-[11px] font-normal text-slate-400">
                    ({t('providers.leaveBlankKey', 'leave blank to keep existing key')})
                  </span>
                ) : (
                  <span className="text-rose-500">*</span>
                )}
              </label>
              <div className="relative">
                <input
                  type={showKey ? 'text' : 'password'}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  placeholder={editingProvider ? editingProvider.key_masked : 'sk-********************************'}
                  className="w-full rounded-xl border border-slate-200 bg-slate-50/50 pl-3 pr-10 py-2 text-xs font-mono text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:bg-white focus:outline-hidden transition-all"
                />
                <button
                  type="button"
                  onClick={() => setShowKey(!showKey)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600 transition-colors"
                >
                  {showKey ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                </button>
              </div>
            </div>
          </div>

          {/* Action Row: Distinct Ping Test & Fetch Upstream Models */}
          <div className="rounded-xl border border-slate-200/90 bg-slate-50/60 p-3 flex flex-wrap items-center justify-between gap-2.5">
            <div className="flex items-center gap-2 min-w-0">
              {pingResult ? (
                pingResult.success ? (
                  <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[11px] font-semibold bg-emerald-50 text-emerald-700 border border-emerald-200/80">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-500"></span>
                    {t('providers.testSuccess', {
                      defaultValue: `Ping: ${pingResult.latency_ms}ms`,
                      latency: pingResult.latency_ms,
                    })}
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[11px] font-semibold bg-rose-50 text-rose-700 border border-rose-200/80 truncate max-w-xs" title={pingResult.message}>
                    <span className="h-1.5 w-1.5 rounded-full bg-rose-500"></span>
                    {pingResult.message}
                  </span>
                )
              ) : (
                <span className="text-[11px] text-slate-500">
                  {t('providers.testHint', 'Test connection or query upstream models')}
                </span>
              )}
            </div>

            <div className="flex items-center gap-2 shrink-0">
              {/* Button 1: Ping */}
              <button
                type="button"
                onClick={handlePing}
                disabled={pinging || fetchingUpstream}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium text-slate-700 bg-white hover:bg-slate-50 border border-slate-200 shadow-2xs transition-all disabled:opacity-50"
              >
                <Zap className={`w-3.5 h-3.5 text-amber-500 ${pinging ? 'animate-bounce' : ''}`} />
                <span>{pinging ? t('providers.testing', 'Pinging...') : t('providers.pingConnection', 'Test Ping')}</span>
              </button>

              {/* Button 2: Fetch Upstream Models */}
              <button
                type="button"
                onClick={handleFetchUpstream}
                disabled={pinging || fetchingUpstream}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold text-blue-700 bg-blue-50 hover:bg-blue-100/80 border border-blue-200/80 shadow-2xs transition-all disabled:opacity-50"
              >
                <DownloadCloud className={`w-3.5 h-3.5 text-blue-600 ${fetchingUpstream ? 'animate-pulse' : ''}`} />
                <span>
                  {fetchingUpstream
                    ? t('providers.fetchingModels', 'Fetching...')
                    : t('providers.fetchUpstreamModels', 'Fetch Models')}
                </span>
              </button>
            </div>
          </div>

          {/* Upstream Candidate Models Selection Section */}
          {upstreamModels.length > 0 && (
            <div className="rounded-xl border border-blue-200/70 bg-blue-50/20 p-3 space-y-2.5 animate-in fade-in duration-200">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-bold text-slate-900">
                    {t('providers.upstreamModelsTitle', 'Upstream Models')}
                  </span>
                  <span className="text-[10px] font-mono font-semibold px-1.5 py-0.2 rounded-full bg-blue-100 text-blue-700">
                    {upstreamModels.length}
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  {/* Search Filter for Upstream Models */}
                  <div className="relative">
                    <Search className="w-3 h-3 text-slate-400 absolute left-2 top-1/2 -translate-y-1/2 pointer-events-none" />
                    <input
                      type="text"
                      value={upstreamFilter}
                      onChange={(e) => setUpstreamFilter(e.target.value)}
                      placeholder={t('providers.filterUpstreamModels', 'Filter models...')}
                      className="pl-6 pr-2.5 py-1 text-[11px] rounded-lg border border-slate-200 bg-white text-slate-800 placeholder-slate-400 focus:outline-hidden focus:border-blue-500 w-36 sm:w-44 transition-all"
                    />
                  </div>

                  <button
                    type="button"
                    onClick={handleAddAllUpstream}
                    className="text-[11px] font-semibold text-blue-600 hover:text-blue-800 transition-colors"
                  >
                    {t('providers.addAllUpstream', 'Add All')}
                  </button>
                </div>
              </div>

              {/* Upstream Chips Container */}
              <div className="max-h-36 overflow-y-auto flex flex-wrap gap-1.5 p-1 bg-white/70 rounded-lg border border-blue-100">
                {filteredUpstreamModels.map((m) => {
                  const alreadyInPool = models.includes(m);
                  return (
                    <button
                      key={m}
                      type="button"
                      onClick={() => !alreadyInPool && handleAddModel(m)}
                      disabled={alreadyInPool}
                      className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[11px] font-mono transition-all ${
                        alreadyInPool
                          ? 'bg-slate-100 text-slate-400 border border-slate-200/60 cursor-default'
                          : 'bg-white hover:bg-blue-50 text-slate-700 hover:text-blue-700 border border-slate-200 hover:border-blue-300 shadow-2xs cursor-pointer active:scale-95'
                      }`}
                      title={alreadyInPool ? t('providers.alreadyAdded', 'Already added') : t('providers.clickToAddModel', 'Click to add')}
                    >
                      {alreadyInPool ? (
                        <Check className="w-3 h-3 text-slate-400" />
                      ) : (
                        <Plus className="w-3 h-3 text-blue-600" />
                      )}
                      <span>{m}</span>
                      {alreadyInPool && <span className="text-[9px] text-slate-400">({t('providers.alreadyAdded', 'Added')})</span>}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {/* Provider Default Context Window Configuration Section */}
          <div className="rounded-xl border border-slate-200/90 bg-slate-50/50 p-3 space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <label className="block text-xs font-semibold text-slate-700">
                  {t('providers.defaultContextWindowLabel', '服务商默认上下文')}
                </label>
                <span className="text-[10px] text-slate-400">
                  ({t('providers.contextWindowMinHint', '最低 256K 保底，未单独指定上下文的模型默认继承')})
                </span>
              </div>
              <span className="text-[11px] font-mono text-blue-600 font-semibold">
                {formatContextWindow(isCustomCw ? Math.max(256_000, parseContextWindow(customCwInput)) : contextWindow)}
                <span className="text-[10px] text-slate-400 font-normal ml-1">
                  ({(isCustomCw ? Math.max(256_000, parseContextWindow(customCwInput)) : contextWindow).toLocaleString()} tokens)
                </span>
              </span>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-1.5">
                {[
                  { label: '256K (保底)', value: 256_000 },
                  { label: '512K', value: 512_000 },
                  { label: '1M', value: 1_000_000 },
                  { label: '2M', value: 2_000_000 },
                ].map((preset) => {
                  const isSelected = !isCustomCw && contextWindow === preset.value;
                  return (
                    <button
                      key={preset.value}
                      type="button"
                      onClick={() => {
                        setContextWindow(preset.value);
                        setIsCustomCw(false);
                      }}
                      className={`px-2.5 py-1 rounded-lg text-xs font-mono transition-all cursor-pointer ${
                        isSelected
                          ? 'bg-blue-600 text-white font-semibold shadow-2xs'
                          : 'bg-white hover:bg-slate-100 text-slate-700 border border-slate-200 shadow-2xs'
                      }`}
                    >
                      {preset.label}
                    </button>
                  );
                })}

                <button
                  type="button"
                  onClick={() => {
                    setIsCustomCw(true);
                    if (!customCwInput) setCustomCwInput(String(contextWindow));
                  }}
                  className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all cursor-pointer ${
                    isCustomCw
                      ? 'bg-blue-600 text-white font-semibold shadow-2xs'
                      : 'bg-white hover:bg-slate-100 text-slate-700 border border-slate-200 shadow-2xs'
                  }`}
                >
                  {t('providers.contextWindowCustom', '自定义')}
                </button>
              </div>

              {/* Reset all overrides button if any overrides exist */}
              {Object.keys(modelContextWindows).length > 0 && (
                <button
                  type="button"
                  onClick={() => {
                    setModelContextWindows({});
                    showToast(t('providers.clearedAllOverrides', '已清除所有单模型覆盖，全部统一跟随默认'), 'info');
                  }}
                  className="text-[11px] font-medium text-slate-500 hover:text-blue-600 underline cursor-pointer transition-colors"
                >
                  {t('providers.resetAllToDefault', '全部重置为跟随默认')}
                </button>
              )}
            </div>

            {isCustomCw && (
              <div className="flex items-center gap-2 pt-1">
                <input
                  type="text"
                  value={customCwInput}
                  onChange={(e) => setCustomCwInput(e.target.value)}
                  placeholder="e.g. 1m, 512k, 1000000"
                  className="w-44 rounded-lg border border-slate-200 bg-white px-2.5 py-1 text-xs font-mono text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-hidden transition-all"
                />
                <span className="text-[10px] text-slate-500">
                  {parseContextWindow(customCwInput) < 256_000
                    ? t('providers.contextWindowClampedHint', '低于 256K 将自动保底为 256,000')
                    : `(${parseContextWindow(customCwInput).toLocaleString()} tokens)`}
                </span>
              </div>
            )}
          </div>

          {/* Provider Default Reasoning Levels Section */}
          <div className="rounded-xl border border-slate-200/90 bg-slate-50/50 p-3 space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <label className="block text-xs font-semibold text-slate-700">
                  {t('providers.defaultReasoningLevelsLabel', '服务商默认推理档位')}
                </label>
                <span className="text-[10px] text-slate-400">
                  ({t('providers.reasoningEffortHint', '基线档位包含 low、medium、high，可按需添加 max 等高阶档位')})
                </span>
              </div>
              <span className="text-[11px] font-mono text-slate-500 font-medium">
                {reasoningLevels.join(', ')}
              </span>
            </div>

            <div className="flex flex-wrap items-center gap-1.5">
              {['low', 'medium', 'high', 'xhigh', 'max'].map((tier) => {
                const isActive = reasoningLevels.includes(tier);
                const isMax = tier === 'max';
                return (
                  <button
                    key={tier}
                    type="button"
                    onClick={() => {
                      if (isActive) {
                        if (reasoningLevels.length > 1) {
                          setReasoningLevels(sortReasoningLevels(reasoningLevels.filter((t) => t !== tier)));
                        }
                      } else {
                        setReasoningLevels(sortReasoningLevels([...reasoningLevels, tier]));
                      }
                    }}
                    className={`inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-mono transition-all cursor-pointer ${
                      isActive
                        ? isMax
                          ? 'bg-amber-500 text-white font-bold shadow-2xs'
                          : 'bg-blue-600 text-white font-semibold shadow-2xs'
                        : 'bg-white hover:bg-slate-100 text-slate-600 border border-slate-200 shadow-2xs'
                    }`}
                  >
                    {isMax && <span>⚡</span>}
                    <span>{tier}</span>
                    {isActive ? (
                      <Check className="w-3 h-3 ml-0.5 opacity-80" />
                    ) : (
                      <Plus className="w-3 h-3 ml-0.5 text-slate-400" />
                    )}
                  </button>
                );
              })}

              {/* Custom tiers */}
              {reasoningLevels
                .filter((t) => !['low', 'medium', 'high', 'xhigh', 'max'].includes(t))
                .map((tier) => (
                  <button
                    key={tier}
                    type="button"
                    onClick={() => setReasoningLevels(sortReasoningLevels(reasoningLevels.filter((t) => t !== tier)))}
                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-mono bg-indigo-600 text-white font-semibold shadow-2xs cursor-pointer"
                    title={t('common.delete', 'Click to remove')}
                  >
                    <span>{tier}</span>
                    <X className="w-3 h-3 ml-0.5 opacity-80" />
                  </button>
                ))}

              {/* Add custom tier */}
              {showAddTierInput ? (
                <div className="flex items-center gap-1">
                  <input
                    type="text"
                    value={customTierInput}
                    onChange={(e) => setCustomTierInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        const tClean = customTierInput.trim().toLowerCase();
                        if (tClean && !reasoningLevels.includes(tClean)) {
                          setReasoningLevels(sortReasoningLevels([...reasoningLevels, tClean]));
                        }
                        setCustomTierInput('');
                        setShowAddTierInput(false);
                      } else if (e.key === 'Escape') {
                        setShowAddTierInput(false);
                      }
                    }}
                    placeholder="tier name"
                    autoFocus
                    className="w-24 rounded-lg border border-slate-200 bg-white px-2 py-0.5 text-xs font-mono text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-hidden"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      const tClean = customTierInput.trim().toLowerCase();
                      if (tClean && !reasoningLevels.includes(tClean)) {
                        setReasoningLevels(sortReasoningLevels([...reasoningLevels, tClean]));
                      }
                      setCustomTierInput('');
                      setShowAddTierInput(false);
                    }}
                    className="px-2 py-0.5 rounded-lg bg-blue-600 text-white text-xs cursor-pointer"
                  >
                    <Check className="w-3 h-3" />
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => setShowAddTierInput(true)}
                  className="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs text-slate-500 hover:text-slate-800 bg-white hover:bg-slate-100 border border-dashed border-slate-300 shadow-2xs cursor-pointer"
                >
                  <Plus className="w-3 h-3 text-slate-400" />
                  <span>{t('providers.addReasoningTier', '添加档位')}</span>
                </button>
              )}
            </div>
          </div>

          {/* Local Protocol Gateway Master Switch */}
          <div className="rounded-xl border border-slate-200/90 bg-slate-50/50 p-3 space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setLocalGatewayEnabled(v => !v)}
                  aria-pressed={localGatewayEnabled}
                  className={`relative h-5 w-9 rounded-full transition-colors cursor-pointer ${localGatewayEnabled ? 'bg-blue-600' : 'bg-slate-300'}`}
                >
                  <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${localGatewayEnabled ? 'left-auto right-0.5' : 'left-0.5 right-auto'}`} />
                </button>
                <label className="text-xs font-semibold text-slate-700">
                  {t('providers.localGatewayLabel', '本地协议网关')}
                </label>
              </div>
              <span className="text-[10px] font-mono text-slate-500">
                {localGatewayEnabled
                  ? t('providers.localGatewayOn', '开启时可为模型选择 Responses / Chat；全 Responses 仍直连')
                  : t('providers.localGatewayOff', '关闭：全部模型 Responses 直连')}
              </span>
            </div>

            {/* Read-only global port + availability. The port belongs to the process-wide
                listener, so it is displayed here (where the user reasons about model routing)
                but edited only in Settings — an editable field here would imply a per-provider
                port that a single listener cannot honour. */}
            {gatewayInfo && (
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1 pt-1 border-t border-slate-200/70">
                <span className="text-[10px] text-slate-400 font-medium">
                  {t('providers.gatewayPortShort', '端口')}
                </span>
                <span className="text-[11px] font-mono text-slate-700">{gatewayInfo.port}</span>
                <span
                  className={`px-1.5 py-0.5 rounded-full text-[10px] font-semibold border ${
                    gatewayInfo.state === 'running'
                      ? 'bg-blue-50 text-blue-700 border-blue-200/80'
                      : gatewayInfo.state === 'occupied'
                        ? 'bg-rose-50 text-rose-700 border-rose-200/80'
                        : 'bg-emerald-50 text-emerald-700 border-emerald-200/80'
                  }`}
                >
                  {gatewayInfo.state === 'running'
                    ? t('settings.gateway.statusRunning', '运行中')
                    : gatewayInfo.state === 'occupied'
                      ? t('settings.gateway.statusOccupied', '被占用')
                      : t('settings.gateway.statusFree', '可用')}
                </span>
                <span className="text-[10px] text-slate-400">
                  {gatewayInfo.envOverride
                    ? t('settings.gateway.envOverride', { port: gatewayInfo.port, defaultValue: '端口由环境变量决定' })
                    : t('providers.gatewayPortGlobalHint', '全局设置 · 在「设置」中修改')}
                </span>
              </div>
            )}
          </div>
          {/* Active Model Pool Section */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="block text-xs font-semibold text-slate-700">
                {t('providers.modelsPoolLabel', 'Active Model Pool')}{' '}
                <span className="font-normal text-slate-400">({models.length} {t('providers.modelsCount', 'models')})</span>
              </label>
              <span className="text-[10px] text-slate-400">
                {t('providers.activeModelHint', '单选设为默认生效模型，可独立配置上下文')}
              </span>
            </div>

            {/* Model Rows Container */}
            <div className="rounded-xl border border-slate-200 bg-slate-50/50 p-2 space-y-1.5 max-h-60 overflow-y-auto">
              {models.length === 0 ? (
                <div className="py-6 text-center text-[11px] text-slate-400">
                  {t('providers.noModelsYet', 'No models added yet. Click Fetch Models above or add below.')}
                </div>
              ) : (
                models.map((m) => {
                  const isCurrentActive = m === activeModel;
                  const customModelCw = modelContextWindows[m];
                  const isOverridden = customModelCw !== undefined && customModelCw !== null;
                  const effectiveModelCw = isOverridden
                    ? customModelCw
                    : isCustomCw
                    ? Math.max(256_000, parseContextWindow(customCwInput))
                    : contextWindow;

                  return (
                    <div
                      key={m}
                      className={`flex flex-wrap items-center justify-between gap-x-2.5 gap-y-2 px-3 py-2 rounded-xl border transition-all ${
                        isCurrentActive
                          ? 'bg-blue-50/60 border-blue-300 ring-1 ring-blue-500/15 shadow-2xs'
                          : 'bg-white border-slate-200/90 hover:border-slate-300 shadow-2xs'
                      }`}
                    >
                      {/* Left: Radio/Button for default active + Model Name */}
                      <div className="flex items-center gap-2 min-w-0 grow basis-[13rem]">
                        <button
                          type="button"
                          onClick={() => setActiveModel(m)}
                          className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all shrink-0 cursor-pointer ${
                            isCurrentActive
                              ? 'bg-blue-600 text-white shadow-2xs'
                              : 'bg-slate-100 hover:bg-slate-200 text-slate-600'
                          }`}
                          title={isCurrentActive ? t('providers.currentDefault', '当前默认激活模型') : t('providers.setAsDefault', '点击设为默认激活模型')}
                        >
                          {isCurrentActive && <Check className="w-3 h-3" />}
                          <span>{isCurrentActive ? t('providers.defaultTag', '默认') : t('providers.setAsDefaultBtn', '设为默认')}</span>
                        </button>

                        <span
                          className={`font-mono text-xs truncate ${
                            isCurrentActive ? 'font-bold text-slate-900' : 'text-slate-700'
                          }`}
                          title={m}
                        >
                          {m}
                        </span>
                      </div>

                      {/* Right: Per-model Context Window Selector + Delete Button */}
                      <div className="flex flex-wrap items-center justify-end gap-1.5 shrink-0 ml-auto">
                        <span className="text-[10px] text-slate-400 font-medium">
                          {t('providers.contextWindowLabelShort', '上下文:')}
                        </span>

                        <div className="relative">
                          <select
                            value={isOverridden ? String(customModelCw) : 'default'}
                            onChange={(e) => {
                              const val = e.target.value;
                              if (val === 'default') {
                                setModelContextWindows((prev) => {
                                  const next = { ...prev };
                                  delete next[m];
                                  return next;
                                });
                              } else if (val === 'custom') {
                                const currentDisplay = formatContextWindow(effectiveModelCw);
                                const input = window.prompt(
                                  t('providers.customModelCwPrompt', '设置模型 {{model}} 独立上下文 (如 1m, 512k，输入空则跟随默认):', { model: m }),
                                  currentDisplay
                                );
                                if (input !== null) {
                                  const trimmed = input.trim();
                                  if (!trimmed) {
                                    setModelContextWindows((prev) => {
                                      const next = { ...prev };
                                      delete next[m];
                                      return next;
                                    });
                                  } else {
                                    const parsed = Math.max(256_000, parseContextWindow(trimmed));
                                    setModelContextWindows((prev) => ({ ...prev, [m]: parsed }));
                                  }
                                }
                              } else {
                                const num = parseInt(val, 10);
                                if (!isNaN(num)) {
                                  setModelContextWindows((prev) => ({ ...prev, [m]: num }));
                                }
                              }
                            }}
                            className={`text-xs font-mono py-1 pl-2.5 pr-7 rounded-lg border appearance-none cursor-pointer transition-colors focus:outline-hidden ${
                              isOverridden
                                ? 'bg-emerald-50 text-emerald-800 border-emerald-300 font-semibold'
                                : 'bg-slate-50 text-slate-700 border-slate-200 hover:border-slate-300'
                            }`}
                          >
                            <option value="default">
                              {t('providers.inheritDefault', '跟随默认')} ({formatContextWindow(contextWindow)})
                            </option>
                            <option value="256000">256K</option>
                            <option value="512000">512K</option>
                            <option value="1000000">1M</option>
                            <option value="2000000">2M</option>
                            {isOverridden && ![256_000, 512_000, 1_000_000, 2_000_000].includes(customModelCw) && (
                              <option value={String(customModelCw)}>
                                {formatContextWindow(customModelCw)} ({t('providers.custom', '自定义')})
                              </option>
                            )}
                            <option value="custom">{t('providers.contextWindowCustom', '自定义...')}</option>
                          </select>
                          <ChevronDown className="w-3.5 h-3.5 text-slate-400 absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none" />
                        </div>

                        {/* Model-specific upstream protocol override.
                            One upstream can expose both protocols at once, so this lets a
                            Responses model stay native while a sibling model is translated
                            from Chat Completions by the local gateway. */}
                        {localGatewayEnabled && (() => {
                          const explicit = modelWireApis[m];
                          const isOverriddenProtocol = explicit !== undefined && explicit !== null;
                          const providerDefault = normalizeWireApi(wireApi);
                          const effectiveProtocol = isOverriddenProtocol
                            ? normalizeWireApi(explicit)
                            : providerDefault;
                          const protocolLabel = (value: 'responses' | 'chat') =>
                            value === 'chat'
                              ? t('providers.wireApiChatShort', 'Chat')
                              : t('providers.wireApiResponsesShort', 'Responses');
                          return (
                            <div className="relative shrink-0">
                              <select
                                value={isOverriddenProtocol ? effectiveProtocol : 'default'}
                                onChange={(e) => {
                                  const val = e.target.value;
                                  setModelWireApis((prev) => {
                                    const next = { ...prev };
                                    if (val === 'default') {
                                      delete next[m];
                                    } else {
                                      next[m] = val;
                                    }
                                    return next;
                                  });
                                }}
                                title={t(
                                  'providers.modelWireApiTitle',
                                  '该模型的上游协议。Responses 直连上游；Chat Completions 经本地协议网关转换。'
                                )}
                                className={`text-xs py-1 pl-2.5 pr-7 rounded-lg border appearance-none cursor-pointer transition-colors focus:outline-hidden ${
                                  isOverriddenProtocol
                                    ? 'bg-amber-50 text-amber-800 border-amber-300 font-semibold'
                                    : 'bg-slate-50 text-slate-700 border-slate-200 hover:border-slate-300'
                                }`}
                              >
                                <option value="default">
                                  {t('providers.inheritDefault', '跟随默认')} ({protocolLabel(providerDefault)})
                                </option>
                                <option value="responses">{protocolLabel('responses')}</option>
                                <option value="chat">{protocolLabel('chat')}</option>
                              </select>
                              <ChevronDown className="w-3.5 h-3.5 text-slate-400 absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none" />
                            </div>
                          );
                        })()}

                        {/* Model-specific Max Reasoning toggle */}
                        {(() => {
                          const currentModelLevels = modelReasoningLevels[m] ?? reasoningLevels;
                          const hasMax = currentModelLevels.includes('max');
                          return (
                            <button
                              type="button"
                              onClick={() => {
                                const nextLevels = hasMax
                                  ? currentModelLevels.filter((lvl) => lvl !== 'max')
                                  : sortReasoningLevels([...currentModelLevels, 'max']);
                                setModelReasoningLevels((prev) => ({
                                  ...prev,
                                  [m]: sortReasoningLevels(nextLevels),
                                }));
                              }}
                              className={`inline-flex items-center gap-0.5 px-2 py-1 rounded-lg text-xs font-mono transition-all cursor-pointer ${
                                hasMax
                                  ? 'bg-amber-500 text-white font-bold shadow-2xs'
                                  : 'bg-slate-100 hover:bg-slate-200 text-slate-500'
                              }`}
                              title={
                                hasMax
                                  ? t('providers.modelReasoningBadgeTitle', '独立推理档位: {{levels}}', { levels: currentModelLevels.join(', ') })
                                  : t('providers.maxReasoningBadge', '点击为该模型开启 Max 推理')
                              }
                            >
                              <span>⚡</span>
                              <span>Max</span>
                            </button>
                          );
                        })()}

                        <button
                          type="button"
                          onClick={() => handleRemoveModel(m)}
                          className="p-1 rounded-lg text-slate-400 hover:text-rose-600 hover:bg-rose-50 transition-colors cursor-pointer"
                          title={t('common.delete', '删除')}
                        >
                          <X className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                  );
                })
              )}
            </div>

            {/* Manual Model Adder */}
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={newModelInput}
                onChange={(e) => setNewModelInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    handleAddModel();
                  }
                }}
                placeholder={t('providers.addModelPlaceholder', 'Type model ID (e.g. step-5-preview, deepseek-chat) and hit Enter')}
                className="flex-1 rounded-xl border border-slate-200 bg-slate-50/50 px-3 py-1.5 text-xs font-mono text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:bg-white focus:outline-hidden transition-all"
              />
              <button
                type="button"
                onClick={() => handleAddModel()}
                className="inline-flex items-center gap-1 px-3 py-1.5 rounded-xl text-xs font-medium text-slate-700 bg-white hover:bg-slate-50 border border-slate-200 shadow-2xs transition-all shrink-0"
              >
                <Plus className="w-3.5 h-3.5" />
                <span>{t('providers.addModelBtn', 'Add')}</span>
              </button>
            </div>
          </div>

          {/* Advanced Settings Collapsible Accordion */}
          <div className="border border-slate-200/80 rounded-xl bg-slate-50/40 overflow-hidden transition-all">
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="w-full flex items-center justify-between p-3 text-xs font-bold text-slate-700 hover:text-slate-900 transition-colors"
            >
              <div className="flex items-center gap-2">
                <Settings2 className="w-4 h-4 text-slate-500" />
                <span>{t('providers.advancedSettings', 'Advanced Settings')}</span>
              </div>
              {showAdvanced ? (
                <ChevronUp className="w-4 h-4 text-slate-400" />
              ) : (
                <ChevronDown className="w-4 h-4 text-slate-400" />
              )}
            </button>

            {showAdvanced && (
              <div className="p-3 pt-0 space-y-3.5 border-t border-slate-100">
                {/* Notes */}
                <div className="grid grid-cols-1 gap-3 pt-2">
                  <div>
                    <label className="block text-xs font-semibold text-slate-700 mb-1">
                      {t('providers.notesLabel', 'Notes / Description')}
                    </label>
                    <input
                      type="text"
                      value={notes}
                      onChange={(e) => setNotes(e.target.value)}
                      placeholder="e.g. Dedicated production account"
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-hidden transition-all"
                    />
                  </div>
                </div>

                {/* config.toml Textarea */}
                <div className="space-y-1">
                  <div className="flex items-center justify-between">
                    <label className="block text-xs font-semibold text-slate-700 font-mono">
                      {t('providers.customConfigTomlLabel', 'config.toml Configuration')}
                    </label>
                    <button
                      type="button"
                      onClick={() => {
                        setCustomConfigToml(generateDefaultConfigToml(activeModel, localGatewayEnabled ? wireApi : 'responses'));
                        setConfigTomlManuallyEdited(false);
                      }}
                      className="inline-flex items-center gap-1 text-[11px] text-blue-600 hover:text-blue-800 transition-colors"
                    >
                      <RotateCcw className="w-3 h-3" />
                      <span>{t('providers.resetToTemplate', 'Reset to Template')}</span>
                    </button>
                  </div>
                  <p className="text-[10px] text-slate-400">
                    {t('providers.customConfigTomlHint', 'Merged into ~/.codex/config.toml when switched; leave blank for auto-generated template.')}
                  </p>
                  <textarea
                    value={customConfigToml}
                    onChange={(e) => {
                      setCustomConfigToml(e.target.value);
                      setConfigTomlManuallyEdited(true);
                    }}
                    rows={6}
                    className="w-full rounded-xl border border-slate-200 bg-white p-2.5 font-mono text-[11px] text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-hidden transition-all"
                    placeholder={`[model_providers.${providerSlug}]\nname = "..."\nbase_url = "..."\n`}
                  />
                </div>

                {/* auth.json Textarea */}
                <div className="space-y-1">
                  <div className="flex items-center justify-between">
                    <label className="block text-xs font-semibold text-slate-700 font-mono">
                      {t('providers.customAuthJsonLabel', 'auth.json Payload')}
                    </label>
                    <button
                      type="button"
                      onClick={() => {
                        setCustomAuthJson(DEFAULT_AUTH_JSON);
                        setAuthJsonManuallyEdited(false);
                      }}
                      className="inline-flex items-center gap-1 text-[11px] text-blue-600 hover:text-blue-800 transition-colors"
                    >
                      <RotateCcw className="w-3 h-3" />
                      <span>{t('providers.resetToTemplate', 'Reset to Template')}</span>
                    </button>
                  </div>
                  <p className="text-[10px] text-slate-400">
                    {t('providers.customAuthJsonHint', 'Written to ~/.codex/auth.json when switched; leave blank for standard structure.')}
                  </p>
                  <textarea
                    value={customAuthJson}
                    onChange={(e) => {
                      setCustomAuthJson(e.target.value);
                      setAuthJsonManuallyEdited(true);
                    }}
                    rows={4}
                    className="w-full rounded-xl border border-slate-200 bg-white p-2.5 font-mono text-[11px] text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-hidden transition-all"
                    placeholder={`{\n  "auth_mode": "api_key"\n}`}
                  />
                </div>

                {/* Model Catalog Path info */}
                {catalogPath && (
                  <div className="space-y-1.5 p-2.5 rounded-xl border border-slate-200/80 bg-white">
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-semibold text-slate-700 flex items-center gap-1.5 font-mono">
                        <FileCode className="w-3.5 h-3.5 text-blue-600" />
                        {t('providers.catalogPath', '模型目录文件路径')}
                      </span>
                      <div className="flex items-center gap-1.5">
                        <button
                          type="button"
                          onClick={async () => {
                            try {
                              await navigator.clipboard.writeText(catalogPath);
                              showToast(t('providers.copiedCatalogPath', '已复制模型目录路径'), 'success');
                            } catch {
                              // fallback
                            }
                          }}
                          className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-medium text-slate-600 hover:text-slate-900 bg-slate-100 hover:bg-slate-200 transition-colors cursor-pointer"
                          title={t('providers.copyCatalogPath', '复制路径')}
                        >
                          <Copy className="w-3 h-3" />
                          <span>{t('providers.copyCatalogPath', '复制路径')}</span>
                        </button>
                        <button
                          type="button"
                          onClick={async () => {
                            try {
                              await invoke('open_model_catalog', { providerId: editingProvider?.id || providerSlug });
                            } catch (err: any) {
                              showToast(String(err), 'error');
                            }
                          }}
                          className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-medium text-blue-700 hover:text-blue-900 bg-blue-50 hover:bg-blue-100 border border-blue-200/80 transition-colors cursor-pointer"
                          title={t('providers.openCatalogFile', '打开模型配置 (JSON)')}
                        >
                          <ExternalLink className="w-3 h-3" />
                          <span>{t('providers.openCatalogFile', '打开模型配置')}</span>
                        </button>
                      </div>
                    </div>
                    <p className="text-[11px] font-mono text-slate-500 break-all select-all bg-slate-50 p-1.5 rounded-lg border border-slate-200/60">
                      {catalogPath}
                    </p>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2.5 border-t border-slate-100 bg-slate-50/40 px-6 py-4 shrink-0">
          <button
            type="button"
            onClick={onClose}
            className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-xs font-medium text-slate-700 hover:bg-slate-50 shadow-2xs transition-all"
          >
            {t('common.cancel', 'Cancel')}
          </button>

          <button
            type="button"
            onClick={() => handleSave(false)}
            disabled={saving}
            className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-xs font-semibold text-slate-700 hover:bg-slate-50 shadow-2xs transition-all disabled:opacity-50"
          >
            {t('providers.saveOnly', 'Save')}
          </button>

          <button
            type="button"
            onClick={() => handleSave(true)}
            disabled={saving}
            className="rounded-xl bg-blue-600 hover:bg-blue-700 px-4 py-2 text-xs font-semibold text-white shadow-xs shadow-blue-600/20 transition-all disabled:opacity-50"
          >
            {t('providers.saveAndSwitch', 'Save & Switch')}
          </button>
        </div>
      </div>
    </div>
  );
};
