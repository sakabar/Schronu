import { activeShell } from "/schronu-offline-db.js";

self.addEventListener("install", event => event.waitUntil(self.skipWaiting()));
self.addEventListener("activate", event => event.waitUntil(self.clients.claim()));

self.addEventListener("fetch", event => {
    const request = event.request;
    const url = new URL(request.url);
    if (request.method !== "GET" || url.origin !== self.location.origin || request.headers.has("X-Schronu-Offline-Prepare")) return;
    const isPage = request.mode === "navigate" && url.pathname === "/";
    const isAsset = /\.(?:js|mjs|css|wasm)$/.test(url.pathname);
    if (!isPage && !isAsset) return;
    event.respondWith((async () => {
        try {
            const name = await activeShell();
            if (name) {
                const cached = await caches.match(isPage ? "/" : request, { cacheName: name });
                if (cached) return cached;
            }
        } catch {
            // A restricted or evicted browser store must not block ordinary online navigation.
        }
        return fetch(request);
    })());
});
