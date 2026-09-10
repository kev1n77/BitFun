const el = id => document.getElementById(id);
let pendingVersion = null;

function show(report) {
  el('version').textContent = report.currentVersion;
  el('latest').textContent = report.latestVersion ?? 'No update on this channel';
  el('key').textContent = report.publicKeySha256;
  el('endpoint').textContent = report.endpoint;
  el('notice').hidden = !report.updateAvailable;
  el('notes').textContent = report.notes ?? '';
  el('install').disabled = !report.downloadVerified;
  pendingVersion = report.downloadVerified ? report.latestVersion : null;
}

async function run(action, busy, done) {
  const buttons = [...document.querySelectorAll('button'), el('channel')];
  buttons.forEach(button => { button.disabled = true; });
  el('status').textContent = busy;
  try {
    const report = await action();
    if (report) show(report);
    el('status').textContent = done;
  } catch (error) {
    el('status').textContent = String(error);
  } finally {
    buttons.forEach(button => { button.disabled = false; });
    el('install').disabled = !pendingVersion;
  }
}

const check = () => run(() => window.labAPI.check(el('channel').value), 'Checking the public test channel…', 'Check complete.');
el('check').onclick = check;
el('channel').onchange = check;
el('download').onclick = () => run(() => window.labAPI.download(el('channel').value), 'Downloading and verifying the signed test package…', 'Signature verified. Installation still requires your confirmation.');
el('install').onclick = () => {
  if (pendingVersion && window.confirm('Install the isolated 0.2.20 test application and restart it?')) {
    void run(() => window.labAPI.install(pendingVersion), 'Installing the test update…', 'Installer started.');
  }
};
el('download-page').onclick = () => run(() => window.labAPI.openDownloadPage(), 'Opening download page…', 'Download page opened in your browser.');
void check();
