// Run with SCHRONU_TEST_CHROME pointing to a Chromium executable.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const binary = process.env.SCHRONU_TEST_CHROME;
const client = await readFile(new URL("../assets/offline-db.js", import.meta.url));
const worker = await readFile(new URL("../assets/offline-worker.js", import.meta.url));
const injectStorageFailure = `
self.addEventListener("message", event => {
    const [prototype, method] = event.data === "indexedDB"
        ? [IDBFactory.prototype, "open"] : [CacheStorage.prototype, "match"];
    const original = prototype[method];
    prototype[method] = function () {
        prototype[method] = original;
        throw new DOMException("storage unavailable", "SecurityError");
    };
    event.ports[0].postMessage("ready");
});
`;
const page = '<!doctype html><div id="main">Schronu</div><link rel="stylesheet" href="/assets/main.css"><script type="module" src="/wasm/test.js"></script>';
const script = `
import { loadState, saveState, removeState, prepareShell, cancelPreparation } from "./snippets/db.js";
function check(value, message) { if (!value) throw Error(message); }
async function run() {
    await WebAssembly.instantiateStreaming(fetch("/wasm/test.wasm"));
    if (location.search === "?restored") {
        const [token, payload] = await loadState();
        check(payload === "retained", "saved state after cold navigation");
        await removeState(token);
        check(await loadState() === null, "acknowledged removal");
        return;
    }
    check(await loadState() === null, "clean first mount");
    await prepareShell();
    const initial = await saveState(null, "initial");
    if (!navigator.serviceWorker.controller) await new Promise(resolve =>
        navigator.serviceWorker.addEventListener("controllerchange", resolve, { once: true }));
    await fetch("/test/probe", { method: "POST" });
    for (const storage of ["indexedDB", "cacheStorage"]) {
        const channel = new MessageChannel();
        const ready = new Promise(resolve => { channel.port1.onmessage = resolve; });
        navigator.serviceWorker.controller.postMessage(storage, [channel.port2]);
        await ready;
        channel.port1.close();
        const response = await fetch("/assets/main.css", { cache: "reload" });
        check(response.headers.has("X-Network-Probe"), storage + " failure must fall back to network");
    }
    await prepareShell();
    await cancelPreparation();
    check((await caches.keys()).length === 1, "cancelled preparation leaked its cache");
    const orphan = "schronu-shell-v1-interrupted";
    await caches.open(orphan);
    await prepareShell();
    check(!(await caches.has(orphan)), "interrupted staging cache leaked");
    await cancelPreparation();
    const updates = await Promise.allSettled([saveState(initial, "retained"), saveState(initial, "retained")]);
    check(updates.filter(r => r.status === "fulfilled").length === 1, "compare-and-set must have one winner");
    await removeState(initial).then(() => { throw Error("stale delete accepted"); }, () => {});
    await fetch("/test/corrupt", { method: "POST" });
    await prepareShell().then(() => { throw Error("damaged asset accepted"); }, () => {});
    check((await loadState())[1] === "retained", "failed replacement removed state");
    await fetch("/test/disconnect", { method: "POST" });
    location.replace("/?restored");
    await new Promise(() => {});
}
run().then(() => fetch("/test/success", { method: "POST" }), error => fetch("/test/failure", {
    method: "POST", body: String(error.stack || error)
}));
`;

test("offline shell, interrupted replacement and concurrent IndexedDB writes", {
    skip: !binary,
    timeout: 60_000,
}, async t => {
    const assets = new Map([
        ["/assets/main.css", ["text/css", Buffer.from("body { color: black; }")]],
        ["/wasm/test.js", ["text/javascript", Buffer.from(script)]],
        ["/wasm/snippets/db.js", ["text/javascript", client]],
        ["/wasm/test.wasm", ["application/wasm", Buffer.from([0, 97, 115, 109, 1, 0, 0, 0])]],
    ]);
    let corrupt = false;
    let offline = false;
    let probe = false;
    const missing = [];
    const result = Promise.withResolvers();
    const server = createServer(async (req, res) => {
        const path = new URL(req.url, "http://localhost").pathname;
        if (path.startsWith("/test/")) {
            if (path === "/test/probe") probe = true;
            if (path === "/test/corrupt") corrupt = true;
            if (path === "/test/disconnect") offline = true;
            if (path === "/test/success") result.resolve();
            if (path === "/test/failure") {
                let failure = "";
                for await (const part of req) failure += part;
                result.reject(new Error(failure));
            }
            res.end();
        } else if (offline) {
            if (path === "/" || assets.has(path)) missing.push(path);
            res.writeHead(503).end();
        } else if (path === "/schronu-offline-assets") {
            res.setHeader("Content-Type", "application/json");
            res.end(JSON.stringify(Array.from(assets, ([path, [, bytes]]) => ({
                path, integrity: "sha256-" + createHash("sha256").update(bytes).digest("base64"),
            }))));
        } else if (path === "/schronu-offline-sw.js" || path === "/schronu-offline-db.js") {
            res.setHeader("Content-Type", "text/javascript");
            res.end(path.endsWith("-sw.js") ? worker + injectStorageFailure : client);
        } else if (assets.has(path)) {
            const [contentType, bytes] = assets.get(path);
            res.setHeader("Content-Type", contentType);
            if (probe) res.setHeader("X-Network-Probe", "1");
            res.end(corrupt && path.endsWith(".css") ? "damaged" : bytes);
        } else if (path === "/") {
            res.setHeader("Content-Type", "text/html");
            res.end(page);
        } else {
            res.writeHead(404).end();
        }
    });
    await new Promise((resolve, reject) => server.once("error", reject).listen(0, "127.0.0.1", resolve));
    t.after(() => { server.closeAllConnections(); server.close(); });
    const profile = await mkdtemp(join(tmpdir(), "schronu-offline-test-"));
    const browser = spawn(binary, [
        "--headless", "--disable-gpu", "--disable-background-networking", "--no-first-run",
        "--no-default-browser-check", "--user-data-dir=" + profile,
        "http://127.0.0.1:" + server.address().port + "/",
    ], { stdio: "ignore" });
    const exit = once(browser, "exit");
    t.after(async () => {
        browser.kill();
        await exit;
        await rm(profile, { recursive: true, force: true, maxRetries: 3 });
    });
    const timeout = setTimeout(() => result.reject(Error("browser timed out; uncached requests: " + missing)), 40_000);
    try {
        await Promise.race([result.promise, exit.then(([code]) => { throw Error("browser exited: " + code); })]);
        assert.deepEqual(missing, []);
    } finally {
        clearTimeout(timeout);
    }
});
