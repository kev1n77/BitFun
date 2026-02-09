#!/usr/bin/env node

/**
 * Development environment startup script
 * Manages pre-build tasks and dev server startup
 */

const { execSync, spawn } = require('child_process');
const path = require('path');
const {
  printHeader,
  printSuccess,
  printInfo,
  printError,
  printStep,
  printComplete,
  printBlank,
} = require('./console-style.cjs');

const ROOT_DIR = path.resolve(__dirname, '..');

/**
 * Run command synchronously (silent mode)
 */
function runSilent(command, cwd = ROOT_DIR) {
  try {
    execSync(command, { 
      cwd, 
      stdio: 'pipe',
      encoding: 'utf-8'
    });
    return true;
  } catch (error) {
    return false;
  }
}

/**
 * Run command with inherited output
 */
function runInherit(command, cwd = ROOT_DIR) {
  try {
    execSync(command, { cwd, stdio: 'inherit' });
    return true;
  } catch (error) {
    return false;
  }
}

/**
 * Run command and show output
 */
function runCommand(command, cwd = ROOT_DIR) {
  return new Promise((resolve, reject) => {
    const isWindows = process.platform === 'win32';
    const shell = isWindows ? 'cmd.exe' : '/bin/sh';
    const shellArgs = isWindows ? ['/c', command] : ['-c', command];
    
    const child = spawn(shell, shellArgs, {
      cwd,
      stdio: 'inherit'
    });
    
    child.on('close', (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`Command failed with code ${code}`));
      }
    });
    
    child.on('error', reject);
  });
}

/**
 * Main entry
 */
async function main() {
  const startTime = Date.now();
  const mode = process.argv[2] || 'web'; // web | desktop
  const modeLabel = mode === 'desktop' ? 'Desktop' : 'Web';
  
  printHeader(`BitFun ${modeLabel} Development`);
  printBlank();
  
  // Step 1: Copy resources
  printStep(1, 3, 'Copy resources');
  if (runSilent('npm run copy-monaco --silent')) {
    printSuccess('Monaco Editor resources ready');
  } else {
    printError('Copy resources failed');
    process.exit(1);
  }
  
  // Step 2: Generate version info
  printStep(2, 3, 'Generate version info');
  if (!runInherit('node scripts/generate-version.cjs')) {
    printError('Generate version info failed');
    process.exit(1);
  }
  
  const prepTime = ((Date.now() - startTime) / 1000).toFixed(1);
  
  // Step 3: Start dev server
  printStep(3, 3, 'Start dev server');
  printInfo(`Prep took ${prepTime}s`);
  
  printComplete('Initialization complete');
  
  try {
    if (mode === 'desktop') {
      await runCommand('pnpm tauri dev', path.join(ROOT_DIR, 'src/apps/desktop'));
    } else {
      await runCommand('npx vite', path.join(ROOT_DIR, 'src/web-ui'));
    }
  } catch (error) {
    printError('Dev server failed to start');
    process.exit(1);
  }
}

main().catch((error) => {
  printError('Startup failed: ' + error.message);
  process.exit(1);
});
