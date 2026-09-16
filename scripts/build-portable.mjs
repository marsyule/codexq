import { execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, '..');

console.log('\n🚀 Building CodexQ Portable (Standalone Single-File Binary)...');

// Check if running on Windows and CodexQ is running to give a clear prompt
try {
  const isRunning = execSync('tasklist /FI "IMAGENAME eq CodexQ.exe" /NH', { encoding: 'utf8' });
  if (isRunning.includes('CodexQ.exe')) {
    console.warn('\n⚠️ [Notice] An instance of CodexQ.exe is currently running.');
    console.warn('⚠️ Please close CodexQ before building to avoid Windows file lock error (os error 5).\n');
  }
} catch {
  // Ignore check error
}

try {
  execSync('tauri build --no-bundle', { stdio: 'inherit', cwd: rootDir });
  execSync('node scripts/package-release.mjs', { stdio: 'inherit', cwd: rootDir });
} catch (e) {
  process.exit(e.status || 1);
}
