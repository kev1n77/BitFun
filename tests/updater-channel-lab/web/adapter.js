window.labAPI = {
  check: channel => window.__TAURI__.core.invoke('check_for_updates', { request: { channel } }),
  download: channel => window.__TAURI__.core.invoke('download_update', { request: { channel } }),
  install: version => window.__TAURI__.core.invoke('install_update', { request: { version } }),
  openDownloadPage: () => window.__TAURI__.core.invoke('open_download_page', { request: {} }),
};
