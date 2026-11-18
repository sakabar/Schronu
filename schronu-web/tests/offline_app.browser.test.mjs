// Run against a packaged server, its adjacent public/ directory, and the matching CLI.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import test from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

const chrome = process.env.SCHRONU_TEST_CHROME;
const serverBinary = process.env.SCHRONU_TEST_WEB_SERVER;
const cliBinary = process.env.SCHRONU_TEST_CLI;
const taskId = "c9a7aa9a-62d2-40f0-a97d-13c91e9f0321";
const taskName = "保存契約用タスク " + "LongOfflineTaskName".repeat(5);
const minute = 60_000;
const hour = 60 * minute;

async function until(description, attempt, timeout = 30_000) {
    const deadline = Date.now() + timeout;
    do {
        const value = await attempt();
        if (value) return value;
        await sleep(100);
    } while (Date.now() < deadline);
    throw Error("Timed out waiting for " + description);
}

async function isolatedServer(work) {
    const storage = join(work, "tasks");
    const project = join(storage, "fixture");
    const yaml = join(project, "project.yaml");
    const config = join(work, "config.yaml");
    await mkdir(project, { recursive: true });
    await writeFile(yaml, `project:
  name: ${taskName}
  id: ${taskId}
  estimated_work_seconds: 3600
  create_time: "2025/01/01 07:00:00"
  start_time: "2025/01/01 07:00:00"
`);
    await writeFile(config, "busy_time_slots_yaml_path: slots.yaml\nend_of_day_offset_minutes: 360\n");
    const days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    await writeFile(join(work, "slots.yaml"), "days_of_week:\n" +
        days.map(day => `  - day_of_week: ${day}\n    busy_time_slots: []\n`).join(""));

    const listener = createServer().listen(0, "127.0.0.1");
    await once(listener, "listening");
    const port = listener.address().port;
    await new Promise(resolve => listener.close(resolve));
    const url = "http://127.0.0.1:" + port;
    const env = {
        ...process.env, TZ: "Asia/Tokyo", SCHRONU_STORAGE_DIR: storage,
        SCHRONU_CONFIG_PATH: config, DIOXUS_PUBLIC_PATH: join(dirname(serverBinary), "public"),
        PORT: String(port),
    };
    let child;
    return {
        url,
        yaml: () => readFile(yaml, "utf8"),
        cli: (...args) => execFileSync(cliBinary, args, { cwd: work, env, encoding: "utf8" }),
        async start() {
            child = spawn(serverBinary, [], { cwd: work, env, stdio: ["ignore", "ignore", "pipe"] });
            let log = "";
            child.stderr.on("data", bytes => { log = (log + bytes).slice(-2000); });
            await once(child, "spawn");
            await until("isolated server", () => {
                if (child.exitCode !== null) throw Error("Server exited: " + log);
                return fetch(url).then(response => response.ok, () => false);
            });
        },
        async stop() {
            if (child?.pid && child.exitCode === null && !child.signalCode) {
                const exited = once(child, "exit");
                child.kill();
                await exited;
            }
        },
    };
}

// Each call starts a completely new browser process against the same test-owned profile.
async function withBrowser(profile, url, timezoneId, visit) {
    const portFile = join(profile, "DevToolsActivePort");
    await rm(portFile, { force: true });
    const browser = spawn(chrome, [
        "--headless", "--disable-gpu", "--disable-background-networking", "--disable-sync",
        "--no-first-run", "--no-default-browser-check", "--remote-debugging-address=127.0.0.1",
        "--remote-debugging-port=0", "--user-data-dir=" + profile, "about:blank",
    ], { stdio: "ignore" });
    let socket;
    try {
        await once(browser, "spawn");
        const debuggerUrl = await until("Chromium's test tab", async () => {
            if (browser.exitCode !== null) throw Error("Chromium exited: " + browser.exitCode);
            const port = await readFile(portFile, "utf8").then(text => text.split("\n")[0], () => "");
            if (!port) return;
            const pages = await fetch(`http://127.0.0.1:${port}/json/list`).then(r => r.json(), () => []);
            return pages.find(page => page.type === "page")?.webSocketDebuggerUrl;
        });
        socket = new WebSocket(debuggerUrl);
        await once(socket, "open");
        let nextId = 0;
        const replies = new Map();
        let onPaused;
        socket.addEventListener("message", ({ data }) => {
            const packet = JSON.parse(data);
            if (packet.method === "Fetch.requestPaused") onPaused?.(packet.params);
            const reply = replies.get(packet.id);
            if (reply) {
                replies.delete(packet.id);
                if (packet.error) reply.reject(Error(packet.error.message));
                else reply.resolve(packet.result);
            }
        });
        socket.addEventListener("close", () => {
            for (const reply of replies.values()) reply.reject(Error("Chromium disconnected"));
            replies.clear();
        });
        function command(method, params = {}) {
            const id = ++nextId;
            const reply = Promise.withResolvers();
            replies.set(id, reply);
            socket.send(JSON.stringify({ id, method, params }));
            return reply.promise;
        }
        async function evaluate(expression) {
            const { result, exceptionDetails } = await command("Runtime.evaluate", {
                expression, returnByValue: true,
            });
            if (exceptionDetails) throw Error(exceptionDetails.exception?.description || exceptionDetails.text);
            return result.value;
        }
        const page = {
            evaluate,
            setViewport: width => command("Emulation.setDeviceMetricsOverride", {
                width, height: 760, deviceScaleFactor: 1, mobile: width < 768,
            }),
            show: label => until(label, async () => (await evaluate("document.body?.innerText || ''")).includes(label)),
            clock: epoch => evaluate(`Date.now = () => ${epoch}; true`),
            click(label) {
                return until("enabled " + label, () => evaluate(`(() => {
                    const label = ${JSON.stringify(label)};
                    const button = [...document.querySelectorAll("button,summary")].find(element =>
                        !element.disabled && element.getClientRects().length &&
                        (element.textContent.trim() === label || element.getAttribute("aria-label") === label));
                    if (button) button.click();
                    return Boolean(button);
                })()`));
            },
            async dropSyncReply() {
                const interrupted = Promise.withResolvers();
                onPaused = async ({ requestId, responseStatusCode }) => {
                    try {
                        assert.equal(responseStatusCode, 200);
                        await command("Fetch.failRequest", { requestId, errorReason: "ConnectionReset" });
                        interrupted.resolve();
                    } catch (error) { interrupted.reject(error); }
                };
                await command("Fetch.enable", { patterns: [{
                    urlPattern: "*/api/web_sync_offline_operation*", requestStage: "Response",
                }] });
                try {
                    await page.click("同期");
                    await Promise.race([interrupted.promise, sleep(15_000, null, { ref: false })
                        .then(() => { throw Error("No sync response to drop"); })]);
                } finally {
                    await command("Fetch.disable");
                }
            },
        };
        await command("Page.enable");
        await command("Emulation.setTimezoneOverride", { timezoneId });
        await page.setViewport(412);
        await command("Page.navigate", { url });
        await visit(page);
    } finally {
        if (browser.pid && browser.exitCode === null && !browser.signalCode) {
            const exited = once(browser, "exit");
            if (socket?.readyState === WebSocket.OPEN) {
                socket.send(JSON.stringify({ id: -1, method: "Browser.close" }));
            } else {
                browser.kill();
            }
            const killIfStuck = setTimeout(() => browser.kill("SIGKILL"), 5000);
            try { await exited; } finally { clearTimeout(killIfStuck); }
        }
        socket?.close();
    }
}

function jst(epoch) {
    return new Date(epoch + 9 * hour).toISOString().slice(0, 19).replaceAll("-", "/").replace("T", " ");
}

async function scheduleLayout(page, width) {
    await page.setViewport(width);
    const layout = await page.evaluate(`(() => {
        const table = document.querySelector(".task-list-view .task-table");
        const row = table.querySelector(".session-start").closest("tr");
        const columns = cells => Array.from(cells, cell => {
            const rect = cell.getBoundingClientRect();
            return [rect.left, rect.width];
        });
        const name = row.querySelector(".task-name-scroll");
        return {
            width: document.documentElement.scrollWidth,
            headings: columns(table.tHead.rows[0].cells),
            columns: columns(row.cells),
            rowHeight: row.offsetHeight,
            nameScrolls: name.scrollWidth > name.clientWidth,
        };
    })()`);
    assert.equal(layout.width, width);
    assert.equal(layout.columns.length, 4);
    assert.deepEqual(layout.columns, layout.headings);
    if (width === 320) assert(layout.nameScrolls);
    return layout;
}

async function savedSchedule(page) {
    await until("saved date buttons", () => page.evaluate("Boolean(document.querySelector('.offline-shell .date-pill:not(:disabled)'))"));
    await page.show("準備完了");
}

async function selectSavedTaskDate(page) {
    const dates = await page.evaluate("Array.from(document.querySelectorAll('.offline-shell .date-pill:not(:disabled)'), b => b.textContent.trim())");
    assert.equal(dates.length, 7);
    for (const date of dates) {
        await page.click(date);
        if (await page.evaluate("Boolean(document.querySelector('.session-start'))")) return date;
    }
    assert.fail("Fixture task must appear in the saved week");
}

test("offline schedule, cold timer restoration and replay of a committed split", {
    skip: !(chrome && serverBinary && cliBinary), timeout: 120_000,
}, async () => {
    assert([chrome, serverBinary, cliBinary].every(isAbsolute), "Pass absolute executable paths");
    const work = await mkdtemp(join(tmpdir(), "schronu-offline-product-"));
    const profile = join(work, "profile");
    let server;
    try {
        server = await isolatedServer(work);
        await server.start();
        // Anchor all simulated button-clicks in the past relative to the real server.
        const start = Math.floor((Date.now() - 6 * hour) / 1000) * 1000;
        const split = start + 25 * minute;
        const resumed = split + 5 * minute;
        const finished = resumed + 5 * hour;
        let selectedDate;
        const onlineLayouts = new Map();
        await withBrowser(profile, server.url, "Asia/Tokyo", async page => {
            await page.click("一覧");
            await until("online dates ready", () => page.evaluate("Boolean(document.querySelector('.date-pill:nth-child(2):not(:disabled)'))"));
            const days = await page.evaluate("Array.from(document.querySelectorAll('.date-pill'), b => b.textContent.trim()).slice(1, 8)");
            for (const day of days) {
                await page.click(day);
                await until("selected online day", () => page.evaluate(`!document.querySelector('main[inert]') && document.querySelector('.date-pill[aria-pressed=true]')?.textContent.trim() === ${JSON.stringify(day)}`));
                if (await page.evaluate("Boolean(document.querySelector('.task-list-view .session-start'))")) break;
            }
            for (const width of [320, 412, 1024]) onlineLayouts.set(width, await scheduleLayout(page, width));
            await page.setViewport(412);
            await page.click("設定");
            await page.click("オフラインで使う");
            await savedSchedule(page);
            assert.equal(await page.evaluate("document.querySelector('.offline-status') !== null"), false, "Schedule has no status header");
            await page.click("負荷");
            await page.show("から14日の負荷");
            assert.equal(await page.evaluate("document.querySelector('.offline-status') !== null"), false, "Load has no status header");
            assert.equal(await page.evaluate("document.querySelectorAll('.load-day').length"), 14);
            assert.equal(await page.evaluate("document.querySelectorAll('.load-day:disabled').length"), 7);
            await page.click("繰返負荷");
            await page.show("から28日の負荷");
        });
        await server.stop();
        await withBrowser(profile, server.url, "Asia/Tokyo", async page => {
            await savedSchedule(page);
            selectedDate = await selectSavedTaskDate(page);
            for (const [width, layout] of onlineLayouts) assert.deepEqual(await scheduleLayout(page, width), layout);
            assert(await page.evaluate("['.task-defer', '.date-pill', '.list-mode-button'].every(s => document.querySelector(s + ':disabled'))"));
            await page.evaluate("document.querySelector('.deviation-sort input').click()");
            await until("offline deviation sort", () => page.evaluate("Boolean(document.querySelector('.deviation-heading[aria-sort=descending]'))"));
            await page.evaluate(`const input = document.querySelector('.task-name-filter-input');
                input.value = 'unmatched-filter'; input.dispatchEvent(new Event('input', { bubbles: true }));`);
            await page.show("一致するタスクがありません");
            await page.click("検索文字列をクリア");
            await page.setViewport(320);
            await page.clock(start);
            await page.click(taskName + ": セッションに追加");
            await page.show("記録して分割");
            await page.clock(split);
            await page.click("記録して分割");
            await savedSchedule(page);
            await page.click("セッション");
            await page.show("未送信1件");
        });
        await withBrowser(profile, server.url, "America/Los_Angeles", async page => {
            await page.click("セッション");
            await page.show("未送信1件");
            await page.click("一覧");
            await page.click(selectedDate);
            await page.show("ロサンゼルス");
            await page.clock(resumed);
            await page.click(taskName + ": セッションに追加");
            await page.show("記録して完了");
            await page.show("00:35:00");
        });
        await withBrowser(profile, server.url, "Asia/Taipei", async page => {
            await page.show("記録して完了");
            await page.clock(finished);
            await page.show("経過 05:00:00");
            await page.click("記録して完了");
            await savedSchedule(page);
            await page.click("セッション");
            await page.show("未送信2件");
            assert.doesNotMatch(await server.yaml(), /status: done/);
            await server.start();
            await page.dropSyncReply();
            await page.show("サーバーに接続できません");
            await page.show("未送信2件");
            assert.match(await server.yaml(), /actual_work_seconds: 1500\b/);
            await page.click("同期");
            await page.show("同期済み 2件・要確認 0タスク");
        });
        const persisted = await server.yaml();
        assert.equal((persisted.match(/status: done/g) || []).length, 2);
        assert.match(persisted, /actual_work_seconds: 1500\b/);
        assert.match(persisted, /actual_work_seconds: 18000\b/);
        assert(persisted.includes(jst(split)) && persisted.includes(jst(finished)));
        assert.match(server.cli("検証"), /検証: OK/);
        const dates = new Set([split, finished].map(epoch => jst(epoch - 6 * hour).slice(0, 10)));
        const reports = [...dates].map(date => server.cli("済", date)).join("\n");
        assert(reports.includes(taskId) && reports.includes("00:25:00") && reports.includes("05:00:00"));
    } finally {
        try { await server?.stop(); } finally { await rm(work, { recursive: true, force: true, maxRetries: 3 }); }
    }
});
