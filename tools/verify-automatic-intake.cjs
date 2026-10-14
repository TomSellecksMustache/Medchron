#!/usr/bin/env node
'use strict';
// Windows release gate: process 30 *fictional* sample PDFs through the real
// browser/server/locally bundled OCR, then confirm exactly five identities.
// No client information, remote services, or third-party OCR APIs are used.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn, spawnSync } = require('node:child_process');

const portable = path.resolve(process.argv[2] || '');
const out = path.resolve(process.argv[3] || 'dist');
const modulePath = process.env.MEDCHRON_PLAYWRIGHT_MODULE;
if (!modulePath || !fs.existsSync(modulePath)) {
  console.error('MEDCHRON_PLAYWRIGHT_MODULE must point to locally installed Playwright.');
  process.exit(2);
}
const { chromium } = require(modulePath);
const root = process.cwd();
const shaFile = path.join(root, 'packaging', 'windows', 'synthetic-sha256.txt');
const lines = fs.readFileSync(shaFile, 'utf8').split(/\r?\n/).filter(Boolean);
const pdfFiles = lines.map(line => {
  const m = /^([0-9a-f]{64})  ([A-Za-z0-9_-]+\.pdf)$/.exec(line);
  if (!m) throw new Error('Invalid sample manifest line');
  const filename = path.join(root, 'samples', m[2]);
  if (!fs.existsSync(filename)) throw new Error(`Missing synthetic sample: ${m[2]}`);
  const actual = crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
  if (actual !== m[1]) throw new Error(`Synthetic sample checksum mismatch: ${m[2]}`);
  return filename;
});
if (pdfFiles.length !== 30) throw new Error(`Expected 30 PDFs; found ${pdfFiles.length}`);
const people = ['Jordan Test', 'Taylor Example', 'Morgan Sample', 'Casey Demo', 'Avery Fixture'];
const base = 'http://127.0.0.1:56753';
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'medchron-release-smoke-'));
const executable = path.join(portable, 'bin', 'medchron-server.exe');
if (!fs.existsSync(executable)) throw new Error(`Missing Windows server: ${executable}`);
fs.mkdirSync(out, {recursive: true});
let server, browser;
let logs = '';
const delay = ms => new Promise(r => setTimeout(r, ms));
async function waitForServer() {
  for(let i=0;i<120;i++) {
    if (server.exitCode !== null) throw new Error(`Server exited early: ${server.exitCode}; logs: ${logs.slice(-3000)}`);
    try { const r=await fetch(base + '/healthz', {signal: AbortSignal.timeout(1500)}); if(r.ok) return; } catch (_) {}
    await delay(500);
  }
  throw new Error(`Timed out waiting for server; logs: ${logs.slice(-3000)}`);
}
async function fetchCases() {
  const response = await fetch(base + '/api/cases', {signal: AbortSignal.timeout(10000)});
  if(!response.ok) throw new Error(`Cases API returned ${response.status}`);
  return response.json();
}
async function run() {
  server = spawn(executable, [], {
    cwd: path.join(portable,'bin'),
    env: {
      ...process.env,
      MEDCHRON_BIND: '127.0.0.1:56753',
      MEDCHRON_DATA: scratch,
      MEDCHRON_TESSERACT_EXECUTABLE: path.join(portable,'ocr','tesseract','tesseract.exe'),
      MEDCHRON_POPPLER_EXECUTABLE: path.join(portable,'ocr','poppler','pdftoppm.exe')
    },
    windowsHide:true,
    stdio:['ignore','pipe','pipe']
  });
  for (const stream of [server.stdout,server.stderr]) stream.on('data', data => { logs=(logs+data.toString()).slice(-50000); });
  await waitForServer();
  browser = await chromium.launch({headless:true});
  const page = await browser.newPage({viewport:{width:1440,height:960}, deviceScaleFactor:1});
  const errors = [];
  page.on('pageerror',err => errors.push(String(err)));
  await page.goto(base, {waitUntil:'domcontentloaded',timeout:30000});
  await page.locator('#intakeFiles').waitFor();
  if(await page.locator('input[type=file]').count()!==1) throw new Error('Expected one file picker');
  await page.locator('.brand-wordmark').waitFor({timeout:15000});
  if (await page.locator('.brand-wordmark').evaluate(el => el.naturalWidth) < 200) throw new Error('Branded header did not render');
  if(await page.locator('#calendarGrid .calday').count() !== 30) throw new Error('Calendar did not render 30 days');
  await page.screenshot({path:path.join(out,'Medchron-Batch-Intake.png'),fullPage:true});
  console.log('Uploading 30 fictional PDFs; the OCR smoke test may take several minutes...');
  await page.locator('#intakeFiles').setInputFiles(pdfFiles);
  const deadline = Date.now() + 12*60*1000;
  let summaries=[];
  while (Date.now() < deadline) {
    summaries = await fetchCases();
    if (summaries.length === 5) break;
    const message = await page.locator('#uploadStatus').textContent().catch(()=> '');
    if (message && /Unable to import:/i.test(message)) throw new Error(`Browser import failed: ${message}`);
    if (server.exitCode!==null) throw new Error(`Server stopped during OCR: ${logs.slice(-3000)}`);
    await delay(1500);
  }
  if (summaries.length!==5) {
    const message=await page.locator('#uploadStatus').textContent().catch(()=>'');
    throw new Error(`Expected five imported people, got ${summaries.length}. UI: ${message}; logs: ${logs.slice(-3000)}`);
  }
  for (const name of people) {
    const matching = summaries.filter(c=>c.name===name);
    if (matching.length!==1) throw new Error(`Expected exactly one profile for ${name}; got ${matching.length}`);
    const c = matching[0];
    const response=await fetch(base+'/api/cases/'+encodeURIComponent(c.id));
    if(!response.ok) throw new Error(`Could not read profile for ${name}`);
    const record=await response.json();
    if(record.files?.length!==6) throw new Error(`${name} has ${record.files?.length} documents, expected 6`);
    console.log(`Verified ${name}: 6 PDFs`);
  }
  await page.waitForFunction(()=>document.querySelectorAll('#peopleList button.person').length===5,null,{timeout:20000});
  if (!(await page.locator('#compactDropzone').isVisible())) throw new Error('Compact upload bar is missing after intake');
  await page.locator('#peopleList button.person').filter({hasText:'Jordan Test'}).click();
  await page.waitForFunction(()=>document.querySelectorAll('#docsList button.fileitem').length===6,null,{timeout:20000});
  await page.screenshot({path:path.join(out,'Medchron-People-Workspace.png'),fullPage:true});
  await page.locator('#docsList button.fileitem').first().click();
  await page.locator('#pdfDialog[open]').waitFor({timeout:10000});
  await page.locator('#verifyFields .verify-field').first().waitFor({timeout:15000});
  if (await page.locator('#verifyFields .verify-field').count()!==4) throw new Error('Four verification fields must appear in the PDF viewer');
  const nameField = page.locator('#verifyFields .verify-field').first();
  if (await nameField.isEnabled()) {
    await nameField.click();
    if (await nameField.getAttribute('aria-pressed') !== 'true') throw new Error('Name verification did not toggle on');
    await nameField.click();
    if (await nameField.getAttribute('aria-pressed') !== 'false') throw new Error('Name verification did not toggle off');
  }
  await page.locator('#closePdf').click();
  if(errors.length) throw new Error(`Browser JavaScript error(s): ${errors.join('; ').slice(0,2000)}`);
  console.log('Windows OCR, automatic grouping, original branding, PDF preview and screenshots passed.');
}
run().catch(error=>{console.error(error.stack||error);process.exitCode=1;}).finally(async()=>{
  try { if (browser) await browser.close(); } catch(_) {}
  if(server && server.exitCode===null) {
    // taskkill ends the entire local-only smoke-test process tree on Windows.
    if(process.platform==='win32') spawnSync('taskkill',['/PID',String(server.pid),'/T','/F'],{stdio:'ignore'});
    else server.kill();
  }
  try { fs.rmSync(scratch,{recursive:true,force:true}); } catch(_) {}
});
