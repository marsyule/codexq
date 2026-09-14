import React, { useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { ToastPayload } from '../types';
import {
  X,
  UserPlus,
  Terminal,
  FileCode2,
  UploadCloud,
  CheckCircle2,
  Loader2,
  ExternalLink,
  ArrowRight,
  AlertCircle,
} from 'lucide-react';

/**
 * Props for the {@link AddAccountModal} component.
 */
interface AddAccountModalProps {
  /** Whether the modal is currently visible. */
  isOpen: boolean;
  /** Callback to dismiss the modal. */
  onClose: () => void;
  /** Callback triggered upon successful account addition or synchronization. */
  onSuccess: (email?: string) => void;
  /** Toast display callback for feedback alerts. */
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
}

/**
 * Modal component providing two methods to onboard Codex accounts:
 * 1. Spawning official `codex login` in an external terminal.
 * 2. Importing an existing `auth.json` credential file or JSON payload.
 *
 * @param props - Component props.
 * @returns Modal overlay and dialog elements.
 */
export const AddAccountModal: React.FC<AddAccountModalProps> = ({
  isOpen,
  onClose,
  onSuccess,
  showToast,
}) => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<'terminal' | 'file'>('terminal');

  // Terminal login state
  const [launchingTerminal, setLaunchingTerminal] = useState(false);
  const [terminalLaunched, setTerminalLaunched] = useState(false);
  const [syncing, setSyncing] = useState(false);

  // File import state
  const [importing, setImporting] = useState(false);
  const [manualPath, setManualPath] = useState('');
  const [selectedFileName, setSelectedFileName] = useState<string | null>(null);
  const [selectedFileContent, setSelectedFileContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  if (!isOpen) return null;

  const handleLaunchTerminal = async () => {
    setLaunchingTerminal(true);
    setError(null);
    try {
      await invoke('launch_codex_login');
      setTerminalLaunched(true);
      showToast({ key: 'addAccount.terminalLaunchedToast' }, 'info');
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to launch terminal';
      setError(msg);
      showToast(msg, 'error');
    } finally {
      setLaunchingTerminal(false);
    }
  };

  const handleSyncAfterLogin = async () => {
    setSyncing(true);
    setError(null);
    try {
      await invoke('refresh_all', { concurrency: 5 });
      showToast({ key: 'addAccount.syncSuccessToast' }, 'success');
      onSuccess();
      onClose();
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to sync accounts';
      setError(msg);
    } finally {
      setSyncing(false);
    }
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setSelectedFileName(file.name);
    setError(null);

    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      setSelectedFileContent(content);
    };
    reader.onerror = () => {
      setError(t('addAccount.fileReadError') || 'Failed to read the selected file.');
    };
    reader.readAsText(file);
  };

  const handleImport = async () => {
    setImporting(true);
    setError(null);
    try {
      let res: any;
      if (selectedFileContent) {
        res = await invoke('import_auth_content', { content: selectedFileContent });
      } else if (manualPath.trim()) {
        res = await invoke('import_auth_file', { path: manualPath.trim() });
      } else {
        setError(t('addAccount.noFileSelected') || 'Please select a file or enter a valid file path.');
        setImporting(false);
        return;
      }

      const email = res?.email || res?.profile_id || '';
      const isNew = res?.is_new ?? true;

      showToast(
        isNew
          ? { key: 'addAccount.importSuccessNew', params: { email } }
          : { key: 'addAccount.importSuccessUpdated', params: { email } },
        'success'
      );
      onSuccess(email);
      onClose();
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to import auth.json';
      setError(msg);
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/45 p-4 backdrop-blur-xs animate-in fade-in duration-150">
      <div
        className="relative w-full max-w-lg overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-slate-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-blue-100/80 text-blue-700">
              <UserPlus className="h-5 w-5" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-slate-900">{t('addAccount.title')}</h3>
              <p className="text-[11px] text-slate-500">{t('addAccount.subtitle')}</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Tab Switcher */}
        <div className="flex border-b border-slate-100 bg-slate-50/50 px-6 pt-2">
          <button
            onClick={() => {
              setActiveTab('terminal');
              setError(null);
            }}
            className={`flex items-center gap-2 pb-2.5 pt-1 text-xs font-semibold border-b-2 transition-all mr-6 ${
              activeTab === 'terminal'
                ? 'border-blue-600 text-blue-700'
                : 'border-transparent text-slate-500 hover:text-slate-800'
            }`}
          >
            <Terminal className="h-3.5 w-3.5" />
            <span>{t('addAccount.tabTerminal')}</span>
          </button>

          <button
            onClick={() => {
              setActiveTab('file');
              setError(null);
            }}
            className={`flex items-center gap-2 pb-2.5 pt-1 text-xs font-semibold border-b-2 transition-all ${
              activeTab === 'file'
                ? 'border-blue-600 text-blue-700'
                : 'border-transparent text-slate-500 hover:text-slate-800'
            }`}
          >
            <FileCode2 className="h-3.5 w-3.5" />
            <span>{t('addAccount.tabFile')}</span>
          </button>
        </div>

        {/* Modal Body */}
        <div className="p-6 space-y-4">
          {error && (
            <div className="flex items-start gap-2 rounded-xl border border-rose-200 bg-rose-50/90 p-3 text-xs text-rose-700">
              <AlertCircle className="h-4 w-4 shrink-0 mt-0.5 text-rose-600" />
              <span>{error}</span>
            </div>
          )}

          {activeTab === 'terminal' ? (
            <div className="space-y-4">
              <div className="rounded-xl border border-slate-200/80 bg-slate-50/70 p-4 space-y-3">
                <div className="flex items-start gap-3 text-xs text-slate-600">
                  <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-blue-100 text-[11px] font-bold text-blue-700">
                    1
                  </span>
                  <p>{t('addAccount.step1')}</p>
                </div>
                <div className="flex items-start gap-3 text-xs text-slate-600">
                  <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-blue-100 text-[11px] font-bold text-blue-700">
                    2
                  </span>
                  <p>{t('addAccount.step2')}</p>
                </div>
                <div className="flex items-start gap-3 text-xs text-slate-600">
                  <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-blue-100 text-[11px] font-bold text-blue-700">
                    3
                  </span>
                  <p>{t('addAccount.step3')}</p>
                </div>
              </div>

              <div className="flex flex-col sm:flex-row items-center gap-3 pt-1">
                <button
                  type="button"
                  onClick={handleLaunchTerminal}
                  disabled={launchingTerminal}
                  className="flex-1 flex items-center justify-center gap-2 rounded-xl border border-blue-200 bg-blue-50/80 px-4 py-2.5 text-xs font-semibold text-blue-700 hover:bg-blue-100 hover:border-blue-300 shadow-xs transition-all disabled:opacity-50 w-full"
                >
                  {launchingTerminal ? (
                    <Loader2 className="h-4 w-4 animate-spin text-blue-600" />
                  ) : (
                    <ExternalLink className="h-4 w-4 text-blue-600" />
                  )}
                  <span>{t('addAccount.launchBtn')}</span>
                </button>

                <button
                  type="button"
                  onClick={handleSyncAfterLogin}
                  disabled={syncing}
                  className="flex-1 flex items-center justify-center gap-2 rounded-xl bg-blue-600 px-4 py-2.5 text-xs font-semibold text-white hover:bg-blue-700 shadow-xs shadow-blue-600/20 transition-all disabled:opacity-50 w-full"
                >
                  {syncing ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <CheckCircle2 className="h-4 w-4" />
                  )}
                  <span>{terminalLaunched ? t('addAccount.syncBtnLaunched') : t('addAccount.syncBtnDefault')}</span>
                </button>
              </div>
            </div>
          ) : (
            <div className="space-y-4">
              {/* File Drop / Select Area */}
              <div
                onClick={() => fileInputRef.current?.click()}
                className="group relative flex flex-col items-center justify-center rounded-xl border-2 border-dashed border-slate-200 bg-slate-50/60 p-6 text-center cursor-pointer transition-all hover:border-blue-400 hover:bg-blue-50/40"
              >
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".json"
                  className="hidden"
                  onChange={handleFileChange}
                />
                <div className="flex h-10 w-10 items-center justify-center rounded-full bg-white shadow-xs group-hover:scale-105 transition-transform mb-2">
                  <UploadCloud className="h-5 w-5 text-blue-600" />
                </div>
                <p className="text-xs font-semibold text-slate-700">
                  {selectedFileName ? selectedFileName : t('addAccount.fileSelectPrompt')}
                </p>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  {selectedFileName
                    ? t('addAccount.fileSelectedHint')
                    : t('addAccount.fileFormatsHint')}
                </p>
              </div>

              {/* Or Manual Path Input */}
              <div className="space-y-1.5">
                <label className="text-[11px] font-semibold text-slate-600">
                  {t('addAccount.manualPathLabel')}
                </label>
                <input
                  type="text"
                  value={manualPath}
                  onChange={(e) => {
                    setManualPath(e.target.value);
                    if (e.target.value) {
                      setSelectedFileContent(null);
                      setSelectedFileName(null);
                    }
                  }}
                  placeholder="C:\path\to\auth.json"
                  className="w-full rounded-xl border border-slate-200 bg-slate-50/60 px-3 py-2 text-xs text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:bg-white focus:outline-none focus:ring-2 focus:ring-blue-500/20 shadow-xs transition-all font-mono"
                />
              </div>

              {/* Import Button */}
              <div className="pt-2">
                <button
                  type="button"
                  onClick={handleImport}
                  disabled={importing || (!selectedFileContent && !manualPath.trim())}
                  className="w-full flex items-center justify-center gap-2 rounded-xl bg-blue-600 px-4 py-2.5 text-xs font-semibold text-white hover:bg-blue-700 shadow-xs shadow-blue-600/20 transition-all disabled:opacity-50"
                >
                  {importing ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <ArrowRight className="h-4 w-4" />
                  )}
                  <span>{t('addAccount.importBtn')}</span>
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
