import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import {
  Edit3,
  Trash2,
  CheckCircle2,
  RefreshCw,
  ChevronDown,
  Layers,
  ArrowRightLeft,
  Server,
  FileCode,
} from 'lucide-react';
import {
  type ProviderData,
  type ConnectivityResult,
  type ToastPayload,
  formatContextWindow,
  getModelReasoningLevels,
  getModelWireApi,
  hasMaxReasoning,
} from '../types';

interface ProviderCardProps {
  provider: ProviderData;
  isActive: boolean;
  onSwitch: (providerId: string, modelOverride?: string) => Promise<void>;
  onEdit: (provider: ProviderData) => void;
  onDelete: (provider: ProviderData) => void;
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
}

/**
 * Card component for rendering a configured third-party AI provider in light Clash Verge theme.
 *
 * Provides in-card model selection, connectivity diagnostic testing,
 * and seamless one-click runtime slot switching.
 */
export const ProviderCard: React.FC<ProviderCardProps> = ({
  provider,
  isActive,
  onSwitch,
  onEdit,
  onDelete,
  showToast,
}) => {
  const { t } = useTranslation();
  const [selectedModel, setSelectedModel] = useState(
    provider.active_model || (provider.models[0] ?? 'default')
  );
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectivityResult | null>(null);
  const [switching, setSwitching] = useState(false);

  const effectiveCw =
    (provider.model_context_windows && provider.model_context_windows[selectedModel]) ||
    provider.context_window ||
    256_000;

  const effectiveLevels = getModelReasoningLevels(provider, selectedModel);
  const supportsMax = hasMaxReasoning(effectiveLevels);

  // Protocol is resolved per model: a mixed-protocol provider serves some models natively
  // and translates others from Chat Completions through the loopback gateway.
  const selectedWireApi = getModelWireApi(provider, selectedModel);

  const handleOpenCatalog = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke('open_model_catalog', { providerId: provider.id });
    } catch (err: any) {
      showToast(String(err), 'error');
    }
  };

  const handleShowCatalogInFolder = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke('show_model_catalog_in_folder', { providerId: provider.id });
    } catch (err: any) {
      showToast(String(err), 'error');
    }
  };

  const initial = provider.name.trim().charAt(0).toUpperCase() || 'P';

  const handleTest = async () => {
    setTesting(true);
    try {
      const res = await invoke<ConnectivityResult>('test_provider_connectivity', {
        baseUrl: provider.base_url,
        apiKey: '',
        providerId: provider.id,
      });
      setTestResult(res);
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
      setTestResult({
        success: false,
        message: String(err),
        available_models: [],
      });
      showToast(String(err), 'error');
    } finally {
      setTesting(false);
    }
  };

  const handleSwitch = async () => {
    setSwitching(true);
    try {
      await onSwitch(provider.id, selectedModel);
    } finally {
      setSwitching(false);
    }
  };

  return (
    <div
      className={`group relative flex flex-col justify-between overflow-hidden rounded-xl border p-3.5 shadow-xs transition-all duration-200 ${
        isActive
          ? 'border-blue-300 bg-blue-50/20 shadow-xs ring-1 ring-blue-500/15'
          : 'border-slate-200/90 bg-white hover:border-blue-300 hover:shadow-sm'
      }`}
    >
      {/* Top Details */}
      <div className="space-y-3">
        {/* Header row: Avatar, Name, Badges */}
        <div className="flex items-start justify-between gap-2.5">
          <div className="flex items-center gap-2.5 min-w-0 flex-1">
            <div
              className={`flex h-9 w-9 items-center justify-center rounded-xl font-bold text-sm shrink-0 transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white shadow-xs'
                  : 'bg-blue-100 text-blue-700'
              }`}
            >
              {initial}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5 flex-wrap">
                <h3
                  className="truncate text-sm font-semibold text-slate-900 group-hover:text-blue-600 transition-colors"
                  title={provider.name}
                >
                  {provider.name}
                </h3>
                {isActive && (
                  <span className="inline-flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.2 rounded-full border bg-emerald-50 text-emerald-700 border-emerald-200/80">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-500 animate-pulse"></span>
                    {t('providers.activeBadge', 'Active')}
                  </span>
                )}
                {provider.wire_api && provider.wire_api !== 'responses' && (
                  <span className="text-[9px] font-mono px-1.5 py-0.2 rounded-full border bg-slate-100 text-slate-600 border-slate-200">
                    {provider.wire_api}
                  </span>
                )}
              </div>
              <p className="truncate text-[11px] text-slate-400 font-mono mt-0.5" title={provider.base_url}>
                {provider.base_url}
              </p>
            </div>
          </div>
        </div>

        {/* Note if available */}
        {provider.notes && (
          <p className="text-[11px] text-slate-500 bg-slate-50 border border-slate-200/70 rounded-lg px-2.5 py-1 italic line-clamp-2">
            "{provider.notes}"
          </p>
        )}

        {/* Model Selection Dropdown */}
        <div className="space-y-1">
          <div className="flex items-center justify-between text-[11px]">
            <span className="flex items-center gap-1 text-slate-500 font-medium">
              <Layers className="w-3 h-3 text-blue-600" />
              {t('providers.targetModel', 'Target Model')}
            </span>
            <div className="flex items-center gap-1.5">
              {supportsMax && (
                <span
                  className="inline-flex items-center gap-0.5 text-[10px] font-mono px-1.5 py-0.2 rounded border bg-amber-50 text-amber-700 border-amber-200/80 font-semibold"
                  title={t('providers.maxReasoningBadge', '支持 Max')}
                >
                  <span>⚡</span>
                  <span>Max</span>
                </span>
              )}
              <span
                className="inline-flex items-center text-[10px] font-mono px-1.5 py-0.2 rounded border bg-blue-50/80 text-blue-700 border-blue-200/80 font-medium"
                title={t('providers.contextWindowBadgeTitle', 'Context window: {{tokens}} tokens', {
                  tokens: effectiveCw.toLocaleString(),
                })}
              >
                {formatContextWindow(effectiveCw)}
              </span>
              <span
                className={`inline-flex items-center text-[10px] font-mono px-1.5 py-0.2 rounded border font-medium ${
                  selectedWireApi === 'chat'
                    ? 'bg-amber-50/80 text-amber-700 border-amber-200/80'
                    : 'bg-emerald-50/80 text-emerald-700 border-emerald-200/80'
                }`}
                title={
                  selectedWireApi === 'chat'
                    ? t(
                        'providers.wireApiChatBadgeTitle',
                        '该模型仅支持 Chat Completions，请求经本地协议网关自动转换'
                      )
                    : t('providers.wireApiResponsesBadgeTitle', '该模型原生支持 Responses，直连上游')
                }
              >
                {selectedWireApi === 'chat' ? '⇄ Chat' : 'Responses'}
              </span>
              <span className="text-[10px] text-slate-400 font-mono">
                {provider.models.length} {t('providers.modelsAvailable', 'models')}
              </span>
            </div>
          </div>

          <div className="relative">
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              className="w-full appearance-none px-2.5 py-1.5 bg-slate-50/70 border border-slate-200 rounded-lg text-xs font-mono text-slate-800 focus:bg-white focus:border-blue-500 focus:outline-hidden transition-colors pr-8 cursor-pointer"
            >
              {provider.models.length === 0 ? (
                <option value={provider.active_model}>
                  {provider.active_model} [{formatContextWindow(effectiveCw)}{supportsMax ? ' · ⚡Max' : ''}]
                </option>
              ) : (
                provider.models.map((m) => {
                  const mCw =
                    (provider.model_context_windows && provider.model_context_windows[m]) ||
                    provider.context_window ||
                    256_000;
                  const mLevels = getModelReasoningLevels(provider, m);
                  const mHasMax = hasMaxReasoning(mLevels);
                  const mWireApi = getModelWireApi(provider, m);
                  return (
                    <option key={m} value={m}>
                      {m} [{formatContextWindow(mCw)}{mHasMax ? ' · ⚡Max' : ''}{mWireApi === 'chat' ? ' · ⇄Chat' : ''}] {m === provider.active_model ? `(${t('providers.defaultTag', 'Default')})` : ''}
                    </option>
                  );
                })
              )}
            </select>
            <ChevronDown className="w-3.5 h-3.5 text-slate-400 absolute right-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
          </div>
        </div>

        {/* Diagnostic Status Pill */}
        <div className="flex items-center justify-between p-2 bg-slate-50 border border-slate-200/80 rounded-lg text-[11px]">
          <div className="flex items-center gap-2 min-w-0">
            <span
              className={`w-2 h-2 rounded-full shrink-0 ${
                testResult
                  ? testResult.success
                    ? 'bg-emerald-500'
                    : 'bg-rose-500'
                  : 'bg-slate-300'
              }`}
            />
            <span className="font-mono text-slate-600 truncate">
              {testResult ? (
                <span className={testResult.success ? 'text-emerald-700 font-semibold' : 'text-rose-600'}>
                  {testResult.success
                    ? `HTTP ${testResult.status_code || 200} (${testResult.latency_ms}ms)`
                    : testResult.message}
                </span>
              ) : (
                <span className="text-slate-400">
                  {provider.key_masked || t('providers.keyConfigured', 'Key configured')}
                </span>
              )}
            </span>
          </div>

          <button
            type="button"
            onClick={handleTest}
            disabled={testing}
            className="p-1 text-slate-400 hover:text-blue-600 hover:bg-slate-200/60 rounded-md transition-colors disabled:opacity-40"
            title={t('providers.testConnection', 'Test Connection')}
          >
            <RefreshCw className={`w-3 h-3 ${testing ? 'animate-spin text-blue-600' : ''}`} />
          </button>
        </div>
      </div>

      {/* Card Footer: Actions */}
      <div className="mt-3 flex items-center justify-between border-t border-slate-100/90 pt-2.5">
        <button
          type="button"
          onClick={handleOpenCatalog}
          onContextMenu={(e) => {
            e.preventDefault();
            handleShowCatalogInFolder(e);
          }}
          className="flex items-center gap-1 text-[11px] font-mono text-slate-400 hover:text-blue-600 transition-colors cursor-pointer"
          title={`${t('providers.openCatalogFile', '打开模型配置 (JSON)')} (右键: ${t('providers.showCatalogInFolder', '在文件夹中定位')})`}
        >
          <Server className="h-3 w-3" />
          <span>{provider.id.slice(0, 12)}</span>
        </button>

        <div className="flex items-center gap-1">
          {/* Open Model Catalog File */}
          <button
            type="button"
            onClick={handleOpenCatalog}
            onContextMenu={(e) => {
              e.preventDefault();
              handleShowCatalogInFolder(e);
            }}
            className="p-1 rounded-md text-slate-400 hover:text-blue-600 hover:bg-blue-50 transition-colors cursor-pointer"
            title={`${t('providers.openCatalogFile', '打开模型配置 (JSON)')} (右键: ${t('providers.showCatalogInFolder', '在文件夹中定位')})`}
          >
            <FileCode className="w-3.5 h-3.5" />
          </button>

          {/* Edit Button */}
          <button
            onClick={() => onEdit(provider)}
            className="p-1 rounded-md text-slate-400 hover:text-slate-700 hover:bg-slate-100 transition-colors cursor-pointer"
            title={t('common.edit', 'Edit')}
          >
            <Edit3 className="w-3.5 h-3.5" />
          </button>

          {/* Delete Button */}
          <button
            onClick={() => onDelete(provider)}
            className="p-1 rounded-md text-slate-400 hover:text-rose-600 hover:bg-rose-50 transition-colors cursor-pointer"
            title={t('common.delete', 'Delete')}
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>

          {/* Switch Button */}
          {isActive ? (
            <span className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-semibold text-emerald-700 bg-emerald-50 border border-emerald-200/80 shadow-2xs">
              <CheckCircle2 className="w-3 h-3 text-emerald-600" />
              <span>{t('providers.inUse', 'In Use')}</span>
            </span>
          ) : (
            <button
              onClick={handleSwitch}
              disabled={switching}
              className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 shadow-2xs transition-all disabled:opacity-50"
              title={t('providers.activateBtn', 'Switch to this Provider')}
            >
              <ArrowRightLeft className={`w-3 h-3 ${switching ? 'animate-spin' : ''}`} />
              <span>{switching ? t('providers.switching', 'Switching...') : t('accounts.switchTo', 'Switch')}</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
