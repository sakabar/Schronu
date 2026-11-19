// Shared by the WASM client and the module Service Worker.
const DATABASE = "schronu_web.offline";
const STORE = "offline";
const POINTER = "active";
const STATE = "state";
const CACHE_PREFIX = "schronu-shell-v1-";
let stagedShell;

async function withStore(mode, operation) {
    const db = await new Promise((resolve, reject) => {
        const open = indexedDB.open(DATABASE, 1);
        open.onupgradeneeded = () => open.result.createObjectStore(STORE);
        open.onerror = () => reject(open.error);
        open.onblocked = () => reject(new Error("ほかのタブを閉じてから再試行してください。"));
        open.onsuccess = () => resolve(open.result);
    });
    try {
        return await new Promise((resolve, reject) => {
            const tx = db.transaction(STORE, mode, { durability: mode === "readwrite" ? "strict" : "default" });
            const store = tx.objectStore(STORE);
            let outcome;
            let failure;
            tx.oncomplete = () => resolve(outcome);
            tx.onabort = () => reject(failure || tx.error || new Error("端末への保存に失敗しました。"));
            const pointer = store.get(POINTER);
            pointer.onsuccess = () => {
                try {
                    outcome = operation(store, pointer.result);
                } catch (error) {
                    failure = error;
                    tx.abort();
                }
            };
        });
    } finally {
        db.close();
    }
}

export function activeShell() {
    return withStore("readonly", (_store, pointer) => pointer?.shell);
}

export async function loadState() {
    const pair = await withStore("readonly", (store, pointer) =>
        pointer?.revision ? [pointer.revision, store.get(STATE)] : null);
    if (!pair) return null;
    if (typeof pair[0] !== "string" || typeof pair[1].result !== "string") {
        throw new Error("端末に保存したオフラインデータを読み取れません。");
    }
    return [pair[0], pair[1].result];
}

function ensureRevision(pointer, expected) {
    if ((pointer?.revision ?? null) !== (expected ?? null)) {
        throw new Error("別のタブで保存内容が更新されました。画面を開き直してください。");
    }
}

async function deleteCache(name) {
    if (name) await caches.delete(name).catch(() => {});
}

async function discardStage(name) {
    await withStore("readwrite", (store, pointer) => {
        if (pointer?.stage !== name) return;
        if (pointer.revision) store.put({ revision: pointer.revision, shell: pointer.shell }, POINTER);
        else store.delete(POINTER);
    });
    await deleteCache(name);
}

export async function cancelPreparation() {
    const name = stagedShell;
    stagedShell = undefined;
    if (name) await discardStage(name);
}

async function reserveStage(name) {
    // List first. A cache created concurrently after this list is never a cleanup candidate.
    const previousCaches = await caches.keys();
    const active = await withStore("readwrite", (store, pointer) => {
        store.put({ ...pointer, stage: name }, POINTER);
        return pointer?.shell;
    });
    await Promise.all(previousCaches.filter(key => key.startsWith(CACHE_PREFIX) && key !== active)
        .map(deleteCache));
}

export async function saveState(expected, value) {
    const revision = crypto.randomUUID();
    const staged = stagedShell;
    stagedShell = undefined;
    try {
        const oldShell = await withStore("readwrite", (store, pointer) => {
            ensureRevision(pointer, expected);
            if (staged && pointer?.stage !== staged) {
                throw new Error("別のタブでオフライン準備が始まりました。画面を開き直してください。");
            }
            const shell = staged || pointer?.shell;
            if (!shell) throw new Error("画面のオフライン準備が完了していません。");
            store.put(value, STATE);
            store.put({ revision, shell, stage: staged ? undefined : pointer.stage }, POINTER);
            return pointer?.shell;
        });
        if (staged && oldShell !== staged) await deleteCache(oldShell);
        return revision;
    } catch (error) {
        if (staged) await discardStage(staged);
        throw error;
    }
}

export async function removeState(expected) {
    const shell = await withStore("readwrite", (store, pointer) => {
        ensureRevision(pointer, expected);
        store.delete(STATE);
        store.delete(POINTER);
        return pointer?.shell;
    });
    await deleteCache(shell);
}

const fresh = { cache: "no-store", redirect: "error", headers: { "X-Schronu-Offline-Prepare": "1" } };

export async function prepareShell() {
    if (!globalThis.isSecureContext || !navigator.serviceWorker || !globalThis.caches) {
        throw new Error("オフライン利用には、HTTPS と Service Worker に対応したブラウザが必要です。");
    }
    await navigator.serviceWorker.register("/schronu-offline-sw.js", { scope: "/", type: "module" });
    await navigator.serviceWorker.ready;
    await cancelPreparation();

    // Mark preparation requests so the active worker fetches and verifies the new build.
    const manifestResponse = await fetch("/schronu-offline-assets", fresh);
    const assets = await manifestResponse.json();
    if (!manifestResponse.ok || !Array.isArray(assets)) throw new Error("画面の構成を取得できません。");
    const urls = new Set();
    const requests = assets.map(({ path, integrity }) => {
        const url = new URL(path, location.origin);
        if (url.origin !== location.origin || !/^sha256-[A-Za-z0-9+/]{43}=$/.test(integrity)) {
            throw new Error("画面のファイル一覧を検証できません。");
        }
        urls.add(url.href);
        return new Request(url.href, { ...fresh, integrity });
    });
    if (![".wasm", ".js", ".css"].every(ext => Array.from(urls).some(url => url.endsWith(ext)))) {
        throw new Error("画面の起動ファイルが不足しています。");
    }
    const root = new URL("/", location.origin).href;
    const name = CACHE_PREFIX + crypto.randomUUID();
    try {
        await reserveStage(name);
        const cache = await caches.open(name);
        await cache.addAll([new Request(root, fresh), ...requests]);
        const page = new DOMParser().parseFromString(await (await cache.match(root)).text(), "text/html");
        const entry = page.querySelector('script[type="module"][src]');
        const styles = page.querySelectorAll('link[rel="stylesheet"][href]');
        if (!page.getElementById("main") || !entry || !styles.length ||
            ![entry.getAttribute("src"), ...Array.from(styles, link => link.getAttribute("href"))]
                .every(path => urls.has(new URL(path, root).href))) {
            throw new Error("画面と起動ファイルの版が一致しません。再読み込みして準備してください。");
        }
        stagedShell = name;
    } catch (error) {
        await discardStage(name);
        throw error;
    }
}
