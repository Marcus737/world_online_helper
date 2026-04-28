#!/usr/bin/env python3
"""RapidOCR TCP Server - 多客户端，纯内存处理"""

import asyncio
import json
import struct
import signal
import sys
import os
from concurrent.futures import ThreadPoolExecutor
from rapidocr import RapidOCR

_engine = RapidOCR(config_path="rapidocr_config.yaml")


def ocr(image_bytes: bytes) -> str:
    result = _engine(image_bytes)
    if result is None:
        return "[]"
    txts = result.txts if hasattr(result, "txts") else [r[1] for r in result[0]]
    boxes = result.boxes if hasattr(result, "boxes") else [r[0] for r in result[0]]
    items = []
    for i in range(len(txts)):
        lt, lr, rb = boxes[i][0], boxes[i][1], boxes[i][2]
        items.append({
            "text": txts[i],
            "x": int(lt[0]), "y": int(lt[1]),
            "width": int(lr[0] - lt[0]), "height": int(rb[1] - lr[1]),
        })
    return json.dumps(items, ensure_ascii=False)


async def read_frame(reader):
    type_len = struct.unpack("!I", await reader.readexactly(4))[0]
    msg_type = (await reader.readexactly(type_len)).decode()
    payload = None
    if msg_type == "Image":
        payload_len = struct.unpack("!I", await reader.readexactly(4))[0]
        if payload_len > 0:
            payload = await reader.readexactly(payload_len)
    return msg_type, payload


async def write_response(writer, text):
    data = text.encode()
    writer.write(struct.pack("!I", len(data)) + data)
    await writer.drain()


async def handle(reader, writer, *, pool, sem, server_stop):
    peer = writer.get_extra_info("peername")
    print(f"[{peer}] 已连接", flush=True)
    try:
        while not server_stop.is_set():
            try:
                msg_type, payload = await read_frame(reader)
            except (asyncio.IncompleteReadError, ConnectionResetError, BrokenPipeError):
                print(f"[{peer}] 断开", flush=True)
                break
            if msg_type == "Exit":
                await write_response(writer, "OK")
                print(f"[{peer}] 发送 Exit，关闭服务", flush=True)
                server_stop.set()
                break
            elif msg_type == "Image":
                if not payload:
                    await write_response(writer, "ERR:empty image")
                    continue
                async with sem:
                    try:
                        result = await asyncio.get_running_loop().run_in_executor(
                            pool, ocr, payload
                        )
                        await write_response(writer, result)
                    except Exception as e:
                        await write_response(writer, f"ERR:{e}")
            else:
                await write_response(writer, f"ERR:unknown type '{msg_type}'")
    except Exception as e:
        print(f"[{peer}] 错误: {e}", flush=True)
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass


async def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "0.0.0.0"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 9000
    worker_num = int(sys.argv[3]) if len(sys.argv) > 3 else 4

    pool = ThreadPoolExecutor(max_workers=worker_num)
    sem = asyncio.Semaphore(worker_num)
    server_stop = asyncio.Event()

    srv = await asyncio.start_server(
        lambda r, w: handle(r, w, pool=pool, sem=sem, server_stop=server_stop),
        host, port,
    )

    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, server_stop.set)
        except NotImplementedError:
            pass

    print(json.dumps({
        "host": host, "port": port, "pid": os.getpid(),
        "workers": worker_num,
    }), flush=True)

    async with srv:
        await server_stop.wait()
    srv.close()
    await srv.wait_closed()
    pool.shutdown(wait=False)
    print("Server stopped.", flush=True)


if __name__ == "__main__":
    asyncio.run(main())
