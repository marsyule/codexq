import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { Plus, Search, Server, FolderOpen } from 'lucide-react';
import type { ProviderData, ToastPayload, ActiveRuntimeMode } from '../types';
import { ProviderCard } from './ProviderCard';
import { AddProviderModal } from './AddProviderModal';

interface ProvidersViewProps {
  providers: ProviderData[];
  activeMode: ActiveRuntimeMode | null;
  onRefresh: () => Promise<void>;
  onSwitchProvider: (providerId: string, modelOverride?: string) => Promise<void>;
  onDeleteProvider: (id: string) => Promise<void>;
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
}

/**
 * Main view for managing third-party AI model providers in light Clash Verge style.
 *
 * Provides search filtering, provider cards listing, and triggers
 * for creating, updating, and deleting provider configurations.
 */
export const ProvidersView: React.FC<ProvidersViewProps> = ({
  providers,
  activeMode,
  onRefresh,
  onSwitchProvider,
  onDeleteProvider,
  showToast,
}) => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState('');
  const [modalOpen, setModalOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<ProviderData | null>(null);

  const activeProviderId =
    activeMode && activeMode.mode === 'provider' ? activeMode.provider_id : null;

  const filteredProviders = providers.filter((p) => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return true;
    return (
      p.name.toLowerCase().includes(q) ||
      p.base_url.toLowerCase().includes(q) ||
      p.active_model.toLowerCase().includes(q) ||
      p.models.some((m) => m.toLowerCase().includes(q)) ||
      (p.notes && p.notes.toLowerCase().includes(q))
    );
  });

  const handleOpenAdd = () => {
    setEditingProvider(null);
    setModalOpen(true);
  };

  const handleOpenEdit = (p: ProviderData) => {
    setEditingProvider(p);
    setModalOpen(true);
  };

  const handleDelete = async (p: ProviderData) => {
    if (
      window.confirm(
        t('providers.deleteConfirm', {
          defaultValue: `Are you sure you want to delete provider '${p.name}'?`,
          name: p.name,
        })
      )
    ) {
      try {
        await onDeleteProvider(p.id);
        showToast(t('providers.deleted', 'Provider deleted successfully'), 'success');
      } catch (err: any) {
        showToast(String(err), 'error');
      }
    }
  };

  const handleSaved = async (savedProvider: ProviderData, shouldSwitch: boolean) => {
    await onRefresh();
    if (shouldSwitch) {
      await onSwitchProvider(savedProvider.id, savedProvider.active_model);
    }
  };

  const handleOpenCatalogDir = async () => {
    try {
      await invoke('open_model_catalog', { providerId: null });
    } catch (err: any) {
      showToast(String(err), 'error');
    }
  };

  return (
    <div className="space-y-4">
      {/* Top action bar: Search & Add Provider */}
      <div className="flex flex-col sm:flex-row items-center justify-between gap-3">
        <div className="relative w-full sm:w-80">
          <Search className="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t('providers.searchPlaceholder', 'Search providers or models...')}
            className="w-full pl-9 pr-3.5 py-1.5 bg-white border border-slate-200 rounded-xl text-xs text-slate-800 placeholder-slate-400 focus:outline-hidden focus:border-blue-500 shadow-2xs transition-all"
          />
        </div>

        <div className="flex items-center gap-2 w-full sm:w-auto">
          <button
            type="button"
            onClick={handleOpenCatalogDir}
            title={t('providers.openCatalogFolder', '打开模型目录')}
            className="w-full sm:w-auto inline-flex items-center justify-center gap-1.5 px-3 py-1.5 text-xs font-medium text-slate-600 hover:text-slate-900 bg-white hover:bg-slate-50 border border-slate-200 rounded-xl shadow-2xs transition-all active:scale-[0.99] shrink-0 cursor-pointer"
          >
            <FolderOpen className="w-3.5 h-3.5 text-slate-500" />
            <span>{t('providers.openCatalogFolder', '打开模型目录')}</span>
          </button>

          <button
            type="button"
            onClick={handleOpenAdd}
            className="w-full sm:w-auto inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 rounded-xl shadow-xs shadow-blue-600/20 transition-all active:scale-[0.99] shrink-0 cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>{t('providers.addBtn', 'Add Provider')}</span>
          </button>
        </div>
      </div>

      {/* Grid of Providers */}
      {filteredProviders.length === 0 ? (
        <div className="py-16 text-center border border-dashed border-slate-200 rounded-2xl bg-white/70 p-8 shadow-xs">
          <div className="w-12 h-12 rounded-2xl bg-blue-50 border border-blue-100 flex items-center justify-center mx-auto text-blue-600 mb-3">
            <Server className="w-6 h-6" />
          </div>
          <h3 className="text-sm font-semibold text-slate-800">
            {searchQuery
              ? t('providers.noSearchResults', 'No providers match your search')
              : t('providers.noProvidersYet', 'No third-party providers configured')}
          </h3>
          <p className="text-xs text-slate-500 mt-1 max-w-sm mx-auto">
            {searchQuery
              ? t('providers.tryAnotherSearch', 'Try searching by a different name, URL, or model.')
              : t(
                  'providers.emptyHint',
                  'Connect any OpenAI Responses compatible endpoint (e.g. DeepSeek, SiliconFlow, StepFun, OpenRouter) to switch models instantly.'
                )}
          </p>
          {!searchQuery && (
            <button
              type="button"
              onClick={handleOpenAdd}
              className="mt-4 inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold text-blue-700 bg-blue-50 hover:bg-blue-100 border border-blue-200 rounded-xl transition-all shadow-2xs"
            >
              <Plus className="w-4 h-4" />
              <span>{t('providers.addFirstProvider', 'Add your first Provider')}</span>
            </button>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filteredProviders.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              isActive={activeProviderId === provider.id}
              onSwitch={onSwitchProvider}
              onEdit={handleOpenEdit}
              onDelete={handleDelete}
              showToast={showToast}
            />
          ))}
        </div>
      )}

      {/* Modal Dialog */}
      <AddProviderModal
        isOpen={modalOpen}
        onClose={() => setModalOpen(false)}
        onSaved={handleSaved}
        showToast={showToast}
        editingProvider={editingProvider}
      />
    </div>
  );
};
