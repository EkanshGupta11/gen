/**
 * Automated end-to-end browser test using Chrome DevTools Protocol (CDP)
 * via native Node.js (v24 built-in fetch & WebSocket) - no npm packages needed.
 */
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const CHROME_PATH = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const PORT = 9222;
const APP_URL = "http://127.0.0.1:8787/";

async function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

class CDPClient {
    constructor(wsUrl) {
        this.ws = new WebSocket(wsUrl);
        this.id = 1;
        this.callbacks = new Map();
        this.ws.onmessage = (event) => {
            const data = JSON.parse(event.data);
            if (data.id && this.callbacks.has(data.id)) {
                const { resolve, reject } = this.callbacks.get(data.id);
                this.callbacks.delete(data.id);
                if (data.error) reject(new Error(data.error.message));
                else resolve(data.result);
            }
        };
    }

    async ready() {
        if (this.ws.readyState === WebSocket.OPEN) return;
        return new Promise((resolve, reject) => {
            this.ws.onopen = () => resolve();
            this.ws.onerror = (err) => reject(err);
        });
    }

    async send(method, params = {}) {
        await this.ready();
        return new Promise((resolve, reject) => {
            const msgId = this.id++;
            this.callbacks.set(msgId, { resolve, reject });
            this.ws.send(JSON.stringify({ id: msgId, method, params }));
        });
    }

    async eval(expression) {
        const res = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
        if (res.exceptionDetails) {
            throw new Error(`Eval error: ${JSON.stringify(res.exceptionDetails)}`);
        }
        return res.result?.value;
    }

    close() {
        this.ws.close();
    }
}

async function runBrowserTest() {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'chrome_cdp_'));
    console.log(`[BROWSER TEST] Using temporary profile: ${tmpDir}`);

    console.log(`[BROWSER TEST 1/6] Launching headless Chrome with remote debugging on port ${PORT}...`);
    const chrome = spawn(CHROME_PATH, [
        `--remote-debugging-port=${PORT}`,
        `--user-data-dir=${tmpDir}`,
        '--headless=new',
        '--disable-gpu',
        '--no-first-run',
        '--no-default-browser-check',
        'about:blank'
    ]);

    let cdp = null;
    try {
        // Wait for CDP endpoint to be active
        let targets = null;
        for (let i = 0; i < 30; i++) {
            await sleep(300);
            try {
                const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
                if (res.ok) {
                    targets = await res.json();
                    if (targets.length > 0 && targets[0].webSocketDebuggerUrl) break;
                }
            } catch (e) {}
        }

        if (!targets || !targets[0]?.webSocketDebuggerUrl) {
            throw new Error('Failed to connect to Chrome DevTools endpoint');
        }
        console.log(`[BROWSER TEST 2/6] Connected to Chrome CDP: ${targets[0].title}`);

        cdp = new CDPClient(targets[0].webSocketDebuggerUrl);
        await cdp.send('Page.enable');
        await cdp.send('Runtime.enable');

        console.log(`[BROWSER TEST 3/6] Navigating to ${APP_URL}...`);
        await cdp.send('Page.navigate', { url: APP_URL });

        // Wait for page load and initial JS execution
        await sleep(1500);

        const pageTitle = await cdp.eval('document.title');
        console.log(`  Page Title: "${pageTitle}"`);
        if (pageTitle !== 'NeuroGenesis AI — Research workspace') {
            throw new Error(`Unexpected page title: ${pageTitle}`);
        }

        // Verify initial backend communication via settings and history
        let badgeText = '';
        for (let i = 0; i < 20; i++) {
            badgeText = await cdp.eval('document.getElementById("mode")?.textContent');
            if (badgeText === 'Offline mock mode' || badgeText === 'Hosted model ready') break;
            await sleep(200);
        }
        console.log(`  System Mode Badge: "${badgeText}"`);
        if (badgeText !== 'Offline mock mode' && badgeText !== 'Hosted model ready') {
            throw new Error(`Backend communication failed, badge text: ${badgeText}`);
        }
        console.log(`[BROWSER TEST 4/6] Backend communication verified on page load.`);

        // Enter a new question into the textarea
        const question = "For a projectile launched at speed 20 m/s and angle 35 degrees, estimate the ideal range and explain assumptions.";
        console.log(`[BROWSER TEST 5/6] Entering investigation question and triggering submission...`);
        await cdp.eval(`
            document.getElementById('question').value = ${JSON.stringify(question)};
            document.getElementById('run').click();
        `);

        // Wait for run button to show planning, then complete
        console.log(`  Waiting for investigation execution in browser...`);
        let completed = false;
        let resultHTML = '';
        for (let i = 0; i < 60; i++) {
            await sleep(500);
            const btnText = await cdp.eval('document.getElementById("run")?.textContent');
            const hasResult = await cdp.eval('Boolean(document.querySelector(".card.result"))');
            if (hasResult && btnText.includes('Start investigation')) {
                completed = true;
                resultHTML = await cdp.eval('document.getElementById("output")?.innerText');
                break;
            }
        }

        if (!completed) {
            const errorText = await cdp.eval('document.getElementById("error")?.innerText');
            throw new Error(`Investigation did not complete. Error: ${errorText}`);
        }

        console.log(`[BROWSER TEST 6/6] Investigation completed and rendered in browser DOM!`);
        console.log(`  Summary of rendered content:\n${resultHTML.split('\n').map(l => '    ' + l).slice(0, 15).join('\n')}\n    ...`);

        // Check that history sidebar updated
        const historyCount = await cdp.eval('document.querySelectorAll(".history").length');
        console.log(`  History items in sidebar: ${historyCount}`);
        if (historyCount < 1) {
            throw new Error('History list was not populated');
        }

        console.log(`\n>>> BROWSER END-TO-END VERIFICATION: PASSED <<<`);
        return true;
    } finally {
        if (cdp) cdp.close();
        chrome.kill();
        // Give time for chrome to release files
        await sleep(500);
        try {
            fs.rmSync(tmpDir, { recursive: true, force: true });
        } catch (e) {}
    }
}

runBrowserTest().then(() => process.exit(0)).catch(err => {
    console.error('Browser test failed:', err);
    process.exit(1);
});
