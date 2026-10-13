// Validate real OCR and capture the packaged application using synthetic PDFs.
// Usage: node tools/capture-release.cjs PORTABLE_DIRECTORY OUTPUT_DIRECTORY
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const net = require('node:net');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { chromium } = require(process.env.MEDCHRON_PLAYWRIGHT_MODULE || 'playwright');

async function main() {
  const [portableArg, outputArg] = process.argv.slice(2);
  if (!portableArg || !outputArg) {
    throw new Error('Usage: node tools/capture-release.cjs PORTABLE_DIRECTORY OUTPUT_DIRECTORY');
  }
  const portable = path.resolve(portableArg);
  const output = path.resolve(outputArg);
  const fixtureRoot = path.resolve(__dirname, '../tests/fixtures/ocr');
  const filenames = (await fs.readdir(fixtureRoot)).filter(name => name.endsWith('.pdf')).sort();
  assert.equal(filenames.length, 4);
  const files = filenames.map(name => path.join(fixtureRoot, name));
  const expectedNames = filenames.map(name => name.startsWith('Jordan_') ? 'Jordan Test' : 'Taylor Example');
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'medchron-release-'));
  const port = await new Promise((resolve, reject) => {
    const reservation = net.createServer();
    reservation.on('error', reject);
    reservation.listen(0, '127.0.0.1', () => {
      const assigned = reservation.address().port;
      reservation.close(error => error ? reject(error) : resolve(assigned));
    });
  });
  await fs.mkdir(output, { recursive: true });
  await fs.mkdir(path.join(scratch, 'ocr'));
  const env = {
    ...process.env,
    MEDCHRON_DATA: path.join(scratch, 'data'),
    MEDCHRON_BIND: `127.0.0.1:${port}`,
    MEDCHRON_OCR_LANGUAGE: 'eng',
    TEMP: path.join(scratch, 'ocr'),
    TMP: path.join(scratch, 'ocr'),
    TMPDIR: path.join(scratch, 'ocr'),
  };
  // Test runtime discovery in the portable layout, independent of the runner PATH.
  for (const key of ['MEDCHRON_STORAGE_MODE', 'MEDCHRON_OCR_EXECUTABLE', 'MEDCHRON_TESSERACT_EXECUTABLE', 'MEDCHRON_POPPLER_EXECUTABLE']) {
    delete env[key];
  }
  const binary = path.join(portable, 'bin', process.platform === 'win32' ? 'medchron-server.exe' : 'medchron-server');
  const server = spawn(binary, [], { cwd: portable, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  let startupError;
  server.on('error', error => { startupError = error; });
  for (const stream of [server.stdout, server.stderr]) {
    stream.on('data', chunk => { log = (log + chunk).slice(-8000); });
  }
  const base = `http://127.0.0.1:${port}`;
  let browser;

  async function api(url, options) {
    const response = await fetch(base + url, { ...options, signal: AbortSignal.timeout(60000) });
    if (!response.ok) throw new Error(`${url}: ${response.status} ${await response.text()}`);
    return response;
  }

  async function upload(indices, fields) {
    const form = new FormData();
    for (const index of indices) {
      form.append('files', new Blob([await fs.readFile(files[index])], { type: 'application/pdf' }), filenames[index]);
    }
    for (const [name, value] of Object.entries(fields)) form.append(name, value);
    return form;
  }

  try {
    let ready = false;
    for (let attempt = 0; attempt < 80; attempt++) {
      if (startupError) throw startupError;
      if (server.exitCode !== null) throw new Error(`Server exited: ${log}`);
      try {
        const response = await fetch(base + '/healthz', { signal: AbortSignal.timeout(2000) });
        ready = response.ok && await response.text() === 'medchron-local-ok';
        if (ready) break;
      } catch { /* The packaged server may still be starting. */ }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    assert.ok(ready, `Packaged server did not start: ${log}`);
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1440, height: 1400 }, deviceScaleFactor: 1 });
    page.setDefaultTimeout(60000);
    await page.goto(base, { waitUntil: 'networkidle' });
    await page.locator('#batchFiles').setInputFiles(files);
    const analysisResponse = page.waitForResponse(response => response.url() === base + '/api/intake/analyze' && response.request().method() === 'POST', { timeout: 120000 });
    await page.locator('#batchAnalyze').click();
    const response = await analysisResponse;
    assert.equal(response.status(), 200);
    const previews = await response.json();
    assert.equal(previews.length, 4);
    for (const [index, preview] of previews.entries()) {
      assert.equal(preview.file_index, index);
      assert.equal(preview.suggested_name, expectedNames[index]);
      assert.ok(preview.ocr_pages > 0, `No OCR text recognized in ${preview.filename}`);
      assert.equal(preview.warning, null);
    }
    assert.deepEqual(await (await api('/api/cases')).json(), []);
    await page.waitForFunction(() => document.querySelectorAll('.batchrow').length === 4);
    await page.locator('#batchImport').scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(output, 'Medchron-Batch-Intake.png'), animations: 'disabled' });

    await page.locator('#batchConfirmed').check();
    const importResponse = page.waitForResponse(response => response.url() === base + '/api/intake/import' && response.request().method() === 'POST', { timeout: 120000 });
    await page.locator('#batchImport').click();
    const imported = await importResponse;
    assert.equal(imported.status(), 200);
    const summaries = await imported.json();
    assert.equal(summaries.length, 2);
    await page.waitForFunction(() => document.querySelector('#personHeading').textContent === 'Jordan Test' && document.querySelectorAll('#peopleList .person').length === 2 && document.querySelectorAll('#docsList .fileitem').length === 2);
    const cases = [];
    for (const summary of summaries) {
      const record = await (await api(`/api/cases/${summary.id}`)).json();
      assert.equal(record.files.length, 2);
      assert.ok(record.entries.length > 0);
      assert.ok(record.entries.every(entry => !entry.verified && entry.extractor === 'rules+ocr' && entry.page >= 1 && record.files.includes(entry.source_file)));
      assert.ok(record.warnings.every(warning => !warning.includes('unavailable') && !warning.includes('no readable text')));
      for (const [index, filename] of filenames.entries()) {
        if (expectedNames[index] !== record.patient.name) continue;
        const pdf = await api(`/api/cases/${record.id}/files/${encodeURIComponent(filename)}`);
        assert.deepEqual(Buffer.from(await pdf.arrayBuffer()), await fs.readFile(files[index]));
      }
      cases.push(record);
    }
    await page.screenshot({ path: path.join(output, 'Medchron-People-Workspace.png'), animations: 'disabled' });

    const jordan = cases.find(record => record.patient.name === 'Jordan Test');
    const taylor = cases.find(record => record.patient.name === 'Taylor Example');
    const merged = await (await api('/api/intake/import', {
      method: 'POST',
      body: await upload([0], { decisions: JSON.stringify({ names: ['Jordan Test'], case_ids: [jordan.id], confirmed: true }) }),
    })).json();
    assert.equal(merged[0].id, jordan.id);
    assert.equal(merged[0].file_count, 3);
    assert.equal((await (await api('/api/cases')).json()).length, 2);
    assert.deepEqual((await (await api(`/api/cases/${taylor.id}`)).json()).files, taylor.files);
    assert.deepEqual(await fs.readdir(path.join(scratch, 'ocr')), []);
    console.log('Packaged OCR, reviewed batch intake, PDF retrieval, explicit existing-person import, and screenshots passed.');
  } finally {
    try {
      if (browser) await browser.close();
    } finally {
      if (server.pid && server.exitCode === null && server.signalCode === null) {
        await new Promise(resolve => { server.once('exit', resolve); server.kill(); });
      }
      await fs.rm(scratch, { recursive: true, force: true });
    }
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
