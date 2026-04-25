#!/usr/bin/env python3
"""RapidOCR TCP Server"""
import asyncio, json, struct, signal, sys
from concurrent.futures import ThreadPoolExecutor
from rapidocr import RapidOCR

_engine = RapidOCR(config_path="rapidocr_config.yaml")

def ocr(image_path):
    result = _engine(image_path)
    if result is None:
        return "[]"
    txts = result.txts if hasattr(result, "txts") else [r[1] for r in result[0]]
    boxes = result.boxes if hasattr(result, "boxes") else [r[0] for r in result[0]]
    items = []
    for i in range(len(txts)):
        lt, lr, rb = boxes[i][0], boxes[i][1], boxes[i][2]
        items.append({"text": txts[i], "x": int(lt[0]), "y": int(lt[1]),
                       "width": int(lr[0] - lt[0]), "height": int(rb[1] - lr[1])})
    return json.dumps(items, ensure_ascii=False)

async def read(r):
    l = struct.unpack("!I", await r.readexactly(4))[0]
    return (await r.readexactly(l)).decode()

async def write(w, t):
    d = t.encode()
    w.write(struct.pack("!I", len(d)) + d)
    await w.drain()

async def handle(r, w, pool, sem):
    try:
        while True:
            path = (await read(r)).strip()
            if not path:
                await write(w, "ERR:empty"); continue
            async with sem:
                try:
                    t = await asyncio.get_running_loop().run_in_executor(pool, ocr, path)
                    await write(w, t)
                except Exception as e:
                    await write(w, f"ERR:{e}")
    except (asyncio.IncompleteReadError, ConnectionResetError):
        pass
    finally:
        w.close(); await w.wait_closed()

async def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "0.0.0.0"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 9000
    worker_num = int(sys.argv[3]) if len(sys.argv) > 3 else 4
    pool = ThreadPoolExecutor(max_workers=worker_num)
    sem = asyncio.Semaphore(worker_num)
    srv = await asyncio.start_server(lambda r, w: handle(r, w, pool, sem), host, port)
    print(f"listening on {host}:{port}", flush=True)
    stop = asyncio.Event()
    for s in (signal.SIGINT, signal.SIGTERM):
        signal.signal(s, lambda *_: stop.set())
    async with srv:
        await stop.wait()
        srv.close(); await srv.wait_closed(); pool.shutdown(wait=False)

asyncio.run(main())
