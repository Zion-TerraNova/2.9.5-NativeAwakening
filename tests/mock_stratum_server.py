#!/usr/bin/env python3
"""
Mock Stratum Server for ZION miner testing.
Supports both XMRig (login) and Stratum v1 (subscribe/authorize) protocols.

Usage:
    python3 tests/mock_stratum_server.py [port]  (default: 13333)
"""
import asyncio
import json
import uuid
import hashlib
import time
import sys
import os

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 13333
DIFFICULTY = 1000
ALGO = "cosmic_harmony_v3"

# Generate fake block template
def make_job(height: int = 100) -> dict:
    ts = int(time.time())
    blob = hashlib.sha256(f"zion-test-{ts}-{height}".encode()).hexdigest()
    blob = blob * 5  # ~160 hex chars
    blob = blob[:152]  # 76 bytes = 152 hex chars
    target = f"{DIFFICULTY:08x}"
    job_id = f"h{height}-{ts:08x}-{ALGO}"
    return {
        "job_id": job_id,
        "blob": blob,
        "target": target,
        "difficulty": DIFFICULTY,
        "height": height,
        "algo": ALGO,
        "seed_hash": "0" * 64,
        "cosmic_state0_endian": "little",
    }

class MockStratumServer:
    def __init__(self):
        self.height = 100
        self.connections = {}
        self.shares_accepted = 0
        self.shares_rejected = 0

    async def handle_client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        addr = writer.get_extra_info("peername")
        session_id = str(uuid.uuid4())
        protocol = None  # Will be set on first method
        print(f"[+] New connection from {addr}, session={session_id[:8]}...")

        try:
            while True:
                data = await asyncio.wait_for(reader.readline(), timeout=120)
                if not data:
                    break

                line = data.decode("utf-8").strip()
                if not line:
                    continue

                print(f"[<] {addr}: {line[:200]}")

                try:
                    msg = json.loads(line)
                except json.JSONDecodeError as e:
                    print(f"[!] JSON parse error: {e}")
                    continue

                method = msg.get("method", "")
                msg_id = msg.get("id")
                params = msg.get("params", {})
                response = None

                # ── XMRig login ──────────────────────────────────────
                if method == "login":
                    protocol = "xmrig"
                    wallet = params.get("login", "unknown")
                    worker = params.get("rigid", "default")
                    agent = params.get("agent", "unknown")
                    algo_hint = params.get("pass", "")
                    print(f"[*] XMRig LOGIN: wallet={wallet}, worker={worker}, agent={agent}, algo={algo_hint}")

                    job = make_job(self.height)
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": {
                            "id": session_id,
                            "job": job,
                            "status": "OK",
                        },
                    }

                # ── XMRig submit ─────────────────────────────────────
                elif method == "submit":
                    nonce = params.get("nonce", "?")
                    job_id = params.get("job_id", "?")
                    result_hash = params.get("result", "?")
                    self.shares_accepted += 1
                    print(f"[*] XMRig SUBMIT: job={job_id}, nonce={nonce}, shares_accepted={self.shares_accepted}")
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": {"status": "OK"},
                    }

                # ── Stratum v1: mining.subscribe ─────────────────────
                elif method == "mining.subscribe":
                    protocol = "stratum"
                    print(f"[*] Stratum SUBSCRIBE")
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": [
                            [["mining.notify", session_id], ["mining.set_difficulty", session_id]],
                            "",  # extranonce1
                            4,   # extranonce2_size
                        ],
                    }

                # ── Stratum v1: mining.authorize ─────────────────────
                elif method == "mining.authorize":
                    username = params[0] if isinstance(params, list) and len(params) > 0 else "?"
                    password = params[1] if isinstance(params, list) and len(params) > 1 else ""
                    print(f"[*] Stratum AUTHORIZE: user={username}, pass={password}")
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": True,
                    }
                    # Send first job after authorize
                    await asyncio.sleep(0.1)
                    await self._send_notify(writer, self.height)

                # ── Stratum v1: mining.submit ────────────────────────
                elif method == "mining.submit":
                    arr = params if isinstance(params, list) else []
                    self.shares_accepted += 1
                    print(f"[*] Stratum SUBMIT: params={arr}, shares_accepted={self.shares_accepted}")
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": True,
                    }

                # ── keepalived ───────────────────────────────────────
                elif method == "keepalived":
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": {"status": "KEEPALIVED"},
                    }

                # ── getjob ───────────────────────────────────────────
                elif method == "getjob":
                    job = make_job(self.height)
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "result": job,
                    }

                # ── unknown ──────────────────────────────────────────
                else:
                    print(f"[?] Unknown method: {method}")
                    response = {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "error": {"code": -1, "message": f"Unknown method: {method}"},
                    }

                if response:
                    resp_json = json.dumps(response) + "\n"
                    print(f"[>] {addr}: {resp_json[:200].strip()}")
                    writer.write(resp_json.encode("utf-8"))
                    await writer.drain()

        except asyncio.TimeoutError:
            print(f"[-] {addr}: read timeout (120s)")
        except ConnectionResetError:
            print(f"[-] {addr}: connection reset")
        except Exception as e:
            print(f"[-] {addr}: error: {e}")
        finally:
            print(f"[-] {addr}: disconnected (accepted={self.shares_accepted}, rejected={self.shares_rejected})")
            writer.close()

    async def _send_notify(self, writer: asyncio.StreamWriter, height: int):
        """Send mining.notify to Stratum v1 client."""
        job = make_job(height)
        notify = {
            "id": None,
            "method": "mining.notify",
            "params": [
                job["job_id"],
                job["blob"],
                job["target"],
                job["height"],
                job["algo"],
                job["seed_hash"],
                True,  # clean_jobs
            ],
        }
        data = json.dumps(notify) + "\n"
        print(f"[>] mining.notify: job={job['job_id']}")
        writer.write(data.encode("utf-8"))
        await writer.drain()

    async def job_broadcaster(self, writers: list):
        """Periodically broadcast new jobs."""
        while True:
            await asyncio.sleep(15)
            self.height += 1
            print(f"\n[*] New block template height={self.height}")
            # In a real implementation, we'd broadcast to all connected clients

    async def start(self):
        server = await asyncio.start_server(self.handle_client, "0.0.0.0", PORT)
        print(f"╔══════════════════════════════════════════════════╗")
        print(f"║  🧪 ZION Mock Stratum Server                    ║")
        print(f"║  Port: {PORT:<42} ║")
        print(f"║  Algo: {ALGO:<42} ║")
        print(f"║  Diff: {DIFFICULTY:<42} ║")
        print(f"║  Protocols: XMRig (login) + Stratum v1          ║")
        print(f"╚══════════════════════════════════════════════════╝")
        print(f"Waiting for miners...\n")
        async with server:
            await server.serve_forever()

if __name__ == "__main__":
    mock = MockStratumServer()
    try:
        asyncio.run(mock.start())
    except KeyboardInterrupt:
        print(f"\n[*] Shutting down. Shares: accepted={mock.shares_accepted}, rejected={mock.shares_rejected}")
