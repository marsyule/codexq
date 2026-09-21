import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle, RotateCcw } from 'lucide-react';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

/**
 * Universal React Error Boundary to prevent White Screen of Death.
 *
 * Catches JavaScript errors anywhere in the child component tree,
 * logs them, and displays a graceful fallback UI with recovery options.
 */
export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Uncaught error caught by ErrorBoundary:', error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  public render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-screen flex-col items-center justify-center bg-[#f5f7fa] p-6 text-slate-800">
          <div className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-6 shadow-xl text-center space-y-4">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-amber-50 text-amber-600 border border-amber-200">
              <AlertTriangle className="h-6 w-6" />
            </div>
            <div>
              <h2 className="text-base font-bold text-slate-900">应用渲染异常 (UI Render Error)</h2>
              <p className="mt-1 text-xs text-slate-500">
                前端组件捕获到运行时异常，已阻止应用白屏崩溃。
              </p>
            </div>
            {this.state.error && (
              <div className="max-h-32 overflow-y-auto rounded-xl bg-slate-50 p-3 text-left font-mono text-[11px] text-rose-600 border border-slate-200/80">
                {this.state.error.message || String(this.state.error)}
              </div>
            )}
            <div className="pt-2">
              <button
                type="button"
                onClick={this.handleReload}
                className="inline-flex items-center gap-2 rounded-xl bg-blue-600 px-4 py-2 text-xs font-semibold text-white shadow-sm hover:bg-blue-700 transition-colors"
              >
                <RotateCcw className="h-3.5 w-3.5" />
                <span>重新加载应用</span>
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
