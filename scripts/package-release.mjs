import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, '..');

// Read version from tauri.conf.json
const tauriConfPath = path.join(rootDir, 'src-tauri', 'tauri.conf.json');
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, 'utf8'));
const version = tauriConf.version || '1.0.0';

const releaseDir = path.join(rootDir, 'release');
if (!fs.existsSync(releaseDir)) {
  fs.mkdirSync(releaseDir, { recursive: true });
}

const targetReleaseDir = path.join(rootDir, 'src-tauri', 'target', 'release');
const rawExePath = path.join(targetReleaseDir, 'CodexQ.exe');

if (!fs.existsSync(rawExePath)) {
  console.error(`❌ Error: ${rawExePath} not found! Please run 'pnpm run build:portable' or 'pnpm run tauri build' first.`);
  process.exit(1);
}

const deliverables = [];

// 1. Standalone Portable EXE
const portableExeName = `CodexQ-v${version}-portable.exe`;
const portableExeDest = path.join(releaseDir, portableExeName);
fs.copyFileSync(rawExePath, portableExeDest);
const portableExeStats = fs.statSync(portableExeDest);
deliverables.push({
  name: portableExeName,
  type: 'Portable Binary (免安装单文件)',
  size: `${(portableExeStats.size / (1024 * 1024)).toFixed(2)} MB`,
  path: portableExeDest,
});

// 2. Portable ZIP
const portableZipName = `CodexQ-v${version}-windows-x64-portable.zip`;
const portableZipDest = path.join(releaseDir, portableZipName);
const portableStagingDir = path.join(releaseDir, '.portable_staging');
try {
  if (fs.existsSync(portableStagingDir)) {
    fs.rmSync(portableStagingDir, { recursive: true, force: true });
  }
  fs.mkdirSync(portableStagingDir, { recursive: true });
  fs.copyFileSync(rawExePath, path.join(portableStagingDir, 'CodexQ.exe'));
  fs.writeFileSync(
    path.join(portableStagingDir, 'portable'),
    'This file indicates CodexQ is running in portable mode.\nData will be stored in ./data directory.\n'
  );
  // Use Windows built-in tar to create clean zip with CodexQ.exe and portable marker
  execSync(`tar -a -cf "${portableZipDest}" -C "${portableStagingDir}" CodexQ.exe portable`, {
    stdio: 'ignore',
  });
  fs.rmSync(portableStagingDir, { recursive: true, force: true });
  if (fs.existsSync(portableZipDest)) {
    const zipStats = fs.statSync(portableZipDest);
    deliverables.push({
      name: portableZipName,
      type: 'Portable Archive (便携压缩包)',
      size: `${(zipStats.size / (1024 * 1024)).toFixed(2)} MB`,
      path: portableZipDest,
    });
  }
} catch (e) {
  console.warn('⚠️ Could not create zip archive via tar:', e.message);
  if (fs.existsSync(portableStagingDir)) {
    fs.rmSync(portableStagingDir, { recursive: true, force: true });
  }
}

// 3. NSIS Setup Installer (if present)
const nsisDir = path.join(targetReleaseDir, 'bundle', 'nsis');
if (fs.existsSync(nsisDir)) {
  const nsisFiles = fs.readdirSync(nsisDir).filter((f) => f.includes(version) && f.endsWith('-setup.exe'));
  for (const nsisFile of nsisFiles) {
    const nsisSrc = path.join(nsisDir, nsisFile);
    const nsisDestName = `CodexQ-v${version}-windows-x64-setup.exe`;
    const nsisDest = path.join(releaseDir, nsisDestName);
    fs.copyFileSync(nsisSrc, nsisDest);
    const stats = fs.statSync(nsisDest);
    deliverables.push({
      name: nsisDestName,
      type: 'NSIS Installer (标准安装包)',
      size: `${(stats.size / (1024 * 1024)).toFixed(2)} MB`,
      path: nsisDest,
    });
  }
}

// 4. MSI Installer (if present)
const msiDir = path.join(targetReleaseDir, 'bundle', 'msi');
if (fs.existsSync(msiDir)) {
  const msiFiles = fs.readdirSync(msiDir).filter((f) => f.includes(version) && f.endsWith('.msi'));
  for (const msiFile of msiFiles) {
    const msiSrc = path.join(msiDir, msiFile);
    const msiDestName = `CodexQ-v${version}-windows-x64.msi`;
    const msiDest = path.join(releaseDir, msiDestName);
    fs.copyFileSync(msiSrc, msiDest);
    const stats = fs.statSync(msiDest);
    deliverables.push({
      name: msiDestName,
      type: 'MSI Package (企业静默安装包)',
      size: `${(stats.size / (1024 * 1024)).toFixed(2)} MB`,
      path: msiDest,
    });
  }
}

console.log('\n============================================================');
console.log(`🎉 CodexQ v${version} Release Packages Ready in: release/`);
console.log('============================================================');
for (const item of deliverables) {
  console.log(`- ${item.name.padEnd(42)} [${item.size.padStart(8)}] (${item.type})`);
}
console.log('============================================================\n');
