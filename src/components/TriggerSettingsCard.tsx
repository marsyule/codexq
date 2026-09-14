import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { ToastPayload } from '../types';
import { Sparkles, Plus, X } from 'lucide-react';

interface TriggerSettingsCardProps {
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
}

export const TriggerSettingsCard: React.FC<TriggerSettingsCardProps> = ({ showToast }) => {
  const { t } = useTranslation();
  const [defaultModel, setDefaultModel] = useState<string>('gpt-5.6-luna');
  const [presetModels, setPresetModels] = useState<string[]>(['gpt-5.6-luna', 'o3-mini', 'gpt-4o']);
  const [warmupPrompt, setWarmupPrompt] = useState<string>('ping');
  const [skipIfActive, setSkipIfActive] = useState<boolean>(true);
  const [saving, setSaving] = useState<boolean>(false);

  const [showAddPreset, setShowAddPreset] = useState<boolean>(false);
  const [newPresetInput, setNewPresetInput] = useState<string>('');

  const fetchSettings = useCallback(async () => {
    try {
      const res = await invoke<Record<string, string>>('get_app_settings');
      if (res) {
        if (res['warmup.default_model']) setDefaultModel(res['warmup.default_model']);
        if (res['warmup.preset_models']) {
          try {
            const parsed = JSON.parse(res['warmup.preset_models']);
            if (Array.isArray(parsed) && parsed.length > 0) setPresetModels(parsed);
          } catch (e) {
            console.warn('Failed to parse preset models', e);
          }
        }
        if (res['warmup.prompt']) setWarmupPrompt(res['warmup.prompt']);
        if (res['warmup.skip_if_active']) setSkipIfActive(res['warmup.skip_if_active'] === 'true');
      }
    } catch (err) {
      console.warn('Failed to load settings', err);
    }
  }, []);

  useEffect(() => {
    fetchSettings();
  }, [fetchSettings]);

  const updateSetting = async (key: string, value: string) => {
    setSaving(true);
    try {
      await invoke('set_app_setting', { key, value });
    } catch (err) {
      console.error('Failed to update setting', err);
      showToast(`保存失败: ${err}`, 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleModelChange = (val: string) => {
    setDefaultModel(val);
    updateSetting('warmup.default_model', val);
  };

  const handleDeletePreset = (tag: string) => {
    const updated = presetModels.filter((m) => m !== tag);
    setPresetModels(updated);
    updateSetting('warmup.preset_models', JSON.stringify(updated));
  };

  const handleAddPreset = () => {
    const trimmed = newPresetInput.trim();
    if (!trimmed) return;
    if (!presetModels.includes(trimmed)) {
      const updated = [...presetModels, trimmed];
      setPresetModels(updated);
      updateSetting('warmup.preset_models', JSON.stringify(updated));
      showToast({ key: 'toasts.settingsSaved' }, 'success');
    }
    setNewPresetInput('');
    setShowAddPreset(false);
  };

  return (
    <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
      <div className="flex items-center justify-between pb-2 border-b border-slate-100">
        <div className="flex items-center gap-2.5">
          <div className="p-2 rounded-xl bg-indigo-50 text-indigo-600">
            <Sparkles className="w-4 h-4" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-base font-bold text-slate-900">{t('settings.trigger.title')}</h3>
              {saving && (
                <span className="text-[10px] bg-blue-50 text-blue-600 px-1.5 py-0.5 rounded font-medium animate-pulse">
                  ...
                </span>
              )}
            </div>
            <p className="text-xs text-slate-500 mt-0.5">{t('settings.trigger.defaultModelDesc')}</p>
          </div>
        </div>
      </div>

      {/* Default Model */}
      <div className="py-2 border-b border-slate-100 space-y-2">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-semibold text-slate-800">{t('settings.trigger.defaultModel')}</h4>
        </div>
        <input
          type="text"
          value={defaultModel}
          onChange={(e) => handleModelChange(e.target.value)}
          placeholder="gpt-5.6-luna"
          className="w-full max-w-md px-3 py-1.5 text-xs rounded-lg border border-slate-300 bg-white font-mono font-bold text-blue-600 focus:border-blue-500 focus:outline-none"
        />

        {/* Preset Chips */}
        <div className="flex items-center gap-1.5 flex-wrap pt-1">
          <span className="text-[11px] text-slate-400">{t('settings.trigger.presetModels')}:</span>
          {presetModels.map((m) => {
            const isSelected = m === defaultModel;
            return (
              <div
                key={m}
                className={`flex items-center gap-1 text-[10px] pl-2 pr-1 py-0.5 rounded-md font-mono font-semibold transition-all border ${
                  isSelected
                    ? 'bg-blue-600 text-white border-blue-600 shadow-2xs'
                    : 'bg-slate-50 text-slate-700 border-slate-200 hover:bg-slate-100'
                }`}
              >
                <span
                  onClick={() => handleModelChange(m)}
                  className="cursor-pointer select-none"
                  title={`${m}`}
                >
                  {m}
                </span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDeletePreset(m);
                  }}
                  className="p-0.5 rounded hover:bg-black/10 transition-colors"
                >
                  <X className={`w-2.5 h-2.5 ${isSelected ? 'stroke-white' : 'stroke-slate-400 hover:stroke-rose-600'}`} />
                </button>
              </div>
            );
          })}

          {showAddPreset ? (
            <div className="flex items-center gap-1">
              <input
                type="text"
                placeholder={t('settings.trigger.addPresetPlaceholder')}
                value={newPresetInput}
                onChange={(e) => setNewPresetInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleAddPreset()}
                className="px-1.5 py-0.5 text-[11px] rounded border border-blue-300 bg-white font-mono focus:outline-none w-28"
                autoFocus
              />
              <button
                onClick={handleAddPreset}
                className="px-2 py-0.5 text-[10px] rounded bg-blue-600 text-white font-medium"
              >
                OK
              </button>
              <button
                onClick={() => setShowAddPreset(false)}
                className="text-[10px] text-slate-400 hover:text-slate-600 px-1"
              >
                ✕
              </button>
            </div>
          ) : (
            <button
              onClick={() => setShowAddPreset(true)}
              className="flex items-center gap-1 text-[10px] px-2 py-0.5 rounded border border-dashed border-slate-300 text-slate-500 hover:text-blue-600 hover:border-blue-400 transition-all"
            >
              <Plus className="w-2.5 h-2.5" />
            </button>
          )}
        </div>
      </div>

      {/* Prompt */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 py-2 border-b border-slate-100">
        <div>
          <h4 className="text-sm font-semibold text-slate-800">{t('settings.trigger.prompt')}</h4>
          <p className="text-xs text-slate-500">{t('settings.trigger.promptDesc')}</p>
        </div>
        <input
          type="text"
          value={warmupPrompt}
          onChange={(e) => {
            setWarmupPrompt(e.target.value);
            updateSetting('warmup.prompt', e.target.value);
          }}
          className="w-48 px-3 py-1 text-xs rounded-lg border border-slate-300 bg-white font-mono text-slate-700 focus:border-blue-500 focus:outline-none"
        />
      </div>

      {/* Skip if active */}
      <div className="flex items-center justify-between py-2">
        <div>
          <h4 className="text-sm font-semibold text-slate-800">{t('settings.trigger.skipIfActive')}</h4>
          <p className="text-xs text-slate-500">{t('settings.trigger.skipIfActiveDesc')}</p>
        </div>
        <label className="relative inline-flex items-center cursor-pointer">
          <input
            type="checkbox"
            checked={skipIfActive}
            onChange={(e) => {
              setSkipIfActive(e.target.checked);
              updateSetting('warmup.skip_if_active', e.target.checked ? 'true' : 'false');
            }}
            className="sr-only peer"
          />
          <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
        </label>
      </div>
    </div>
  );
};
