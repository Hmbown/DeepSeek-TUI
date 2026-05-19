import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

test("ThreadStore is initialized before bridge startup opens it", async () => {
  const source = await fs.readFile(path.join(__dirname, "../src/index.mjs"), "utf8");
  const declaration = source.indexOf("class ThreadStore");
  const startupUse = source.indexOf("await ThreadStore.open");

  assert.notEqual(declaration, -1);
  assert.notEqual(startupUse, -1);
  assert.ok(declaration < startupUse);
});

test("active turns are reattached only after the websocket client starts", async () => {
  const source = await fs.readFile(path.join(__dirname, "../src/index.mjs"), "utf8");
  const wsStart = source.indexOf("wsClient.start(");
  const reattachCall = source.indexOf("void reattachActiveTurns()");
  const reattachDecl = source.indexOf("async function reattachActiveTurns");
  const listChats = source.indexOf("listChats()");

  assert.notEqual(wsStart, -1);
  assert.notEqual(reattachCall, -1);
  assert.notEqual(reattachDecl, -1);
  assert.notEqual(listChats, -1);
  assert.ok(wsStart < reattachCall, "reattach must run after wsClient.start");
});
