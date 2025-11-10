// ws-dexparser-pretty.ts (base64 + full timing breakdown)
import WebSocket from "ws";
import { DexParser } from "./index";
import { VersionedTransaction } from "@solana/web3.js";

const API_KEY = "767f42d9-06c2-46f8-8031-9869035d6ce4";
const WS_URL = `wss://atlas-mainnet.helius-rpc.com/?api-key=${API_KEY}`;
const ACCOUNT_INCLUDE = ["pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"];

const MAX_EVENTS = 50;
const VERBOSE_JSON = false;

const WSOL = "So11111111111111111111111111111111111111112";
const sh = (x: string) => (x?.length > 12 ? `${x.slice(0, 4)}…${x.slice(-4)}` : x);
const sol = (lamports: number) => (lamports / 1_000_000_000).toFixed(9);
const fmtAmt = (amt: number | string | bigint, dec = 6) => Number(typeof amt === "bigint" ? amt.toString() : amt).toFixed(Math.min(dec, 9));
const hr = () => console.log("—".repeat(90));

const parser = new DexParser();
const ws = new WebSocket(WS_URL, { perMessageDeflate: false });

ws.on("open", () => {
  console.log("✅ Connected. Subscribing (base64)...");
  ws.send(
    JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "transactionSubscribe",
      params: [
        { accountInclude: ACCOUNT_INCLUDE, vote: false, failed: false },
        {
          commitment: "processed",
          encoding: "base64",
          transactionDetails: "full",
          maxSupportedTransactionVersion: 0,
        },
      ],
    })
  );
});

let shown = 0;

ws.on("message", async (buf) => {
  const t0 = performance.now(); // старт

  // === 1️⃣ JSON parse ===
  let msg: any;
  try {
    msg = JSON.parse(buf.toString());
  } catch {
    return;
  }
  const tJsonParsed = performance.now();

  if (msg?.method !== "transactionNotification") return;

  const r = msg.params?.result;
  if (!r) return;

  // === 2️⃣ Decode base64 transaction ===
  const txRaw = r.transaction?.transaction ?? r.transaction;
  let tDecoded: number;

  let tx;
  try {
    if (Array.isArray(txRaw)) {
      const rawBytes = Buffer.from(txRaw[0], "base64");
      tx = VersionedTransaction.deserialize(rawBytes);
      tDecoded = performance.now();
    } else {
      tx = txRaw;
      tDecoded = tJsonParsed; // No decoding needed, time is same as JSON parse
    }
  } catch (e) {
    console.log("⚠️ decode failed:", (e as Error).message);
    return;
  }

  // === 3️⃣ Prepare txLike and call parser ===
  const slot = r.slot;
  const blockTime = r.blockTime ?? Math.floor(Date.now() / 1000);
  const meta = r.transaction?.meta ?? r.meta;
  const txLike = { slot, blockTime, meta, transaction: tx };

  let res;
  const tParse0 = performance.now();
  try {
    res = parser.parseAll(txLike);
  } catch (e) {
    console.log("⚠️ Parser error:", (e as Error).message);
    return;
  }
  const tParsed = performance.now();

  // === 4️⃣ Build and print summary ===
  const signature: string = r.signature ?? r.transaction?.signatures?.[0] ?? "unknown";

  hr();
  console.log(`🔗 ${signature}  @ slot ${slot}  (${new Date(blockTime * 1000).toISOString()})`);
  console.log(`⚙️ status=${res.txStatus ?? "n/a"}  CU=${res.computeUnits ?? "?"}  fee=${fmtAmt(res?.fee?.uiAmount ?? sol(res?.fee?.amount ?? 0), 9)} SOL`);

  if (res?.aggregateTrade) {
    const t = res.aggregateTrade;
    console.log(`💱 ${fmtAmt(t.inputToken.amount, t.inputToken.decimals)} ${t.inputToken.mint === WSOL ? "SOL" : sh(t.inputToken.mint)} → ${fmtAmt(t.outputToken.amount, t.outputToken.decimals)} ${t.outputToken.mint === WSOL ? "SOL" : sh(t.outputToken.mint)} ${t.amm ? `| amm=${t.amm}` : ""}`);
  }

  if (Array.isArray(res?.trades)) {
    console.log(`🛣️ trades (${res.trades.length}):`);
    res.trades.forEach((t: any, i: number) => {
      console.log(`   #${i + 1} ${t.amm ?? t.programId ?? "DEX"}: ${fmtAmt(t.inputToken.amount, t.inputToken.decimals)} → ${fmtAmt(t.outputToken.amount, t.outputToken.decimals)}`);
    });
  }

  const tPrinted = performance.now();

  // === 5️⃣ Full timing breakdown ===
  const jsonMs = (tJsonParsed - t0).toFixed(3);
  const decodeMs = (tDecoded - tJsonParsed).toFixed(3);
  const parseMs = (tParsed - tParse0).toFixed(3);
  const printMs = (tPrinted - tParsed).toFixed(3);
  const totalMs = (tPrinted - t0).toFixed(3);

  console.log(`⏱️ Timing: JSON=${jsonMs}ms  Decode=${decodeMs}ms  Parse=${parseMs}ms  Print=${printMs}ms  TOTAL=${totalMs}ms`);

  if (VERBOSE_JSON) {
    console.log("— raw ParseResult —");
    console.dir(res, { depth: 5 });
  }

  if (++shown >= MAX_EVENTS) {
    hr();
    console.log(`✅ shown ${shown} events — closing`);
    ws.close();
  }
});

ws.on("error", (e) => console.error("WS error:", (e as Error).message));
ws.on("close", (c) => console.log("WS closed:", c));
setInterval(() => {
  if (ws.readyState === WebSocket.OPEN) ws.ping();
}, 60_000);
